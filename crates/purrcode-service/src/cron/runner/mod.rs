// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron job runner (v0.3 F19, ADR-014, batch 3b).
//!
//! One firing = one Meowstro turn in a special session
//! `purr_cron_<jobid>_<run>`, under the preset rails pinned at creation
//! and the job's declared tool allowlist. Nothing here prompts: the
//! approval requester approves only tools the job declared (the user
//! pre-approved exactly those at creation), and `ask` never gets an
//! answer in a cron run. Tuxeshadow is mandatory (`is_cron: true` in the
//! tool context, forced — never trusted from the model's arguments), and
//! a model-supplied shadow override reason is ignored in a cron run.
//!
//! The gateway tick (`tick_once`) runs due jobs sequentially, marks
//! missed firings, then drains pending Telegram deliveries. It never
//! fails: every error is logged and the tick continues, so a broken job
//! or a dead Telegram can never kill the gateway's poll loop.

mod model;

pub use model::resolve_target_model;

use crate::cron::queue::{new_delivery_id, Delivery, DeliveryKind, DeliveryQueue};
use crate::cron::schedule::CronExpr;
use crate::cron::store::{validate_job, CronJob, CronStore, DeliveryTarget, PresetSnapshot};
use async_trait::async_trait;
use chrono::{DateTime, Local};
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::provider::MessageRole;
use purrcode_api::tool::{AuditSink, SandboxStrictness, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::telegram::TelegramChannel;
use purrcode_config::{
    ApprovalMode, Config, GuardrailMode, ModePreset, ModelPreset, ModelPresetLoader, SandboxMode,
};
use purrcode_core::{ApprovalRequester, SessionStore};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_tools::{AskOption, AskRequester};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How late a recurring firing may be and still run: twice the tick
/// interval, at least two minutes. A daemon ticking normally fires
/// within one interval of the schedule; anything older missed its tick
/// (the daemon was down) and is skipped, never fired late
/// (ZeroClaw `MissedTickBehavior::Skip`).
fn recurring_grace(tick_secs: u64) -> chrono::Duration {
    chrono::Duration::seconds((2 * tick_secs).max(120) as i64)
}

/// One-shot jobs get a fixed five-minute grace: the daemon may have
/// been starting up when the target time passed.
const ONE_SHOT_GRACE_SECS: i64 = 300;

/// A firing that started but whose daemon died leaves last_status =
/// "running". Older than this, the run is dead and the job may be
/// classified normally again.
const STALE_RUNNING_SECS: i64 = 3600;

/// Cap on the stored delivery detail: the full text lives in the session
/// transcript; the queue only needs enough to notify.
const MAX_DETAIL_CHARS: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueDecision {
    Due,
    NotDue,
    Missed(String),
}

/// Whether a job's "running" marker is fresh enough to block a new run.
/// A marker with no timestamp is malformed (only reachable by
/// hand-editing the store) and counts as stale: the next run overwrites
/// both fields, so this self-heals instead of deadlocking the job.
fn is_lease_fresh(job: &CronJob, now: DateTime<Local>) -> bool {
    job.last_status.as_deref() == Some("running")
        && job
            .last_run
            .is_some_and(|last| now.signed_duration_since(last).num_seconds() < STALE_RUNNING_SECS)
}

/// Pure due/missed classification for one job at one moment (unit-tested;
/// the tick only acts on the outcome).
pub fn classify_due(job: &CronJob, now: DateTime<Local>, tick_secs: u64) -> DueDecision {
    if !job.enabled {
        return DueDecision::NotDue;
    }
    let Some(target) = job.next_run else {
        return DueDecision::NotDue;
    };
    // A "running" marker from a run that started recently means the
    // daemon (or a restart of it) may still be working this job: do not
    // fire it twice. After an hour the run is dead and the job is
    // classified normally again.
    if is_lease_fresh(job, now) {
        return DueDecision::NotDue;
    }
    if target > now {
        return DueDecision::NotDue;
    }
    let late = now.signed_duration_since(target).num_seconds();
    let grace = if job.schedule.once {
        ONE_SHOT_GRACE_SECS
    } else {
        recurring_grace(tick_secs).num_seconds()
    };
    if late <= grace {
        DueDecision::Due
    } else {
        DueDecision::Missed(format!(
            "target {} was {late}s ago, past the {grace}s grace",
            target.format("%Y-%m-%d %H:%M")
        ))
    }
}

/// Approval requester for cron runs (ADR-014 item 4): a gate `Ask` for a
/// tool the job declared is treated as approved — the user pre-approved
/// exactly that tool at creation. Anything else is denied. (The gate's
/// hard allowlist denies undeclared tools before this is even consulted;
/// this is defense in depth — and it is also what the Strict-mode shadow
/// path consults for "tuxeshadow", which is never in the allowlist, so a
/// Carrier shadow objection stays blocked.)
pub struct CronApprovalRequester {
    tools: HashSet<String>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    session_id: String,
}

impl CronApprovalRequester {
    /// Build the deny-always requester for an unattended daemon run
    /// (cron job or heartbeat item): a tool in `tools` is approved,
    /// anything else is denied. Shared by the cron runner and the
    /// heartbeat runner (v0.3 F22) — the rails are identical.
    pub fn new(
        tools: HashSet<String>,
        audit_sink: Option<Arc<dyn AuditSink>>,
        session_id: String,
    ) -> Self {
        Self {
            tools,
            audit_sink,
            session_id,
        }
    }
}

#[async_trait]
impl ApprovalRequester for CronApprovalRequester {
    async fn request_approval(
        &self,
        _call_id: &str,
        tool: &str,
        _description: &str,
        _preview: Option<&str>,
    ) -> bool {
        let ok = self.tools.contains(tool);
        if let Some(sink) = &self.audit_sink {
            sink.record_event(
                &self.session_id,
                "meowstro",
                "cron_approval",
                Some(tool),
                if ok {
                    "approved_in_allowlist"
                } else {
                    "denied_outside_allowlist"
                },
                if ok {
                    "tool is in the job allowlist declared at creation"
                } else {
                    "tool is NOT in the job allowlist declared at creation"
                },
            );
        }
        ok
    }
}

/// Ask requester for cron runs: there is no user to ask. Always declines
/// (returns `None`), and `AskTool` turns that into a tool error — never
/// a stdin prompt, never a fabricated answer.
pub struct CronAskRequester;

#[async_trait]
impl AskRequester for CronAskRequester {
    async fn request_decision(&self, _question: &str, _options: &[AskOption]) -> Option<String> {
        None
    }
}

fn parse_preset_enums(
    snapshot: &PresetSnapshot,
) -> Result<(ApprovalMode, GuardrailMode, SandboxMode)> {
    fn parse_enum<T: for<'de> Deserialize<'de>>(value: &str, what: &str) -> Result<T> {
        serde_json::from_str(&format!("\"{value}\"")).map_err(|_| {
            PurrError::Config(format!(
                "Cron job preset has an unknown {what} value '{value}'"
            ))
        })
    }
    Ok((
        parse_enum(&snapshot.approval, "approval")?,
        parse_enum(&snapshot.guardrail, "guardrail")?,
        parse_enum(&snapshot.sandbox, "sandbox")?,
    ))
}

/// Parse the OS-sandbox strictness pinned in the job snapshot.
/// Fail-closed: a job whose snapshot claims `sandbox: "os"` must
/// carry a known mode; a missing or unknown mode fails the run
/// rather than running the job unsandboxed. For any other sandbox
/// the returned value is the `ReadOnly` placeholder — the caller
/// only reads it when `sandbox == Os`.
fn sandbox_mode_from_snapshot(snapshot: &PresetSnapshot) -> Result<SandboxStrictness> {
    if snapshot.sandbox != "os" {
        return Ok(SandboxStrictness::ReadOnly);
    }
    let mode = snapshot.sandbox_mode.as_deref().ok_or_else(|| {
        PurrError::Config(
            "Cron job preset has sandbox 'os' but no sandbox_mode; refusing to run it unsandboxed"
                .to_string(),
        )
    })?;
    SandboxStrictness::from_kebab(mode).ok_or_else(|| {
        PurrError::Config(format!(
            "Cron job preset has an unknown sandbox_mode value '{mode}'"
        ))
    })
}

/// First non-empty line, capped at 200 chars, for the delivery summary.
/// Shared by the cron runner and the heartbeat runner (v0.3 F22).
pub(crate) fn summarize(text: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let short: String = first.chars().take(200).collect();
    if first.chars().count() > 200 {
        format!("{short}…")
    } else {
        short
    }
}

/// Cap the delivery detail at 32 KiB (a runaway turn must not bloat
/// the queue file). Shared by the cron runner and the heartbeat
/// runner (v0.3 F22).
pub(crate) fn cap_detail(text: &str) -> String {
    if text.chars().count() > MAX_DETAIL_CHARS {
        let kept: String = text.chars().take(MAX_DETAIL_CHARS).collect();
        format!("{kept}\n… (truncated, full text in the session transcript)")
    } else {
        text.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct CronRunOutcome {
    pub job_id: String,
    pub job_name: String,
    pub session_id: String,
    pub ok: bool,
    pub summary: String,
    pub detail: String,
}

/// Everything one cron firing needs that outlives the firing itself.
/// Shared by the heartbeat runner (v0.3 F22), which needs the same
/// daemon-scoped dependencies. Clone only (no Debug: the registry,
/// audit logger and plugin dispatcher do not implement it).
#[derive(Clone)]
pub struct CronRunnerDeps {
    pub config: Config,
    pub workspace_root: PathBuf,
    pub purrcode_dir: PathBuf,
    pub registry: Arc<ProviderRegistry>,
    pub provider_name: String,
    pub wire_model: String,
    pub audit_logger: Arc<AuditLogger>,
    pub plugin_dispatcher: Arc<dyn PluginHookDispatcher>,
    pub unenforced_caps: Vec<(String, Vec<&'static str>)>,
    pub model_preset: Option<ModelPreset>,
    pub system_prompt: String,
}

pub struct CronRunner {
    deps: CronRunnerDeps,
}

impl CronRunner {
    pub fn new(deps: CronRunnerDeps) -> Self {
        Self { deps }
    }

    /// Fire one job now (used by the gateway tick and `cron run`).
    /// Re-validates the job's rails, runs the turn, enqueues the
    /// delivery, advances the schedule, and saves the store. Never
    /// prompts. A failure to even start (bad rails, unknown provider)
    /// is returned as an error and the caller records it; a turn that
    /// ran and failed is an `Ok` outcome with `ok: false`. Refuses a
    /// disabled job and a job already in flight (the running lease) —
    /// due-ness itself is NOT checked here: a manual fire is allowed
    /// before the scheduled time; the tick checks due-ness via
    /// `classify_due` before calling this.
    pub async fn run_job(&self, store: &mut CronStore, needle: &str) -> Result<CronRunOutcome> {
        let job = store
            .find(needle)
            .cloned()
            .ok_or_else(|| PurrError::Session(format!("No cron job matching '{needle}'")))?;
        // Re-validate the pinned rails: a later preset edit (or a
        // hand-edited jobs file) never silently changes a job's rails.
        validate_job(&job)?;
        // Scheduling rails for the manual path (re-check F1): the tick
        // never reaches this point for these jobs because `classify_due`
        // filters them — this guard is the manual path's share of the
        // same rails.
        if !job.enabled {
            return Err(PurrError::Session(format!(
                "Cron job '{}' is disabled; enable it first (`purrcode-ai cron enable {}`)",
                job.name, job.name
            )));
        }
        if is_lease_fresh(&job, Local::now()) {
            return Err(PurrError::Session(format!(
                "Cron job '{}' is already running (started {}); refusing to fire it twice",
                job.name,
                job.last_run
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "at an unknown time".to_string())
            )));
        }
        let (approval, guardrail, sandbox) = parse_preset_enums(&job.preset)?;
        let sandbox_mode = sandbox_mode_from_snapshot(&job.preset)?;
        let preset = ModePreset {
            schema: "preset/v1".to_string(),
            name: job.preset.name.clone(),
            description: String::new(),
            approval,
            guardrail,
            sandbox,
            sandbox_mode,
            tools: job.tools.clone(),
            model: None,
        };
        let allow_set: HashSet<String> = job.tools.iter().cloned().collect();
        let gate = Arc::new(
            ApprovalGate::from_preset(&preset, false).with_tool_allowlist(allow_set.clone()),
        );

        let run_hex = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
        let session_id = format!(
            "purr_cron_{}_{}",
            job.id.trim_start_matches("cron_"),
            run_hex
        );
        let audit_sink: Arc<dyn AuditSink> = self.deps.audit_logger.clone();
        let now = Local::now();

        // Mark running BEFORE the turn (lease): a daemon that dies
        // mid-run leaves a "running" marker so a restart does not fire
        // the same job twice (see `classify_due`).
        if let Some(stored) = store.find_mut(&job.id) {
            stored.last_run = Some(now);
            stored.run_count += 1;
            stored.last_status = Some("running".to_string());
            store.save()?;
        }
        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            agent: "meowstro".to_string(),
            event: "cron_run_start".to_string(),
            tool: None,
            decision: "allow".to_string(),
            detail: format!(
                "job={} ({}) preset={} tools=[{}]",
                job.name,
                job.id,
                job.preset.name,
                job.tools.join(",")
            ),
        });

        let provider = self
            .deps
            .registry
            .get(&self.deps.provider_name)
            .ok_or_else(|| {
                PurrError::Config(format!(
                    "Provider '{}' has no base_url or client configured.",
                    self.deps.provider_name
                ))
            })?;
        let approval_req: Arc<dyn ApprovalRequester> = Arc::new(CronApprovalRequester {
            tools: allow_set,
            audit_sink: Some(audit_sink.clone()),
            session_id: session_id.clone(),
        });
        let ask_req: Arc<dyn AskRequester> = Arc::new(CronAskRequester);
        let session_dir = SessionStore::new(&self.deps.purrcode_dir).session_dir(&session_id);

        let meowstro = MeowstroOrchestrator::with_complete_options(
            provider,
            self.deps.wire_model.clone(),
            &session_dir,
            Some(approval_req),
            Some(ask_req),
            Some(gate),
            Some(audit_sink.clone()),
            Some(self.deps.plugin_dispatcher.clone()),
            self.deps.model_preset.clone(),
            Some(self.deps.config.clone()),
            Some(self.deps.registry.clone()),
            None,
            None,
            false,
            Some(self.deps.workspace_root.clone()),
            None,
        );
        let ctx = ToolContext {
            session_id: session_id.clone(),
            agent_role: "meowstro".to_string(),
            workspace_root: self.deps.workspace_root.clone(),
            is_dry_run: false,
            is_cron: true,
            // v0.3 F21: the job's pinned snapshot carries its sandbox
            // (rebuilt into `preset` above from `job.preset`).
            sandbox: if preset.sandbox == SandboxMode::Os {
                Some(preset.sandbox_mode)
            } else {
                None
            },
        };

        self.deps
            .plugin_dispatcher
            .dispatch_session_start(&session_id)
            .await;
        crate::log_unenforced_plugin_capabilities(
            &self.deps.audit_logger,
            &session_id,
            "meowstro",
            &self.deps.unenforced_caps,
        );

        let session_store = SessionStore::new(&self.deps.purrcode_dir);
        let mut messages = Vec::new();
        let turn = meowstro
            .agent_loop()
            .run_turn_with_history(
                &mut messages,
                &job.prompt,
                Some(&self.deps.system_prompt),
                &ctx,
                None,
            )
            .await;
        // Persist the transcript even when the turn failed (like the
        // gateway): otherwise a failed run leaves no record at all.
        for msg in &messages {
            if let Err(e) = session_store.append_message(&session_id, msg) {
                tracing::warn!("cron: failed to persist message for {session_id}: {e}");
            }
        }
        self.deps
            .plugin_dispatcher
            .dispatch_session_stop(&session_id)
            .await;

        let (ok, raw_text) = match &turn {
            Ok(text) => {
                // The transcript is ground truth; the returned string is
                // the fallback when the transcript holds no final answer.
                let from_messages = messages
                    .iter()
                    .rev()
                    .find(|m| m.role == MessageRole::Assistant)
                    .map(|m| m.content.clone());
                (
                    true,
                    from_messages
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| text.clone()),
                )
            }
            Err(e) => (false, format!("{e}")),
        };
        let now = Local::now();
        let (summary, detail, kind) = if ok {
            (
                summarize(&raw_text),
                cap_detail(&raw_text),
                DeliveryKind::Result,
            )
        } else {
            (
                format!("Cron job '{}' failed: {}", job.name, summarize(&raw_text)),
                cap_detail(&raw_text),
                DeliveryKind::Failure,
            )
        };

        // Enqueue the delivery (best-effort: a queue failure must not fail
        // a run that already happened — the transcript still holds it).
        let chat_id = match &job.delivery {
            DeliveryTarget::Telegram { chat_id } => Some(*chat_id),
            DeliveryTarget::QueueOnly => None,
        };
        let queue = DeliveryQueue::new(&self.deps.purrcode_dir);
        let delivery = Delivery {
            id: new_delivery_id(),
            job_id: job.id.clone(),
            job_name: job.name.clone(),
            created_at: now,
            kind,
            summary: summary.clone(),
            detail: detail.clone(),
            chat_id,
        };
        if let Err(e) = queue.create(&delivery) {
            tracing::warn!("cron: failed to enqueue delivery for {}: {e}", job.name);
        }

        // Advance the schedule and record the outcome, then save.
        let mut last_status = if ok {
            "ok".to_string()
        } else {
            format!("failed: {}", summarize(&raw_text))
        };
        let mut enabled = job.enabled;
        let mut next_run = job.next_run;
        if job.schedule.once {
            enabled = false;
            next_run = None;
        } else if let Some(expr_str) = &job.schedule.expr {
            match CronExpr::parse(expr_str)
                .ok()
                .and_then(|e| e.next_after(now))
            {
                Some(next) => next_run = Some(next),
                None => {
                    // Stored schedule no longer parses: disable rather
                    // than re-firing every tick.
                    enabled = false;
                    next_run = None;
                    last_status = "failed: stored schedule no longer parses".to_string();
                }
            }
        }
        if let Some(stored) = store.find_mut(&job.id) {
            stored.last_run = Some(now);
            if !ok {
                stored.failure_count += 1;
            }
            stored.last_status = Some(last_status);
            stored.enabled = enabled;
            stored.next_run = next_run;
        }
        store.save()?;

        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            agent: "meowstro".to_string(),
            event: if ok { "cron_run_ok" } else { "cron_run_failed" }.to_string(),
            tool: None,
            decision: if ok { "allow" } else { "deny" }.to_string(),
            detail: format!("job={} ({}) delivery={}", job.name, job.id, delivery.id),
        });

        Ok(CronRunOutcome {
            job_id: job.id,
            job_name: job.name,
            session_id,
            ok,
            summary,
            detail,
        })
    }

    /// One scheduler tick: run due jobs (sequentially, in store order),
    /// mark missed firings, then drain pending Telegram deliveries.
    /// Never fails: every error is logged and the tick continues, so a
    /// broken job or a dead Telegram can never kill the gateway's poll
    /// loop (its own error boundary, ADR-014 item 5).
    pub async fn tick_once(&self, channel: &TelegramChannel, tick_secs: u64) {
        let now = Local::now();
        let mut store = match CronStore::load(&self.deps.purrcode_dir) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("cron tick: cannot load job store: {e}");
                return;
            }
        };
        let plan: Vec<(String, DueDecision)> = store
            .jobs()
            .iter()
            .map(|j| (j.id.clone(), classify_due(j, now, tick_secs)))
            .collect();
        for (id, decision) in plan {
            match decision {
                DueDecision::Due => {
                    if let Err(e) = self.run_job(&mut store, &id).await {
                        tracing::warn!("cron tick: job {id} failed to start: {e}");
                        if let Some(job) = store.find_mut(&id) {
                            job.failure_count += 1;
                            job.last_status = Some(format!("failed: {e}"));
                            let _ = store.save();
                        }
                    }
                }
                DueDecision::Missed(reason) => {
                    self.mark_missed(&mut store, &id, &reason, now).await;
                }
                DueDecision::NotDue => {}
            }
        }
        self.drain_deliveries(channel).await;
    }

    /// Record a missed firing: advance a recurring job to its next
    /// occurrence after now (never fire late), or disable a one-shot.
    /// The user is told via a Failure delivery, not silently skipped.
    async fn mark_missed(
        &self,
        store: &mut CronStore,
        job_id: &str,
        reason: &str,
        now: DateTime<Local>,
    ) {
        let Some(job) = store.find_mut(job_id) else {
            return;
        };
        let name = job.name.clone();
        let delivery = job.delivery.clone();
        let once = job.schedule.once;
        let expr_str = job.schedule.expr.clone();
        if once {
            job.enabled = false;
            job.next_run = None;
        } else if let Some(expr_str) = expr_str {
            match CronExpr::parse(&expr_str)
                .ok()
                .and_then(|e| e.next_after(now))
            {
                Some(next) => job.next_run = Some(next),
                None => {
                    job.enabled = false;
                    job.next_run = None;
                }
            }
        }
        job.last_status = Some(format!("missed: {reason}"));
        let next_display = job
            .next_run
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "none (job disabled)".to_string());
        let queue = DeliveryQueue::new(&self.deps.purrcode_dir);
        let chat_id = match &delivery {
            DeliveryTarget::Telegram { chat_id } => Some(*chat_id),
            DeliveryTarget::QueueOnly => None,
        };
        let summary = format!("Cron job '{name}' missed its scheduled run: {reason}");
        let _ = queue.create(&Delivery {
            id: new_delivery_id(),
            job_id: job_id.to_string(),
            job_name: name.clone(),
            created_at: now,
            kind: DeliveryKind::Failure,
            summary: summary.clone(),
            detail: format!("{summary}\nNext run: {next_display}"),
            chat_id,
        });
        let _ = store.save();
        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: format!("purr_cron_{job_id}"),
            agent: "meowstro".to_string(),
            event: "cron_missed".to_string(),
            tool: None,
            decision: "deny".to_string(),
            detail: summary,
        });
    }

    /// Send pending Telegram deliveries (claim-first, Hermes rule).
    /// Queue-only deliveries stay pending for the next TUI open.
    /// A send that fails halfway is still marked delivered — losing
    /// beats duplicating a possibly-completed send. Shared by the
    /// heartbeat tick (v0.3 F22), which enqueues its own deliveries
    /// into the same queue.
    pub async fn drain_deliveries(&self, channel: &TelegramChannel) {
        let queue = DeliveryQueue::new(&self.deps.purrcode_dir);
        let pending = match queue.pending() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("cron drain: cannot read delivery queue: {e}");
                return;
            }
        };
        for delivery in pending {
            let Some(chat_id) = delivery.chat_id else {
                continue;
            };
            if let Err(e) = queue.claim(&delivery.id) {
                tracing::warn!("cron drain: cannot claim {}: {e}", delivery.id);
                continue;
            }
            let text = format!(
                "🐾 Cron: {}\n{}\n\n{}",
                delivery.job_name, delivery.summary, delivery.detail
            );
            let sent = channel
                .renderer(chat_id)
                .send_standalone_reported(&text)
                .await;
            if !sent {
                // At-most-once is preserved (still marked delivered), but
                // the loss is now visible in the audit log instead of
                // vanishing silently (build F2).
                let _ = self.deps.audit_logger.log_entry(AuditEntry {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    session_id: format!("purr_cron_{}", delivery.job_id),
                    agent: "meowstro".to_string(),
                    event: "cron_delivery_failed".to_string(),
                    tool: None,
                    decision: "deny".to_string(),
                    detail: format!(
                        "delivery {} to chat {} was not sent (Bot API error); marked delivered anyway — at-most-once: losing beats duplicating a possibly-completed send",
                        delivery.id, chat_id
                    ),
                });
            }
            if let Err(e) = queue.mark_delivered(&delivery.id) {
                tracing::warn!("cron drain: cannot mark {} delivered: {e}", delivery.id);
            }
        }
    }
}

