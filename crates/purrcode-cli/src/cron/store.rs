// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron job store (v0.3 F19, ADR-014).
//!
//! Job definitions live in a dedicated store, NOT in `purrcode.jsonc`:
//! `~/.purrcode/cron/jobs.jsonc` — a JSON list of job objects (written by
//! code as plain JSON, which is valid JSONC). Jobs are stateful (last/next
//! run, failure count); `purrcode.jsonc` is treated as config. Saves are
//! atomic (write tmp file, rename), like the Telegram `chats.json` store.

use chrono::{DateTime, Local};
use purrcode_api::error::{PurrError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The agent tool names a cron job may declare in its allowlist.
/// A job may NEVER manage cron: there is no cron tool in the agent toolset
/// at all, so self-management is impossible by construction — the allowlist
/// is validated against exactly these 13 names (v0.1 tool set + read_skill).
pub const AGENT_TOOL_NAMES: &[&str] = &[
    "read",
    "write",
    "edit",
    "bash",
    "grep",
    "glob",
    "find",
    "todo",
    "ask",
    "task",
    "webfetch",
    "websearch",
    "read_skill",
    "manage_skill",
];

/// Meowstro's read-only tool set + delegation (the default allowlist when
/// the user declares nothing at creation: no writes unless declared).
pub const DEFAULT_ALLOWLIST: &[&str] = &[
    "read",
    "grep",
    "glob",
    "find",
    "todo",
    "ask",
    "task",
    "read_skill",
];

/// Schedule as stored on a job: either a recurring cron expression or a
/// one-shot target time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSchedule {
    /// 5-field cron expression (local time). `None` for one-shot jobs.
    pub expr: Option<String>,
    /// Human-readable echo of what was understood at creation.
    pub display: String,
    /// True for one-shot (fires once), false for recurring.
    pub once: bool,
    /// One-shot target time. `None` for recurring jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Local>>,
}

/// The preset rails pinned at job creation (ADR-014 item 4). The runner
/// re-checks these at run time — a later preset edit never silently
/// changes a job's rails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetSnapshot {
    pub name: String,
    pub approval: String,
    pub guardrail: String,
    pub sandbox: String,
}

