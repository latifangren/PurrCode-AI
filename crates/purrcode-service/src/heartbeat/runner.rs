// SPDX-License-Identifier: GPL-3.0-or-later

//! Heartbeat / proactive runner (v0.3 F22, ADR-014 item 6).
//!
//! One tick (default every 30 minutes, minimum 5) inside the gateway
//! daemon:
//! 1. Read the user's `HEARTBEAT.md` task list (workspace first, then
//!    global). No file anywhere → create the global template and end
//!    silently; the tick costs no LLM call at all then.
//! 2. Apply `PROACTIVE_PREFERENCES.md`: outside the "When" window the
//!    whole tick is skipped; "Never tell me about" lines filter tasks
//!    out before the decider ever sees them.
//! 3. **Phase 1 (decide, ZeroClaw two-phase):** one cheap, tool-less
//!    model call — `skip` or `run: 1, 3`. Be conservative: an
//!    unparseable answer, a provider error, or an out-of-range index
//!    means SKIP. (Deliberate deviation from ZeroClaw, which runs ALL
//!    tasks when Phase-1 fails: an unattended run must never default
//!    to "run everything" when the decider failed.)
//! 4. **Phase 2 (run):** each selected item runs one read-only
//!    Meowstro turn in its own session `purr_hb_<8hex>`, under the
//!    same unattended rails as a cron run — `yolo=false`, the
//!    deny-always requester with the read-only allowlist, `ask` never
//!    answered, `is_cron: true` in the tool context (Tuxeshadow
//!    mandatory, override reasons ignored), preset guardrail God /
//!    approval yolo rejected. One line per item that produced
//!    something (PRD: "satu baris per thread yang menunggu").
//! 5. Silence is a first-class outcome: when every item answers
//!    `HEARTBEAT_OK` / `NO_REPLY` (or nothing at all), no delivery is
//!    created. Otherwise one combined `Heartbeat` delivery goes into
//!    the same queue cron uses, to the last active Telegram chat (or
//!    the first `allow_from` entry, or queue-only when there is no
//!    chat), and the tick drains the queue through the cron drainer.
//!
//! The gateway tick (`tick_once`) never fails — its own error
//! boundary, like the cron tick. The foreground `run_once` (CLI
//! `heartbeat run`) surfaces setup errors to the caller instead.

use crate::cron::queue::{new_delivery_id, Delivery, DeliveryKind, DeliveryQueue};
use crate::cron::runner::{
    cap_detail, summarize, CronApprovalRequester, CronAskRequester, CronRunner, CronRunnerDeps,
};
use crate::cron::store::DEFAULT_ALLOWLIST;
use crate::heartbeat::prefs::ProactivePrefs;
use crate::heartbeat::tasks::{load_tasks, HeartbeatTask};
use chrono::Local;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{ChatMessage, CompletionRequest, MessageRole};
use purrcode_api::tool::{AuditSink, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry};
use purrcode_channel::telegram::TelegramChannel;
use purrcode_config::{ApprovalMode, GuardrailMode, ModePresetLoader};
use purrcode_core::SessionStore;
use purrcode_orchestrator::MeowstroOrchestrator;
use std::collections::HashSet;
use std::sync::Arc;

/// Outcome of one heartbeat tick, for the CLI and the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatDecision {
    /// No task file had tasks (or the template was just created).
    NoTasks,
    /// The whole tick was outside the proactive time window.
    OutsideWindow,
    /// The decider said `skip`, failed, or answered unparseably.
    SkippedByDecider,
    /// At least one item ran (whether or not it produced a delivery).
    Ran,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatOutcome {
    pub decision: HeartbeatDecision,
    /// One line per item that produced something worth telling.
    pub lines: Vec<String>,
    /// True when a delivery was enqueued (Telegram chat or queue).
    pub delivered: bool,
    pub delivery_id: Option<String>,
}

/// The Phase-1 decider's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase1Decision {
    Skip,
    /// 1-based task indices, in listed order, de-duplicated.
    Run(Vec<usize>),
}

