// SPDX-License-Identifier: GPL-3.0-or-later

//! Approval/ask requesters for unattended daemon runs (split from
//! `runner/mod.rs`, v0.3 polish, ADR-011).
//!
//! Shared by the cron runner and the heartbeat runner (v0.3 F22) — the
//! rails are identical: a tool the job declared is approved, anything
//! else is denied, and `ask` never gets an answer (there is no user to
//! ask in a daemon turn).

use async_trait::async_trait;
use purrcode_api::tool::AuditSink;
use purrcode_core::ApprovalRequester;
use purrcode_tools::{AskOption, AskRequester};
use std::collections::HashSet;
use std::sync::Arc;

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
