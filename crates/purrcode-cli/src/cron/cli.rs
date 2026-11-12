// SPDX-License-Identifier: GPL-3.0-or-later

//! The `purrcode cron` management CLI (v0.3 F19, ADR-014).
//!
//! Actions: `add`, `list`, `remove`, `enable`, `disable`, `deliveries`,
//! `run` (fire a job now, in the foreground, under the same rails as
//! the gateway tick). Adding a job parses the schedule (deterministic
//! NL parser), snapshots the preset rails, validates the tool allowlist,
//! and asks for an interactive confirmation unless `--yes` is passed —
//! the user sees exactly what was understood before the job is saved.

use chrono::Local;
use purrcode_api::error::{PurrError, Result};
use purrcode_config::{Config, ModePresetLoader};
use std::io::IsTerminal;
use std::path::Path;

use crate::cron::parse_natural;
use crate::cron::store::{new_job_id, validate_tools, PresetSnapshot, DEFAULT_ALLOWLIST};
use crate::cron::{CronJob, CronStore, DeliveryQueue, DeliveryTarget, JobSchedule};

/// Arguments for the `purrcode cron` subcommand (bundled so the handler
/// stays under clippy's argument-count lint).
pub struct CronArgs<'a> {
    pub action: &'a str,
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub prompt: Option<String>,
    pub allow: Option<String>,
    pub preset: Option<String>,
    pub yes: bool,
    pub config: &'a Config,
    pub workspace_root: &'a Path,
    pub purrcode_dir: &'a Path,
}

/// Run the `purrcode cron` subcommand. `purrcode_dir` is `~/.purrcode`.
pub async fn run_cron_command(args: CronArgs<'_>) -> Result<()> {
    match args.action {
        "add" => cmd_add(&args),
        "list" | "" => cmd_list(args.purrcode_dir),
        "remove" => cmd_remove(args.name, args.purrcode_dir),
        "enable" => cmd_enable(args.name, true, args.purrcode_dir),
        "disable" => cmd_enable(args.name, false, args.purrcode_dir),
        "deliveries" => cmd_deliveries(args.purrcode_dir),
        "run" => cmd_run(&args).await,
        other => Err(PurrError::Config(format!(
            "Unknown 'cron' action '{other}'. Available: add, list, remove, enable, disable, deliveries, run"
        ))),
    }
}

fn load_store(purrcode_dir: &Path) -> Result<CronStore> {
    CronStore::load(purrcode_dir)
}

fn cmd_list(purrcode_dir: &Path) -> Result<()> {
    let store = load_store(purrcode_dir)?;
    let jobs = store.jobs();
    if jobs.is_empty() {
        println!("🐾 No cron jobs yet. Add one:");
        println!("   purrcode-ai cron add pagi-ringkas --schedule \"tiap pagi jam 8\" --prompt \"ringkas email\"");
        return Ok(());
    }
    println!("🐾 Cron jobs ({}):", jobs.len());
    for job in jobs {
        println!(
            "  - {} [{}] {} — {}",
            job.id,
            if job.enabled { "enabled" } else { "disabled" },
            job.name,
            job.schedule.display
        );
        println!(
            "      next: {} | tools: {} | preset: {} | prompt: {}",
            job.next_run
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".to_string()),
            if job.tools.is_empty() {
                "(none — fail-closed)".to_string()
            } else {
                job.tools.join(", ")
            },
            job.preset.name,
            truncate(&job.prompt, 60)
        );
    }
    Ok(())
}

fn cmd_remove(name: Option<String>, purrcode_dir: &Path) -> Result<()> {
    let needle = name.ok_or_else(|| {
        PurrError::Config("Usage: purrcode-ai cron remove <job id or name>".to_string())
    })?;
    let mut store = load_store(purrcode_dir)?;
    let job = store.remove(&needle)?;
    println!("✓ Removed cron job '{}' ({})", job.name, job.id);
    Ok(())
}