/// Where a job result is delivered. `Telegram` = send to that chat via the
/// gateway; `QueueOnly` = keep it in the delivery queue for the next TUI
/// open (no push).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeliveryTarget {
    Telegram { chat_id: i64 },
    QueueOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronJob {
    /// Stable id, `cron_<12 hex chars>`.
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: JobSchedule,
    /// The agent prompt run on every firing.
    pub prompt: String,
    /// Tool allowlist declared at creation. Empty = NOTHING allowed
    /// (fail-closed, never widened back to the default — Hermes rule).
    pub tools: Vec<String>,
    /// Preset rails pinned at creation.
    pub preset: PresetSnapshot,
    pub delivery: DeliveryTarget,
    pub created_at: DateTime<Local>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<Local>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<DateTime<Local>>,
    #[serde(default)]
    pub run_count: u32,
    #[serde(default)]
    pub failure_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobStoreFile {
    version: u32,
    jobs: Vec<CronJob>,
}

/// The job store: all jobs in one atomic JSONC file.
pub struct CronStore {
    path: PathBuf,
    jobs: Vec<CronJob>,
}

pub fn cron_dir(purrcode_dir: &Path) -> PathBuf {
    purrcode_dir.join("cron")
}

pub fn jobs_file_path(purrcode_dir: &Path) -> PathBuf {
    cron_dir(purrcode_dir).join("jobs.jsonc")
}

impl CronStore {
    pub fn load(purrcode_dir: &Path) -> Result<Self> {
        let path = jobs_file_path(purrcode_dir);
        if !path.exists() {
            return Ok(Self {
                path,
                jobs: Vec::new(),
            });
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            PurrError::Session(format!(
                "Failed to read cron jobs at {}: {e}",
                path.display()
            ))
        })?;
        // Jobs are written as plain JSON; tolerate JSONC comments on read.
        let cleaned = purrcode_config::jsonc::clean_jsonc(&raw);
        let file: JobStoreFile = serde_json::from_str(&cleaned).map_err(|e| {
            PurrError::Session(format!("Invalid cron jobs file {}: {e}", path.display()))
        })?;
        Ok(Self {
            path,
            jobs: file.jobs,
        })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                PurrError::Session(format!(
                    "Failed to create cron dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let file = JobStoreFile {
            version: 1,
            jobs: self.jobs.clone(),
        };
        let text = serde_json::to_string_pretty(&file)
            .map_err(|e| PurrError::Session(format!("Failed to serialize cron jobs: {e}")))?;
        let tmp = self.path.with_extension("jsonc.tmp");
        std::fs::write(&tmp, text).map_err(|e| {
            PurrError::Session(format!(
                "Failed to write cron jobs tmp {}: {e}",
                tmp.display()
            ))
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            PurrError::Session(format!(
                "Failed to rename cron jobs tmp to {}: {e}",
                self.path.display()
            ))
        })?;
        Ok(())
    }

    pub fn jobs(&self) -> &[CronJob] {
        &self.jobs
    }

    // Runner API (batch 3b); exercised only by the unit tests until then.
    #[allow(dead_code)]
    pub fn jobs_mut(&mut self) -> &mut Vec<CronJob> {
        &mut self.jobs
    }

    /// Find a job by full id, id prefix, or exact name.
    #[allow(dead_code)] // runner API (batch 3b); unit tests until then
    pub fn find(&self, needle: &str) -> Option<&CronJob> {
        self.jobs
            .iter()
            .find(|j| j.id == needle || j.id.starts_with(needle) || j.name == needle)
    }

    pub fn find_mut(&mut self, needle: &str) -> Option<&mut CronJob> {
        self.jobs
            .iter_mut()
            .find(|j| j.id == needle || j.id.starts_with(needle) || j.name == needle)
    }

    pub fn add(&mut self, job: CronJob) -> Result<()> {
        validate_job(&job)?;
        if self.jobs.iter().any(|j| j.id == job.id) {
            return Err(PurrError::Session(format!(
                "Cron job id '{}' already exists",
                job.id
            )));
        }
        if self.jobs.iter().any(|j| j.name == job.name) {
            return Err(PurrError::Session(format!(
                "Cron job name '{}' already exists",
                job.name
            )));
        }
        self.jobs.push(job);
        self.save()
    }

    pub fn remove(&mut self, needle: &str) -> Result<CronJob> {
        let pos = self
            .jobs
            .iter()
            .position(|j| j.id == needle || j.id.starts_with(needle) || j.name == needle)
            .ok_or_else(|| PurrError::Session(format!("No cron job matching '{needle}'")))?;
        let job = self.jobs.remove(pos);
        self.save()?;
        Ok(job)
    }
}

pub fn new_job_id() -> String {
    format!(
        "cron_{}",
        &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]
    )
}

pub fn validate_job(job: &CronJob) -> Result<()> {
    if job.name.trim().is_empty() {
        return Err(PurrError::Session(
            "Cron job name must not be empty".to_string(),
        ));
    }
    if job.name.chars().count() > 80 {
        return Err(PurrError::Session(
            "Cron job name must be at most 80 characters".to_string(),
        ));
    }
    if job.prompt.trim().is_empty() {
        return Err(PurrError::Session(
            "Cron job prompt must not be empty".to_string(),
        ));
    }
    validate_tools(&job.tools)?;
    // The pinned preset must never be God/yolo (ADR-014 item 4) — checked
    // again by the runner at run time (defense in depth).
    if job.preset.guardrail == "god" {
        return Err(PurrError::Session(format!(
            "Cron job '{}' may not use a God-guardrail preset ({}): cron jobs never run oyen",
            job.name, job.preset.name
        )));
    }
    if job.preset.approval == "yolo" {
        return Err(PurrError::Session(format!(
            "Cron job '{}' may not use a yolo-approval preset ({}): cron jobs never run yolo",
            job.name, job.preset.name
        )));
    }
    Ok(())
}

