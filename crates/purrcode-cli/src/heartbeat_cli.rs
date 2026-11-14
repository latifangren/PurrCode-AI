// SPDX-License-Identifier: GPL-3.0-or-later

//! The `purrcode heartbeat` subcommand (v0.3 F22, ADR-014 item 6).
//!
//! - `heartbeat` / `heartbeat status` — read-only: daemon settings,
//!   task file (workspace then global), task count, preferences
//!   window, pending heartbeat deliveries. Never creates the
//!   template and never calls the model.
//! - `heartbeat run` — fire one tick in the foreground (same rails
//!   the gateway tick uses: read-only allowlist, deny-always, no
//!   prompting). The delivery is enqueued for the gateway's next
//!   drain (or the next TUI open when there is no Telegram chat) —
//!   this command does not send to Telegram itself.

use crate::onboard;
use purrcode_api::error::{PurrError, Result};
use purrcode_config::Config;
use purrcode_service::cron::runner::build_runner_deps;
use purrcode_service::heartbeat::prefs::ProactivePrefs;
use purrcode_service::heartbeat::runner::{HeartbeatOutcome, HeartbeatRunner};
use purrcode_service::heartbeat::tasks::find_task_file;

/// Bundled CLI args for `purrcode-ai heartbeat <action>`.
/// (Bundled into one struct: clippy rejects too many fn args.)
pub struct HeartbeatArgs {
    pub action: String,
}

pub async fn run_heartbeat_command(config: &Config, args: HeartbeatArgs) -> Result<()> {
    match args.action.as_str() {
        "status" => cmd_status(config),
        "run" => cmd_run(config).await,
        other => Err(PurrError::Session(format!(
            "Unknown heartbeat action '{other}' (expected: status, run)"
        ))),
    }
}

fn cmd_status(config: &Config) -> Result<()> {
    let purrcode_dir = onboard::get_purrcode_dir().unwrap_or_else(|_| ".purrcode".into());
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let enabled = config.heartbeat.as_ref().map(|c| c.enabled).unwrap_or(true);
    let interval = config
        .heartbeat
        .as_ref()
        .map(|c| c.interval_mins())
        .unwrap_or(30);
    println!("🐾 Heartbeat (proactive checks, v0.3 F22)");
    println!("   enabled: {enabled} — interval: {interval} min (min 5)");
    match find_task_file(&workspace_root, &purrcode_dir) {
        Some(path) => {
            let raw = std::fs::read_to_string(&path).unwrap_or_default();
            let tasks = purrcode_service::heartbeat::tasks::parse_tasks(&raw);
            println!("   tasks: {} in {}", tasks.len(), path.display());
            for (i, task) in tasks.iter().enumerate() {
                println!("     {}. {}", i + 1, task);
            }
        }
        None => println!(
            "   tasks: no HEARTBEAT.md yet (one is created at {} on the next tick)",
            purrcode_dir.join("HEARTBEAT.md").display()
        ),
    }
    let prefs = ProactivePrefs::load(&purrcode_dir);
    match prefs.window {
        Some((start, end)) => println!("   window: {start}–{end} (local time)"),
        None => println!("   window: none (ticks run any time)"),
    }
    if !prefs.never_about.is_empty() {
        println!("   never tell me about: {}", prefs.never_about.join("; "));
    }
    let queue = purrcode_service::cron::queue::DeliveryQueue::new(&purrcode_dir);
    match queue.pending() {
        Ok(pending) => {
            let hb = pending
                .iter()
                .filter(|d| {
                    matches!(
                        d.kind,
                        purrcode_service::cron::queue::DeliveryKind::Heartbeat
                    )
                })
                .count();
            println!(
                "   pending heartbeat deliveries: {hb} ({} total pending)",
                pending.len()
            );
        }
        Err(_) => println!("   pending heartbeat deliveries: ? (queue unreadable)"),
    }
    Ok(())
}

async fn cmd_run(config: &Config) -> Result<()> {
    // The gateway tick honors this flag; a foreground `heartbeat run`
    // must too, or the disable switch would only work on one of the
    // two trigger paths (v0.3 F22 fix, re-checker F3).
    if !config.heartbeat.as_ref().map(|c| c.enabled).unwrap_or(true) {
        return Err(PurrError::Config(
            "Heartbeat is disabled in config ([heartbeat] enabled = false). Set enabled = true to run a tick."
                .to_string(),
        ));
    }
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let deps = build_runner_deps(config, &workspace_root, None).await?;
    let runner = HeartbeatRunner::new(deps);
    let outcome = runner.run_once(None).await?;
    print_outcome(&outcome);
    Ok(())
}

fn print_outcome(outcome: &HeartbeatOutcome) {
    use purrcode_service::heartbeat::runner::HeartbeatDecision as D;
    match outcome.decision {
        D::NoTasks => println!("🐾 Heartbeat: no tasks — nothing to check."),
        D::OutsideWindow => println!("🐾 Heartbeat: outside the proactive window — skipped."),
        D::SkippedByDecider => println!("🐾 Heartbeat: decider said skip — nothing worth running."),
        D::Ran => {
            if outcome.lines.is_empty() {
                println!("🐾 Heartbeat: ran, all silent — no delivery.");
            } else {
                println!("🐾 Heartbeat: {} line(s):", outcome.lines.len());
                for line in &outcome.lines {
                    println!("   • {line}");
                }
            }
        }
    }
    if outcome.delivered {
        println!(
            "   delivery {} enqueued (the gateway sends it on its next tick; the TUI shows it at the next open).",
            outcome.delivery_id.as_deref().unwrap_or("?")
        );
    }
}

#[cfg(test)]
mod tests {
    /// The bare `heartbeat` subcommand really defaults to `status`
    /// — parsed through clap itself, not asserted against a struct
    /// literal the test itself wrote (v0.3 F22 fix, build checker F1).
    #[test]
    fn test_bare_heartbeat_defaults_to_status() {
        use clap::Parser;
        let cli = crate::cli::Cli::try_parse_from(["purrcode-ai", "heartbeat"])
            .expect("bare `heartbeat` must parse");
        match cli.command {
            Some(crate::cli::Commands::Heartbeat { action }) => {
                assert_eq!(action, "status")
            }
            other => panic!("expected Heartbeat subcommand, got {other:?}"),
        }
    }
}