/// Build runner deps from a loaded config (the `cron run` path mirrors
/// `run_one_shot`'s setup: same provider resolution, plugins, skills,
/// memory and system prompt).
pub async fn build_runner_deps(
    config: &Config,
    workspace_root: &Path,
    model_override: Option<String>,
) -> Result<CronRunnerDeps> {
    let purrcode_dir = crate::purrcode_dir();
    let target_model = resolve_target_model(config, model_override)?;
    let registry = ProviderRegistry::from_config(config)?;
    let (provider_name, wire_model, _, _) =
        config.resolve_model(&target_model).ok_or_else(|| {
            PurrError::Config(format!(
                "Model '{target_model}' could not be resolved from configuration."
            ))
        })?;

    let audit_logger = Arc::new(AuditLogger::new(&purrcode_dir));
    let mut plugin_mgr = PluginManager::new();
    plugin_mgr.discover_all(workspace_root);
    let unenforced_caps = plugin_mgr.unenforced_capabilities();
    let plugin_dispatcher: Arc<dyn PluginHookDispatcher> = Arc::new(plugin_mgr);

    let model_preset_name = config
        .model_preset
        .clone()
        .unwrap_or_else(|| "free".to_string());
    let model_preset = ModelPresetLoader::load_by_name(&model_preset_name, workspace_root).ok();

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(workspace_root);
    let skills_index = skill_mgr.format_prompt_index();
    let memory = MarkdownMemory::new(&purrcode_dir);
    let mem_context = memory.load_context(Some(workspace_root)).await;
    let mut system_prompt = MeowstroOrchestrator::system_prompt(&skills_index);
    if !mem_context.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&mem_context);
    }

    Ok(CronRunnerDeps {
        config: config.clone(),
        workspace_root: workspace_root.to_path_buf(),
        purrcode_dir,
        registry: Arc::new(registry),
        provider_name: provider_name.to_string(),
        wire_model: wire_model.to_string(),
        audit_logger,
        plugin_dispatcher,
        unenforced_caps,
        model_preset,
        system_prompt,
    })
}

#[cfg(test)]
mod tests;