pub fn validate_tools(tools: &[String]) -> Result<()> {
    for tool in tools {
        if !AGENT_TOOL_NAMES.contains(&tool.as_str()) {
            return Err(PurrError::Session(format!(
                "Unknown tool '{tool}' in cron allowlist (known tools: {})",
                AGENT_TOOL_NAMES.join(", ")
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::schedule::{parse_natural, CronExpr};
    use chrono::TimeZone;
    use std::collections::HashSet;

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 12, 7, 0, 0).unwrap()
    }

    fn sample_job() -> CronJob {
        let parsed = parse_natural("tiap pagi jam 8", fixed_now()).expect("parses");
        CronJob {
            id: new_job_id(),
            name: "pagi-ringkas".to_string(),
            enabled: true,
            schedule: JobSchedule {
                expr: parsed.expr,
                display: parsed.display,
                once: false,
                at: None,
            },
            prompt: "ringkas email".to_string(),
            tools: DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect(),
            preset: PresetSnapshot {
                name: "tabby".to_string(),
                approval: "supervised".to_string(),
                guardrail: "default".to_string(),
                sandbox: "confinement".to_string(),
            },
            delivery: DeliveryTarget::QueueOnly,
            created_at: fixed_now(),
            last_run: None,
            next_run: Some(parsed.next_run),
            run_count: 0,
            failure_count: 0,
        }
    }

    #[test]
    fn test_job_roundtrip_atomic_save() {
        let dir = std::env::temp_dir().join(format!(
            "purr-cron-store-test-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = CronStore::load(&dir).expect("empty store loads");
        assert!(store.jobs().is_empty());
        let job = sample_job();
        let id = job.id.clone();
        store.add(job).expect("add works");
        assert!(jobs_file_path(&dir).exists());
        // No leftover tmp file after an atomic save.
        assert!(!dir.join("cron").join("jobs.jsonc.tmp").exists());

        let store2 = CronStore::load(&dir).expect("reload works");
        assert_eq!(store2.jobs().len(), 1);
        let found = store2.find(&id).expect("find by id");
        assert_eq!(found.name, "pagi-ringkas");
        assert_eq!(found.schedule.display, "every day at 08:00");
        // CronExpr re-parses from the stored expression.
        let expr = CronExpr::parse(found.schedule.expr.as_deref().unwrap()).expect("parses");
        assert_eq!(expr.hours, vec![8]);

        // Duplicate name is rejected; duplicate id is rejected.
        let mut dup = sample_job();
        dup.id = new_job_id();
        assert!(store.add(dup).is_err());
        let mut dup_id = sample_job();
        dup_id.id = id.clone();
        dup_id.name = "other".to_string();
        assert!(store.add(dup_id).is_err());

        // Remove by name prefix of the id.
        let removed = store.remove(&id[..8]).expect("remove by id prefix");
        assert_eq!(removed.id, id);
        assert!(store.jobs().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_job_rejects_god_yolo_and_unknown_tools() {
        let mut job = sample_job();
        job.preset.guardrail = "god".to_string();
        assert!(validate_job(&job).is_err());
        let mut job = sample_job();
        job.preset.approval = "yolo".to_string();
        assert!(validate_job(&job).is_err());
        let mut job = sample_job();
        job.tools = vec!["cron_add".to_string()];
        assert!(validate_job(&job).is_err());
        let mut job = sample_job();
        job.tools = vec![];
        assert!(validate_job(&job).is_ok()); // explicit [] = nothing allowed
        let mut job = sample_job();
        job.prompt = "   ".to_string();
        assert!(validate_job(&job).is_err());
    }

    #[test]
    fn test_default_allowlist_has_no_writes() {
        let set: HashSet<&str> = DEFAULT_ALLOWLIST.iter().copied().collect();
        assert!(!set.contains("write"));
        assert!(!set.contains("edit"));
        assert!(!set.contains("bash"));
        assert!(!set.contains("manage_skill"));
        assert!(set.contains("task"));
    }
}
