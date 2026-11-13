// SPDX-License-Identifier: GPL-3.0-or-later

//! `CronRegistry` implementation for the WebUI (v0.4 B6).
//!
//! A thin adapter: every call loads a fresh `CronStore` from
//! disk, runs the same code the CLI `cron add` path runs — the
//! same NL schedule parser (`parse_natural`), the same preset
//! pinning (God/yolo refused), the same allowlist validation —
//! and saves through the store, so the WebUI never re-implements
//! scheduling logic and never writes `jobs.jsonc` itself. Disk
//! is the source of truth; a fresh load per call can never act
//! on a stale in-memory index (the gateway tick reads the same
//! file). There is no runtime-only cron state to share, unlike
//! the B4 plugin manager's mounted set, so no shared instance
//! is needed — but mutations are serialized with a lock, because
//! load → mutate → save is a read-modify-write on one file and
//! two concurrent web requests must not lose each other's job
//! (same class as the B5 skill-mutation race, fix pass r1 P2-4).
//!
//! The interactive confirm step of `cron add` has no web
//! equivalent: the request itself is the confirmation, and the
//! UI previews the parsed schedule first via `parse_schedule`
//! (the same parser), so the user still sees exactly what was
//! understood before the job is created.

use async_trait::async_trait;
use chrono::Local;
use purrcode_api::cron::{CronAddInput, CronError, CronJobView, CronRegistry, ParsedScheduleView};
use purrcode_config::{Config, ModePresetLoader};
use purrcode_service::cron::parse_natural;
use purrcode_service::cron::store::{
    new_job_id, validate_tools, CronStore, PresetSnapshot, DEFAULT_ALLOWLIST,
};
use purrcode_service::cron::{CronJob, DeliveryTarget, JobSchedule};
use std::path::PathBuf;
use tokio::sync::Mutex;

pub struct WebCronRegistry {
    config: Config,
    workspace_root: PathBuf,
    purrcode_dir: PathBuf,
    /// Serializes web cron mutations (add/set_enabled/remove)
    /// so the check-then-act pairs in them — the duplicate-name
    /// 409 check, the existence pre-check — hold against
    /// concurrent requests. Parse is pure and lock-free.
    mutation_lock: Mutex<()>,
}

impl WebCronRegistry {
    pub fn new(config: Config, workspace_root: PathBuf, purrcode_dir: PathBuf) -> Self {
        Self {
            config,
            workspace_root,
            purrcode_dir,
            mutation_lock: Mutex::new(()),
        }
    }

    fn load_store(&self) -> Result<CronStore, CronError> {
        CronStore::load(&self.purrcode_dir).map_err(|e| match e {
            purrcode_api::error::PurrError::Io(_) => CronError::Internal(e.to_string()),
            _ => CronError::InvalidInput(e.to_string()),
        })
    }

    /// Pin the preset rails exactly like `cron add` (CLI): load
    /// the named preset (or the config default), refuse God
    /// guardrail / yolo approval, snapshot the rails.
    fn preset_snapshot(&self, preset_name: Option<&str>) -> Result<PresetSnapshot, CronError> {
        let pname = preset_name
            .map(str::to_string)
            .unwrap_or_else(|| self.config.preset.clone());
        let preset = ModePresetLoader::load_by_name(&pname, &self.workspace_root)
            .map_err(|e| CronError::InvalidInput(e.to_string()))?;
        if preset.guardrail == purrcode_config::GuardrailMode::God {
            return Err(CronError::InvalidInput(format!(
                "Cron job may not use a God-guardrail preset ('{pname}'): cron jobs never run oyen"
            )));
        }
        if preset.approval == purrcode_config::ApprovalMode::Yolo {
            return Err(CronError::InvalidInput(format!(
                "Cron job may not use a yolo-approval preset ('{pname}'): cron jobs never run yolo"
            )));
        }
        Ok(PresetSnapshot {
            name: preset.name.clone(),
            approval: format!("{:?}", preset.approval).to_lowercase(),
            guardrail: format!("{:?}", preset.guardrail).to_lowercase(),
            sandbox: format!("{:?}", preset.sandbox).to_lowercase(),
            // v0.3 F21: pin the OS-sandbox strictness at creation,
            // like the other rails.
            sandbox_mode: if preset.sandbox == purrcode_config::SandboxMode::Os {
                Some(preset.sandbox_mode.as_kebab_str().to_string())
            } else {
                None
            },
        })
    }

    /// The tool allowlist exactly like `cron add`: an explicit
    /// list wins; an explicitly empty list means nothing is
    /// allowed (fail-closed, never widened); omitted means
    /// Meowstro's read-only set.
    fn allowlist(&self, allow: Option<Vec<String>>) -> Result<Vec<String>, CronError> {
        let tools: Vec<String> = match allow {
            Some(list) => list
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            None => DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect(),
        };
        validate_tools(&tools).map_err(|e| CronError::InvalidInput(e.to_string()))?;
        Ok(tools)
    }
}