/// Parse the decider's raw text. Strict and fail-closed: anything
/// that is not exactly `skip` / `HEARTBEAT_OK` / `NO_REPLY` or a
/// well-formed `run: <1-based indices within range>` is a skip.
/// (Pure function, unit-tested.)
fn parse_decision(text: &str, task_count: usize) -> Phase1Decision {
    let t = text.trim();
    let lower = t.to_lowercase();
    if lower == "skip" || lower == "heartbeat_ok" || lower == "no_reply" || lower == "run: none" {
        return Phase1Decision::Skip;
    }
    let Some(rest) = lower.strip_prefix("run:") else {
        return Phase1Decision::Skip;
    };
    let mut indices: Vec<usize> = Vec::new();
    for part in rest.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Phase1Decision::Skip;
        }
        match part.parse::<usize>() {
            Ok(n) if (1..=task_count).contains(&n) => indices.push(n),
            _ => return Phase1Decision::Skip,
        }
    }
    if indices.is_empty() {
        return Phase1Decision::Skip;
    }
    indices.sort_unstable();
    indices.dedup();
    Phase1Decision::Run(indices)
}

/// A Phase-2 turn's final text is silent when it is empty or exactly
/// one of the silence sentinels. Pure function (unit-tested).
fn is_silence(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "" | "heartbeat_ok" | "no_reply" | "skip"
    )
}

pub struct HeartbeatRunner {
    deps: CronRunnerDeps,
    drainer: CronRunner,
}

impl HeartbeatRunner {
    pub fn new(deps: CronRunnerDeps) -> Self {
        let drainer = CronRunner::new(deps.clone());
        Self { deps, drainer }
    }

    /// One gateway tick. Never fails: every error is logged and the
    /// tick ends (its own error boundary, ADR-014 item 5). Drains the
    /// delivery queue afterwards, so a heartbeat delivery is sent on
    /// the same tick it was created.
    pub async fn tick_once(&self, channel: &TelegramChannel) {
        let outcome = self.run_once(Some(channel)).await;
        match outcome {
            Ok(outcome) => {
                tracing::info!(
                    "heartbeat: tick done: decision={:?} lines={} delivered={}",
                    outcome.decision,
                    outcome.lines.len(),
                    outcome.delivered
                );
            }
            Err(e) => {
                tracing::warn!("heartbeat: tick failed: {e}");
                let _ = self.deps.audit_logger.log_entry(AuditEntry {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    session_id: "heartbeat".to_string(),
                    agent: "meowstro".to_string(),
                    event: "heartbeat_tick".to_string(),
                    tool: None,
                    decision: "deny".to_string(),
                    detail: format!("tick failed: {e}"),
                });
            }
        }
        self.drainer.drain_deliveries(channel).await;
    }

    /// Run one tick in the foreground (CLI `heartbeat run`). Setup
    /// failures (missing provider, forbidden preset) are returned as
    /// errors; a decider skip or a silent run is an `Ok` outcome.
    pub async fn run_once(&self, channel: Option<&TelegramChannel>) -> Result<HeartbeatOutcome> {
        let now = Local::now();
        let tick = |decision: &str, detail: String| {
            let _ = self.deps.audit_logger.log_entry(AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session_id: "heartbeat".to_string(),
                agent: "meowstro".to_string(),
                event: "heartbeat_tick".to_string(),
                tool: None,
                decision: decision.to_string(),
                detail,
            });
        };

        // 1. Time window first: outside it, nothing is checked and
        // nothing is delivered — the tick ends before any LLM call.
        let prefs = ProactivePrefs::load(&self.deps.purrcode_dir);
        if !prefs.is_in_window(now) {
            let (start, end) = prefs.window.unwrap();
            tick(
                "deny",
                format!("outside proactive window {start}–{end} (local time)"),
            );
            return Ok(HeartbeatOutcome {
                decision: HeartbeatDecision::OutsideWindow,
                lines: Vec::new(),
                delivered: false,
                delivery_id: None,
            });
        }

        // 2. Tasks (missing file → template + silent end).
        let load = load_tasks(&self.deps.workspace_root, &self.deps.purrcode_dir)?;
        if load.created_template {
            tick(
                "deny",
                format!(
                    "no HEARTBEAT.md found; created template at {}",
                    load.source
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ),
            );
            return Ok(HeartbeatOutcome {
                decision: HeartbeatDecision::NoTasks,
                lines: Vec::new(),
                delivered: false,
                delivery_id: None,
            });
        }
        if load.tasks.is_empty() {
            tick("deny", "HEARTBEAT.md has no tasks".to_string());
            return Ok(HeartbeatOutcome {
                decision: HeartbeatDecision::NoTasks,
                lines: Vec::new(),
                delivered: false,
                delivery_id: None,
            });
        }

