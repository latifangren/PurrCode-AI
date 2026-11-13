// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron registry trait for the WebUI (v0.4 B6), mirroring
//! `skill.rs` (B5) and `plugin.rs` (B4). The channel crate only
//! ever sees this trait, so the crate chain stays one-way
//! (`channel → api ← service`): the real implementation lives in
//! `purrcode-cli` and reuses the CLI's `cron add` path — the same
//! NL schedule parser, the same preset-rail pinning, the same
//! allowlist validation, the same store — so the WebUI never
//! re-implements scheduling logic and never writes `jobs.jsonc`
//! itself. The HTTP handlers only translate JSON ↔ trait calls.
//!
//! A job is identified by id or exact name, like the CLI's
//! needle matching (`store.find`): full id, id prefix, or name.
//! `add` refuses a duplicate name (409); `set_enabled`/`remove`
//! require the job to exist (404 otherwise). The preset rails
//! are pinned at creation exactly like `cron add`: God-guardrail
//! and yolo-approval presets are refused, and the allowlist
//! defaults to Meowstro's read-only set when omitted — an
//! explicitly empty list means nothing is allowed (fail-closed).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// One cron job as the WebUI lists it. `next_run` is RFC 3339
/// (local time); `None` when the store has not computed one yet
/// (a hand-edited job, or a one-shot whose target already passed
/// and was never fired).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule_display: String,
    pub schedule_expr: Option<String>,
    pub once: bool,
    pub prompt: String,
    pub tools: Vec<String>,
    pub preset_name: String,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub run_count: u32,
    pub failure_count: u32,
    pub last_status: Option<String>,
}

/// The parsed form of a natural-language schedule: what the
/// parser understood, and when the job would next fire. Returned
/// by the parse-preview endpoint so the UI can show the user
/// exactly what was understood before the job is created — the
/// web equivalent of the CLI's interactive confirm step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedScheduleView {
    pub expr: Option<String>,
    pub display: String,
    pub once: bool,
    pub next_run: String,
}

/// Input for creating a job. `name` is optional (auto-generated
/// `job-<suffix>` like the CLI when omitted); `allow` is
/// optional too — `None` means the Meowstro read-only default,
/// `Some(vec![])` means an explicitly empty allowlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronAddInput {
    pub name: Option<String>,
    pub schedule: String,
    pub prompt: String,
    pub allow: Option<Vec<String>>,
    pub preset: Option<String>,
}

/// Why a cron operation was refused. The WebUI maps these to
/// HTTP statuses: `NotFound` → 404, `AlreadyExists` → 409,
/// `InvalidInput` → 400, `Internal` → 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronError {
    NotFound(String),
    AlreadyExists(String),
    InvalidInput(String),
    Internal(String),
}

impl std::fmt::Display for CronError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "no cron job matching '{name}'"),
            Self::AlreadyExists(name) => write!(f, "cron job '{name}' already exists"),
            Self::InvalidInput(msg) => write!(f, "invalid cron input: {msg}"),
            Self::Internal(msg) => write!(f, "cron operation failed: {msg}"),
        }
    }
}

impl std::error::Error for CronError {}

/// The live cron registry the WebUI manages (v0.4 B6).
/// Implemented in `purrcode-cli` on top of the service cron
/// store; the channel crate only ever sees this trait.
#[async_trait]
pub trait CronRegistry: Send + Sync {
    /// Parse a natural-language schedule with the same parser
    /// `cron add` uses. No side effects — the UI calls this for
    /// the preview before creating the job.
    async fn parse_schedule(&self, schedule: &str) -> Result<ParsedScheduleView, CronError>;
    /// Create a job: parse the schedule, pin the preset rails,
    /// validate the allowlist, save to the store. Refuses a
    /// duplicate name (409) and an unparsable schedule, a
    /// God/yolo preset, or a forbidden tool (400).
    async fn add_job(&self, input: CronAddInput) -> Result<CronJobView, CronError>;
    /// Flip a job's `enabled` flag and save. The job must exist.
    async fn set_enabled(&self, needle: &str, enabled: bool) -> Result<CronJobView, CronError>;
    /// Remove a job from the store. The job must exist.
    async fn remove_job(&self, needle: &str) -> Result<CronJobView, CronError>;
}