fn job_view(job: &CronJob) -> CronJobView {
    CronJobView {
        id: job.id.clone(),
        name: job.name.clone(),
        enabled: job.enabled,
        schedule_display: job.schedule.display.clone(),
        schedule_expr: job.schedule.expr.clone(),
        once: job.schedule.once,
        prompt: job.prompt.clone(),
        tools: job.tools.clone(),
        preset_name: job.preset.name.clone(),
        next_run: job.next_run.map(|d| d.to_rfc3339()),
        last_run: job.last_run.map(|d| d.to_rfc3339()),
        run_count: job.run_count,
        failure_count: job.failure_count,
        last_status: job.last_status.clone(),
    }
}

#[async_trait]
impl CronRegistry for WebCronRegistry {
    async fn parse_schedule(&self, schedule: &str) -> Result<ParsedScheduleView, CronError> {
        if schedule.trim().is_empty() {
            return Err(CronError::InvalidInput(
                "cron schedule must not be empty".to_string(),
            ));
        }
        let parsed = parse_natural(schedule, Local::now())
            .map_err(|e| CronError::InvalidInput(e.to_string()))?;
        Ok(ParsedScheduleView {
            expr: parsed.expr,
            display: parsed.display,
            once: parsed.once,
            next_run: parsed.next_run.to_rfc3339(),
        })
    }