        // 3. Hard "Never tell me about" filter, before Phase-1.
        let tasks: Vec<HeartbeatTask> = load
            .tasks
            .into_iter()
            .filter(|t| !prefs.is_denied(&t.text))
            .collect();
        if tasks.is_empty() {
            tick(
                "deny",
                "all tasks denied by PROACTIVE_PREFERENCES.md (Never tell me about)".to_string(),
            );
            return Ok(HeartbeatOutcome {
                decision: HeartbeatDecision::NoTasks,
                lines: Vec::new(),
                delivered: false,
                delivery_id: None,
            });
        }
        // Re-number after filtering (the decider sees 1..=N).
        let tasks: Vec<HeartbeatTask> = tasks
            .into_iter()
            .enumerate()
            .map(|(i, mut t)| {
                t.index = i + 1;
                t
            })
            .collect();

        // The session's preset, loaded by name like the gateway does.
        // Unattended runs never use God guardrail or yolo approval:
        // this is a configuration error, so it fails the whole tick
        // before Phase-1 spends a model call (not per-item).
        // A preset that fails to load (unknown name, unparseable file,
        // `sandbox: "os"` — rejected by the loader, ADR-014 item 8)
        // FAILS the tick loudly: silently falling back to tabby would
        // hide the rejection and quietly replace a configured Strict
        // preset with Default (v0.3 F22 fix, re-checker F2). The
        // gateway's own startup still falls back (pre-4a pattern);
        // the heartbeat tick does not.
        let preset = ModePresetLoader::load_by_name(
            &self.deps.config.preset,
            &self.deps.workspace_root,
        )
        .map_err(|e| {
            PurrError::Config(format!(
                "Heartbeat: failed to load mode preset '{}': {e}. The tick refuses to run under a silent fallback.",
                self.deps.config.preset
            ))
        })?;
        if preset.guardrail == GuardrailMode::God {
            return Err(PurrError::Config(
                "Heartbeat items never run with guardrail 'god' (preset declares God).".to_string(),
            ));
        }
        if preset.approval == ApprovalMode::Yolo {
            return Err(PurrError::Config(
                "Heartbeat items never run with approval 'yolo' (preset declares yolo)."
                    .to_string(),
            ));
        }

        // 4. Phase 1: decide which checks, if any, are worth running.
        let decision = match self.decide(&tasks, &prefs).await {
            Ok(d) => d,
            Err(e) => {
                let _ = self.deps.audit_logger.log_entry(AuditEntry {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    session_id: "heartbeat".to_string(),
                    agent: "meowstro".to_string(),
                    event: "heartbeat_decide_failed".to_string(),
                    tool: None,
                    decision: "deny".to_string(),
                    detail: format!("Phase-1 decider failed: {e} (failing closed: skip)"),
                });
                tick("deny", format!("Phase-1 decider failed: {e}"));
                return Ok(HeartbeatOutcome {
                    decision: HeartbeatDecision::SkippedByDecider,
                    lines: Vec::new(),
                    delivered: false,
                    delivery_id: None,
                });
            }
        };
        let selected = match decision {
            Phase1Decision::Skip => {
                tick("deny", "decider: skip".to_string());
                return Ok(HeartbeatOutcome {
                    decision: HeartbeatDecision::SkippedByDecider,
                    lines: Vec::new(),
                    delivered: false,
                    delivery_id: None,
                });
            }
            Phase1Decision::Run(indices) => indices,
        };

        // 5. Phase 2: run the selected items sequentially.
        let mut lines: Vec<String> = Vec::new();
        for index in &selected {
            let task = &tasks[index - 1];
            match self.run_item(task, &preset).await {
                Ok(Some(line)) => lines.push(line),
                Ok(None) => {}
                Err(e) => {
                    let _ = self.deps.audit_logger.log_entry(AuditEntry {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        session_id: "heartbeat".to_string(),
                        agent: "meowstro".to_string(),
                        event: "heartbeat_run_failed".to_string(),
                        tool: None,
                        decision: "deny".to_string(),
                        detail: format!("item {} ({}): {e}", task.index, task.text),
                    });
                    lines.push(format!(
                        "⚠️ {} — failed: {}",
                        task.text,
                        summarize(&e.to_string())
                    ));
                }
            }
        }