fn cmd_enable(name: Option<String>, enabled: bool, purrcode_dir: &Path) -> Result<()> {
    let needle = name.ok_or_else(|| {
        PurrError::Config(format!(
            "Usage: purrcode-ai cron {} <job id or name>",
            if enabled { "enable" } else { "disable" }
        ))
    })?;
    let mut store = load_store(purrcode_dir)?;
    let job = store
        .find_mut(&needle)
        .ok_or_else(|| PurrError::Session(format!("No cron job matching '{needle}'")))?;
    job.enabled = enabled;
    let (id, job_name) = (job.id.clone(), job.name.clone());
    store.save()?;
    println!(
        "✓ Cron job '{}' ({}) is now {}",
        job_name,
        id,
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn cmd_deliveries(purrcode_dir: &Path) -> Result<()> {
    let queue = DeliveryQueue::new(purrcode_dir);
    let pending = queue.pending()?;
    if pending.is_empty() {
        println!("🐾 No pending cron deliveries.");
        return Ok(());
    }
    println!("🐾 Pending cron deliveries ({}):", pending.len());
    for d in pending {
        println!(
            "  - {} [{}] {} — {}",
            d.id,
            d.job_name,
            d.created_at.format("%Y-%m-%d %H:%M"),
            d.summary
        );
    }
    Ok(())
}

/// Fire a job now, in the foreground, under the same rails the gateway
/// tick uses (pinned preset, job allowlist, no prompting) — except
/// due-ness: a manual fire is allowed before the scheduled time, as
/// long as the job is enabled and no run of it is already in flight.
/// The result prints here; the delivery is still enqueued for the
/// normal drain, so the job's Telegram chat gets it too.
async fn cmd_run(args: &CronArgs<'_>) -> Result<()> {
    let name = args
        .name
        .clone()
        .ok_or_else(|| PurrError::Config("Usage: purrcode-ai cron run <name-or-id>".to_string()))?;
    // Look the job up BEFORE resolving the model: a mistyped name must
    // report "no such job", not "no model configured" (build F1).
    let mut store = load_store(args.purrcode_dir)?;
    if store.find(&name).is_none() {
        return Err(PurrError::Session(format!("No cron job matching '{name}'")));
    }
    let deps = crate::cron::build_runner_deps(args.config, args.workspace_root, None).await?;
    let runner = crate::cron::CronRunner::new(deps);
    let outcome = runner.run_job(&mut store, &name).await?;
    println!(
        "🐾 Cron job '{}' ({}) fired (session {}):",
        outcome.job_name, outcome.job_id, outcome.session_id
    );
    if outcome.ok {
        println!("{}", outcome.detail);
        Ok(())
    } else {
        Err(PurrError::Tool {
            tool: "cron".to_string(),
            message: outcome.summary,
        })
    }
}

fn cmd_add(args: &CronArgs<'_>) -> Result<()> {
    let schedule_text = args.schedule.clone().ok_or_else(|| {
        PurrError::Config(
            "Usage: purrcode-ai cron add <name> --schedule \"tiap pagi jam 8\" --prompt \"ringkas email\"  [--allow write,edit] [--preset tabby]"
                .to_string(),
        )
    })?;
    let prompt_text = args.prompt.clone().ok_or_else(|| {
        PurrError::Config("`cron add` needs --prompt <what the job should do>".to_string())
    })?;

    let now = Local::now();
    let parsed = parse_natural(&schedule_text, now)?;

    // Preset rails pinned at creation (default: the config preset).
    let pname = args
        .preset
        .clone()
        .unwrap_or_else(|| args.config.preset.clone());
    let preset = ModePresetLoader::load_by_name(&pname, args.workspace_root)?;
    if preset.guardrail == purrcode_config::GuardrailMode::God {
        return Err(PurrError::Session(format!(
            "Cron job may not use a God-guardrail preset ('{pname}'): cron jobs never run oyen"
        )));
    }
    if preset.approval == purrcode_config::ApprovalMode::Yolo {
        return Err(PurrError::Session(format!(
            "Cron job may not use a yolo-approval preset ('{pname}'): cron jobs never run yolo"
        )));
    }
    let snapshot = PresetSnapshot {
        name: preset.name.clone(),
        approval: format!("{:?}", preset.approval).to_lowercase(),
        guardrail: format!("{:?}", preset.guardrail).to_lowercase(),
        sandbox: format!("{:?}", preset.sandbox).to_lowercase(),
    };

    // Tool allowlist: explicit --allow wins; `--allow ""` = explicit empty
    // (nothing allowed, fail-closed, never widened); omitted = Meowstro's
    // read-only set (no writes unless declared).
    let tools: Vec<String> = match args.allow.clone() {
        Some(raw) => {
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                raw.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            }
        }
        None => DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect(),
    };
    validate_tools(&tools)?;

    let job_name = args
        .name
        .clone()
        .unwrap_or_else(|| format!("job-{}", &new_job_id()[5..]));
    let delivery = match args.config.telegram.as_ref() {
        Some(tg) if !tg.allow_from.is_empty() => DeliveryTarget::Telegram {
            chat_id: tg.allow_from[0],
        },
        _ => DeliveryTarget::QueueOnly,
    };
    let job = CronJob {
        id: new_job_id(),
        name: job_name.clone(),
        enabled: true,
        schedule: JobSchedule {
            expr: parsed.expr.clone(),
            display: parsed.display.clone(),
            once: parsed.once,
            at: if parsed.once {
                Some(parsed.next_run)
            } else {
                None
            },
        },
        prompt: prompt_text,
        tools: tools.clone(),
        preset: snapshot,
        delivery,
        created_at: now,
        last_run: None,
        next_run: Some(parsed.next_run),
        run_count: 0,
        failure_count: 0,
        last_status: None,
    };

    // Confirm step: the user sees exactly what was understood before the
    // job is saved. Skipped with --yes (e.g. scripts).
    if !args.yes {
        println!("🐾 New cron job:");
        println!("   name:     {}", job.name);
        println!("   schedule: {}", parsed.display);
        println!(
            "   next run: {}",
            parsed.next_run.format("%Y-%m-%d %H:%M %Z")
        );
        println!("   preset:   {} (pinned)", preset.name);
        println!(
            "   tools:    {}",
            if tools.is_empty() {
                "(none — fail-closed)".to_string()
            } else {
                tools.join(", ")
            }
        );
        println!("   prompt:   {}", truncate(&job.prompt, 80));
        if !std::io::stdin().is_terminal() {
            return Err(PurrError::Config(
                "Refusing to create a cron job without confirmation on a non-interactive stdin — pass --yes to confirm, or run interactively."
                    .to_string(),
            ));
        }
        print!("Create this job? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|e| PurrError::Session(format!("Failed to read confirmation: {e}")))?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled — no job created.");
            return Ok(());
        }
    }

    let mut store = load_store(args.purrcode_dir)?;
    store.add(job)?;
    println!("✓ Cron job '{job_name}' created.");
    println!(
        "   Next run: {}",
        parsed.next_run.format("%Y-%m-%d %H:%M %Z")
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abcdefghij", 5), "abcde…");
    }

    // (The CLI handler itself is exercised by the smoke test in the batch
    // verification — unit tests cover the pure layers: schedule/store/queue.)
}