    async fn add_job(&self, input: CronAddInput) -> Result<CronJobView, CronError> {
        let _guard = self.mutation_lock.lock().await;
        if input.prompt.trim().is_empty() {
            return Err(CronError::InvalidInput(
                "cron job prompt must not be empty".to_string(),
            ));
        }
        let now = Local::now();
        let parsed = parse_natural(&input.schedule, now)
            .map_err(|e| CronError::InvalidInput(e.to_string()))?;
        let snapshot = self.preset_snapshot(input.preset.as_deref())?;
        let tools = self.allowlist(input.allow)?;
        let job_name = match input.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => format!("job-{}", &new_job_id()[5..]),
        };
        let delivery = match self.config.telegram.as_ref() {
            Some(tg) if !tg.allow_from.is_empty() => DeliveryTarget::Telegram {
                chat_id: tg.allow_from[0],
            },
            _ => DeliveryTarget::QueueOnly,
        };
        let job = CronJob {
            id: new_job_id(),
            name: job_name,
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
            prompt: input.prompt,
            tools,
            preset: snapshot,
            delivery,
            created_at: now,
            last_run: None,
            next_run: Some(parsed.next_run),
            run_count: 0,
            failure_count: 0,
            last_status: None,
        };
        let mut store = self.load_store()?;
        if store.find(&job.name).is_some() {
            return Err(CronError::AlreadyExists(job.name.clone()));
        }
        store.add(job.clone()).map_err(|e| {
            // The lock makes this unreachable in practice (the
            // pre-check above just ran under it), but a CLI
            // process adding the same name concurrently could
            // still win the race — keep the 409 honest.
            let msg = e.to_string();
            if msg.contains("already exists") {
                CronError::AlreadyExists(job.name.clone())
            } else {
                match e {
                    purrcode_api::error::PurrError::Io(_) => CronError::Internal(msg),
                    _ => CronError::InvalidInput(msg),
                }
            }
        })?;
        Ok(job_view(&job))
    }

    async fn set_enabled(&self, needle: &str, enabled: bool) -> Result<CronJobView, CronError> {
        let _guard = self.mutation_lock.lock().await;
        let mut store = self.load_store()?;
        let job = store
            .find_mut(needle)
            .ok_or_else(|| CronError::NotFound(format!("no cron job matching '{needle}'")))?;
        job.enabled = enabled;
        let view = job_view(job);
        store.save().map_err(|e| match e {
            purrcode_api::error::PurrError::Io(_) => CronError::Internal(e.to_string()),
            _ => CronError::InvalidInput(e.to_string()),
        })?;
        Ok(view)
    }

    async fn remove_job(&self, needle: &str) -> Result<CronJobView, CronError> {
        let _guard = self.mutation_lock.lock().await;
        let mut store = self.load_store()?;
        if store.find(needle).is_none() {
            return Err(CronError::NotFound(format!(
                "no cron job matching '{needle}'"
            )));
        }
        let job = store.remove(needle).map_err(|e| match e {
            purrcode_api::error::PurrError::Io(_) => CronError::Internal(e.to_string()),
            _ => CronError::InvalidInput(e.to_string()),
        })?;
        Ok(job_view(&job))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "purrcode-web-cron-test-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn registry(root: &std::path::Path) -> WebCronRegistry {
        WebCronRegistry::new(
            Config::default(),
            root.to_path_buf(),
            root.join(".purrcode"),
        )
    }

    fn add_input(name: &str) -> CronAddInput {
        CronAddInput {
            name: Some(name.to_string()),
            schedule: "tiap pagi jam 8".to_string(),
            prompt: "ringkas email".to_string(),
            allow: None,
            preset: None,
        }
    }

    #[tokio::test]
    async fn test_parse_schedule_valid_and_invalid() {
        let root = tmp_root("parse");
        let reg = registry(&root);
        let parsed = reg.parse_schedule("tiap pagi jam 8").await.unwrap();
        assert!(!parsed.display.is_empty());
        assert!(!parsed.once);
        assert!(parsed.expr.is_some());
        assert!(reg.parse_schedule("").await.is_err());
        assert!(reg.parse_schedule("blablabla tidak jelas").await.is_err());
    }

    #[tokio::test]
    async fn test_add_job_defaults_and_duplicate() {
        let root = tmp_root("add");
        let reg = registry(&root);
        let created = reg.add_job(add_input("pagi-ringkas")).await.unwrap();
        assert_eq!(created.name, "pagi-ringkas");
        assert!(created.enabled);
        assert!(!created.next_run.as_ref().unwrap().is_empty());
        assert_eq!(created.preset_name, "tabby");
        // Omitted allowlist = Meowstro's read-only default.
        assert!(!created.tools.is_empty());
        assert!(!created.tools.iter().any(|t| t == "manage_skill"));

        // Duplicate name is a 409.
        assert!(matches!(
            reg.add_job(add_input("pagi-ringkas")).await,
            Err(CronError::AlreadyExists(_))
        ));

        // Auto name when omitted.
        let auto = reg
            .add_job(CronAddInput {
                name: None,
                ..add_input("unused")
            })
            .await
            .unwrap();
        assert!(auto.name.starts_with("job-"));
    }

    #[tokio::test]
    async fn test_add_job_rejects_bad_inputs() {
        let root = tmp_root("reject");
        let reg = registry(&root);
        // Empty prompt.
        assert!(matches!(
            reg.add_job(CronAddInput {
                prompt: "   ".to_string(),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // Unparsable schedule.
        assert!(matches!(
            reg.add_job(CronAddInput {
                schedule: "blablabla tidak jelas".to_string(),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // Forbidden tool (manage_skill is never allowed in cron).
        assert!(matches!(
            reg.add_job(CronAddInput {
                allow: Some(vec!["manage_skill".to_string()]),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // Unknown tool.
        assert!(matches!(
            reg.add_job(CronAddInput {
                allow: Some(vec!["not_a_tool".to_string()]),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // God-guardrail preset (oyen) is refused.
        assert!(matches!(
            reg.add_job(CronAddInput {
                preset: Some("oyen".to_string()),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // Unknown preset.
        assert!(matches!(
            reg.add_job(CronAddInput {
                preset: Some("tidak-ada".to_string()),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
        // Explicitly empty allowlist = fail-closed, allowed.
        let empty = reg
            .add_job(CronAddInput {
                allow: Some(vec![]),
                ..add_input("x-empty")
            })
            .await
            .unwrap();
        assert!(empty.tools.is_empty());
    }

    #[tokio::test]
    async fn test_add_job_rejects_yolo_preset_file() {
        let root = tmp_root("yolo");
        fs::create_dir_all(root.join("presets")).unwrap();
        fs::write(
            root.join("presets").join("yolo-test.jsonc"),
            "{\"$schema\": \"preset/v1\", \"name\": \"yolo-test\", \"approval\": \"yolo\", \"guardrail\": \"strict\", \"sandbox\": \"confinement\"}",
        )
        .unwrap();
        let reg = registry(&root);
        assert!(matches!(
            reg.add_job(CronAddInput {
                preset: Some("yolo-test".to_string()),
                ..add_input("x")
            })
            .await,
            Err(CronError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn test_set_enabled_and_remove() {
        let root = tmp_root("toggle");
        let reg = registry(&root);
        reg.add_job(add_input("tog")).await.unwrap();

        let disabled = reg.set_enabled("tog", false).await.unwrap();
        assert!(!disabled.enabled);
        let enabled = reg.set_enabled("tog", true).await.unwrap();
        assert!(enabled.enabled);
        // Lookup by id prefix works, like the CLI needle.
        let by_id = reg.set_enabled(&disabled.id[..10], false).await.unwrap();
        assert!(!by_id.enabled);

        assert!(matches!(
            reg.set_enabled("tidak-ada", true).await,
            Err(CronError::NotFound(_))
        ));

        let removed = reg.remove_job("tog").await.unwrap();
        assert_eq!(removed.name, "tog");
        assert!(matches!(
            reg.remove_job("tog").await,
            Err(CronError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_parallel_add_one_winner() {
        // Same class as B5 r1 P2-4: without the mutation lock,
        // two concurrent adds of one name both pass the 409
        // check and the second save loses the first job.
        let root = tmp_root("race");
        let reg = std::sync::Arc::new(registry(&root));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let reg = reg.clone();
            handles.push(tokio::spawn(
                async move { reg.add_job(add_input("race")).await },
            ));
        }
        let mut ok = 0;
        let mut conflict = 0;
        for h in handles {
            match h.await.unwrap() {
                Ok(_) => ok += 1,
                Err(CronError::AlreadyExists(_)) => conflict += 1,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
        assert_eq!(ok, 1);
        assert_eq!(conflict, 9);
        // Exactly one job on disk.
        let store = CronStore::load(&root.join(".purrcode")).unwrap();
        assert_eq!(store.jobs().len(), 1);
    }
}