        // 6. Silence is a first-class outcome: no delivery at all.
        if lines.is_empty() {
            tick(
                "allow",
                format!("ran {} item(s); all silent, no delivery", selected.len()),
            );
            return Ok(HeartbeatOutcome {
                decision: HeartbeatDecision::Ran,
                lines: Vec::new(),
                delivered: false,
                delivery_id: None,
            });
        }

        // 7. One combined delivery (PRD: one line per waiting
        // thread), to the last active chat / first allow_from / queue.
        let chat_id = self.resolve_delivery_chat(channel).await;
        let detail = cap_detail(&lines.join("\n"));
        let summary = summarize(lines.first().map(|s| s.as_str()).unwrap_or(""));
        let delivery = Delivery {
            id: new_delivery_id(),
            job_id: "heartbeat".to_string(),
            job_name: "Heartbeat".to_string(),
            created_at: now,
            kind: DeliveryKind::Heartbeat,
            summary: summary.clone(),
            detail,
            chat_id,
        };
        let delivery_id = delivery.id.clone();
        let queue = DeliveryQueue::new(&self.deps.purrcode_dir);
        if let Err(e) = queue.create(&delivery) {
            tracing::warn!("heartbeat: failed to enqueue delivery: {e}");
        }
        tick(
            "allow",
            format!(
                "ran {} item(s); delivery {} to chat {}",
                selected.len(),
                delivery_id,
                chat_id
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "queue-only".to_string())
            ),
        );
        Ok(HeartbeatOutcome {
            decision: HeartbeatDecision::Ran,
            lines,
            delivered: true,
            delivery_id: Some(delivery_id),
        })
    }

    /// Delivery routing (ADR-014 item 10): the last active PRIVATE
    /// Telegram chat first (groups are never delivery targets — a
    /// heartbeat summary may quote the user's private checks; v0.3
    /// F22 fix, re-checker F1), else the first `allow_from` entry
    /// (config order — deterministic, unlike the channel's `HashSet`),
    /// else queue-only (the TUI shows it at the next open).
    async fn resolve_delivery_chat(&self, channel: Option<&TelegramChannel>) -> Option<i64> {
        if let Some(ch) = channel {
            if let Some(id) = ch.last_active_private_chat().await {
                return Some(id);
            }
        }
        self.deps
            .config
            .telegram
            .as_ref()
            .and_then(|tg| tg.allow_from.first().copied())
    }

    /// Phase 1: one cheap, tool-less model call. The decider sees the
    /// numbered task list plus the user's "Tell me about" context and
    /// answers `skip` or `run: 1, 3` — nothing else.
    async fn decide(
        &self,
        tasks: &[HeartbeatTask],
        prefs: &ProactivePrefs,
    ) -> Result<Phase1Decision> {
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
        let mut user = format!(
            "Current local time: {}.\nRecurring checks (answer with their numbers):\n",
            Local::now().format("%Y-%m-%d %H:%M")
        );
        for task in tasks {
            user.push_str(&format!("{}. {}\n", task.index, task.text));
        }
        let tell = prefs.tell_context();
        if !tell.is_empty() {
            user.push_str(&tell);
            user.push('\n');
        }
        let request = CompletionRequest {
            model: self.deps.wire_model.clone(),
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "You are the heartbeat decider for PurrCode AI, a personal coding agent. The user wrote a list of recurring checks. Your job is ONLY to decide which checks, if any, are worth running right now. Be conservative: run a check only when its text describes something that can change between ticks and there is a real reason to check it now. Never invent new checks. Answer with exactly one line: either `skip` (nothing is worth running now) or `run: 1, 3` (the 1-based numbers of the checks to run, in the order listed). Nothing else.".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: user,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(256),
            reasoning_effort: None,
            stop: None,
        };
        let mut stream = provider.stream_completion(request).await?;
        let mut text = String::new();
        while let Some(chunk) = stream.recv().await {
            text.push_str(&chunk.delta);
            if chunk.is_finished {
                break;
            }
        }
        Ok(parse_decision(&text, tasks.len()))
    }

    /// Phase 2: one read-only Meowstro turn for one selected item,
    /// under the cron-run rails (see the module docs). Returns the
    /// item's one line, or `None` when the turn was silent (empty or
    /// a silence sentinel). A turn that errors returns `Err` (the
    /// caller records it as a failed line, not a tick failure).
    async fn run_item(
        &self,
        task: &HeartbeatTask,
        preset: &purrcode_config::ModePreset,
    ) -> Result<Option<String>> {
        let allow_set: HashSet<String> = DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect();
        let gate = Arc::new(
            ApprovalGate::from_preset(preset, false).with_tool_allowlist(allow_set.clone()),
        );

        let run_hex = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
        let session_id = format!("purr_hb_{run_hex}");
        let audit_sink: Arc<dyn AuditSink> = self.deps.audit_logger.clone();
        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            agent: "meowstro".to_string(),
            event: "heartbeat_run_start".to_string(),
            tool: None,
            decision: "allow".to_string(),
            detail: format!("item {}: {}", task.index, task.text),
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
        let approval_req = Arc::new(CronApprovalRequester::new(
            allow_set,
            Some(audit_sink.clone()),
            session_id.clone(),
        ));
        let ask_req = Arc::new(CronAskRequester);
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
            // Unattended daemon turn, like a cron run: Tuxeshadow is
            // mandatory and a model-supplied override reason is ignored.
            is_cron: true,
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
                &format!(
                    "Heartbeat check #{}: {}\n\nCheck this now, read-only. If there is nothing new or nothing worth telling the user, answer exactly HEARTBEAT_OK and nothing else. Otherwise give a one-line summary of what changed and why it is worth telling.",
                    task.index, task.text
                ),
                Some(&self.deps.system_prompt),
                &ctx,
                None,
            )
            .await;
        for msg in &messages {
            if let Err(e) = session_store.append_message(&session_id, msg) {
                tracing::warn!("heartbeat: failed to persist message for {session_id}: {e}");
            }
        }
        self.deps
            .plugin_dispatcher
            .dispatch_session_stop(&session_id)
            .await;

        let (ok, raw_text) = match &turn {
            Ok(text) => {
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
        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            agent: "meowstro".to_string(),
            event: if ok {
                "heartbeat_run_ok"
            } else {
                "heartbeat_run_failed"
            }
            .to_string(),
            tool: None,
            decision: if ok { "allow" } else { "deny" }.to_string(),
            detail: format!(
                "item {} ({}): {}",
                task.index,
                task.text,
                summarize(&raw_text)
            ),
        });
        if !ok {
            return Err(PurrError::Tool {
                tool: "heartbeat".to_string(),
                message: format!("heartbeat item {} failed: {}", task.index, raw_text),
            });
        }
        if is_silence(&raw_text) {
            return Ok(None);
        }
        Ok(Some(summarize(&raw_text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_decision_skip_forms() {
        for raw in [
            "skip",
            "SKIP",
            "  skip\n",
            "HEARTBEAT_OK",
            "heartbeat_ok",
            "NO_REPLY",
            "run: none",
        ] {
            assert_eq!(parse_decision(raw, 3), Phase1Decision::Skip, "{raw}");
        }
    }

    #[test]
    fn test_parse_decision_run_forms() {
        assert_eq!(
            parse_decision("run: 1, 3", 3),
            Phase1Decision::Run(vec![1, 3])
        );
        assert_eq!(parse_decision("run: 2", 3), Phase1Decision::Run(vec![2]));
        // Duplicates collapse, order normalizes.
        assert_eq!(
            parse_decision("run: 3, 1, 3", 3),
            Phase1Decision::Run(vec![1, 3])
        );
    }

    #[test]
    fn test_parse_decision_invalid_is_skip() {
        // Anything unparseable fails closed to Skip — never "run all".
        for raw in [
            "run: 0",
            "run: 4",
            "run: 1, 9",
            "run:",
            "run: one",
            "run: 1,,2",
            "1, 2",
            "Yes, run them all",
            "",
            "run: 1 2",
        ] {
            assert_eq!(parse_decision(raw, 3), Phase1Decision::Skip, "{raw}");
        }
    }

    #[test]
    fn test_is_silence() {
        assert!(is_silence(""));
        assert!(is_silence("  HEARTBEAT_OK\n"));
        assert!(is_silence("no_reply"));
        assert!(is_silence("skip"));
        assert!(!is_silence("The build failed."));
        assert!(!is_silence("HEARTBEAT_OK-ish"));
    }
}
