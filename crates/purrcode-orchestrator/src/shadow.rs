// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::{AuditSink, ToolContext, ToolRegistry};
use purrcode_config::GuardrailMode;
use purrcode_core::{AgentLoop, ApprovalRequester};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowVerdict {
    Approve,
    ApproveWithNotes { notes: String },
    Object { reason: String },
}

/// Parses Tuxeshadow's raw output into a structured `ShadowVerdict` (ADR-012).
pub fn parse_shadow_verdict(raw: &str) -> ShadowVerdict {
    for line in raw.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches('*')
            .trim_start_matches('-')
            .trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("OBJECT") || upper.contains("VERDICT: OBJECT") {
            let reason = trimmed
                .split_once(':')
                .map(|(_, r)| r.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| raw.trim().to_string());
            return ShadowVerdict::Object { reason };
        }
        if upper.starts_with("APPROVE_WITH_NOTES") || upper.contains("VERDICT: APPROVE_WITH_NOTES")
        {
            let notes = trimmed
                .split_once(':')
                .map(|(_, r)| r.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| raw.trim().to_string());
            return ShadowVerdict::ApproveWithNotes { notes };
        }
    }

    if raw.to_ascii_uppercase().contains("OBJECT:") {
        return ShadowVerdict::Object {
            reason: raw.trim().to_string(),
        };
    }
    if raw.to_ascii_uppercase().contains("APPROVE_WITH_NOTES") {
        return ShadowVerdict::ApproveWithNotes {
            notes: raw.trim().to_string(),
        };
    }

    ShadowVerdict::Approve
}

/// Determines whether Tuxeshadow's narrow event-driven trigger fires for a delegation (ADR-012):
/// 1. Mandatory on cron jobs (`is_cron`).
/// 2. Mandatory on recovery replans after error (`is_replan`).
/// 3. Destructive / high-risk implementation briefs (`rm`, `delete`, `reset --hard`, `../`, outside workspace).
pub fn should_trigger_shadow(role: &str, raw_brief: &str, is_cron: bool, is_replan: bool) -> bool {
    if role == "tuxeshadow" || role == "whisker" || role == "sphinx" || role == "clowder" {
        return false;
    }
    if is_cron || is_replan {
        return true;
    }
    let lower = raw_brief.to_ascii_lowercase();
    let risky_markers = [
        "rm -rf",
        "rm ",
        "del ",
        "delete",
        "drop table",
        "reset --hard",
        "outside workspace",
        "../",
        "/etc/",
        "overwrite",
        "destructive",
    ];
    risky_markers.iter().any(|m| lower.contains(m))
}

pub struct ShadowAuditor {
    provider: Arc<dyn ModelProvider>,
    model: String,
    guardrail: GuardrailMode,
    audit_sink: Option<Arc<dyn AuditSink>>,
    approval_requester: Option<Arc<dyn ApprovalRequester>>,
}

impl ShadowAuditor {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        guardrail: GuardrailMode,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            guardrail,
            audit_sink: None,
            approval_requester: None,
        }
    }

    pub fn with_audit_sink(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = Some(sink);
        self
    }

    pub fn with_approval_requester(mut self, req: Arc<dyn ApprovalRequester>) -> Self {
        self.approval_requester = Some(req);
        self
    }

    /// Evaluates raw brief/artifact with Tuxeshadow (read-only toolset, no `task` tool)
    pub async fn audit_raw_artifact(
        &self,
        raw_artifact: &str,
        ctx: &ToolContext,
    ) -> Result<ShadowVerdict> {
        let mut reg = ToolRegistry::new();
        let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
        let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
        let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
        let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
        reg.freeze();

        let loop_engine =
            AgentLoop::new(self.provider.clone(), &self.model, Arc::new(reg)).with_max_steps(5);

        let sys_prompt = include_str!("../../../agents/tuxeshadow.md");
        let shadow_ctx = ToolContext {
            session_id: format!("{}:tuxeshadow", ctx.session_id),
            agent_role: "tuxeshadow".to_string(),
            workspace_root: ctx.workspace_root.clone(),
            is_dry_run: true,
        };

        let prompt = format!(
            "Audit this raw delegation artifact before execution. Reply starting with APPROVE, APPROVE_WITH_NOTES: <notes>, or OBJECT: <reason>.\n\n---\n{}\n---",
            raw_artifact
        );

        let output = loop_engine
            .run_turn(&prompt, Some(sys_prompt), &shadow_ctx, |_, _| {})
            .await?;

        Ok(parse_shadow_verdict(&output))
    }

    /// Enforces a `ShadowVerdict` according to the active `GuardrailMode` (ADR-012):
    /// - `Strict` (`carrier`): `Object` blocks execution unless approved by user.
    /// - `Default` (`tabby`): `Object` is advisory; if `override_reason` is supplied, logs `⚠ [shadow overridden]` and proceeds; otherwise returns advisory objection.
    /// - `God` (`oyen`): silent audit log only; never blocks.
    pub async fn enforce_verdict(
        &self,
        verdict: &ShadowVerdict,
        session_id: &str,
        override_reason: Option<&str>,
    ) -> Result<Option<String>> {
        match verdict {
            ShadowVerdict::Approve => {
                if let Some(ref sink) = self.audit_sink {
                    sink.record_event(
                        session_id,
                        "tuxeshadow",
                        "shadow_audit",
                        None,
                        "APPROVE",
                        "Plan approved by Tuxeshadow",
                    );
                }
                Ok(None)
            }
            ShadowVerdict::ApproveWithNotes { notes } => {
                if let Some(ref sink) = self.audit_sink {
                    sink.record_event(
                        session_id,
                        "tuxeshadow",
                        "shadow_audit",
                        None,
                        "APPROVE_WITH_NOTES",
                        notes,
                    );
                }
                Ok(Some(format!("[Tuxeshadow Advisory Note]: {}", notes)))
            }
            ShadowVerdict::Object { reason } => match self.guardrail {
                GuardrailMode::God => {
                    // Oyen (God mode): silent auditor — logs to audit.jsonl without blocking
                    if let Some(ref sink) = self.audit_sink {
                        sink.record_event(
                            session_id,
                            "tuxeshadow",
                            "shadow_silent_audit",
                            None,
                            "OBJECT_SILENT",
                            reason,
                        );
                    }
                    Ok(None)
                }
                GuardrailMode::Default => {
                    // Tabby (Default): advisory — Meowstro can override with a recorded reason
                    if let Some(ov_reason) = override_reason {
                        let detail = format!(
                            "⚠ [shadow overridden] objection='{}' override_reason='{}'",
                            reason, ov_reason
                        );
                        if let Some(ref sink) = self.audit_sink {
                            sink.record_event(
                                session_id,
                                "meowstro",
                                "shadow_override",
                                None,
                                "OVERRIDDEN",
                                &detail,
                            );
                        }
                        Ok(Some(detail))
                    } else {
                        if let Some(ref sink) = self.audit_sink {
                            sink.record_event(
                                session_id,
                                "tuxeshadow",
                                "shadow_audit",
                                None,
                                "OBJECT",
                                reason,
                            );
                        }
                        Err(PurrError::ApprovalDenied(format!(
                            "Tuxeshadow objected to plan: {}. Provide `shadow_override_reason` in task() to override in Tabby mode or revise the plan.",
                            reason
                        )))
                    }
                }
                GuardrailMode::Strict => {
                    // Carrier (Strict): objection blocks unless user explicitly approves
                    if let Some(ref req) = self.approval_requester {
                        let approved = req
                            .request_approval(
                                "tuxeshadow",
                                &format!("Tuxeshadow OBJECTED in Strict mode: {}", reason),
                                None,
                            )
                            .await;
                        if approved {
                            if let Some(ref sink) = self.audit_sink {
                                sink.record_event(
                                    session_id,
                                    "tuxeshadow",
                                    "shadow_strict_override",
                                    None,
                                    "APPROVED_BY_USER",
                                    reason,
                                );
                            }
                            return Ok(Some(format!(
                                "[Tuxeshadow Objection Approved by User]: {}",
                                reason
                            )));
                        }
                    }
                    if let Some(ref sink) = self.audit_sink {
                        sink.record_event(
                            session_id,
                            "tuxeshadow",
                            "shadow_audit",
                            None,
                            "OBJECT_BLOCKED",
                            reason,
                        );
                    }
                    Err(PurrError::ApprovalDenied(format!(
                        "Tuxeshadow blocked execution in Carrier (Strict) mode: {}",
                        reason
                    )))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct CaptureSink {
        entries: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    impl AuditSink for CaptureSink {
        fn record_event(
            &self,
            _session_id: &str,
            agent: &str,
            event: &str,
            _tool: Option<&str>,
            decision: &str,
            detail: &str,
        ) {
            self.entries.lock().unwrap().push((
                format!("{}:{}", agent, event),
                decision.to_string(),
                detail.to_string(),
            ));
        }
    }

    struct DummyProvider;
    #[async_trait::async_trait]
    impl ModelProvider for DummyProvider {
        fn provider_name(&self) -> &str {
            "dummy"
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stream_completion(
            &self,
            _req: purrcode_api::provider::CompletionRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<purrcode_api::provider::StreamChunk>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[test]
    fn test_parse_shadow_verdicts_and_triggers() {
        assert_eq!(
            parse_shadow_verdict("APPROVE: Looks good"),
            ShadowVerdict::Approve
        );
        assert_eq!(
            parse_shadow_verdict("APPROVE_WITH_NOTES: Check edge case x=0"),
            ShadowVerdict::ApproveWithNotes {
                notes: "Check edge case x=0".to_string()
            }
        );
        assert_eq!(
            parse_shadow_verdict("OBJECT: Unbounded rm -rf outside workspace"),
            ShadowVerdict::Object {
                reason: "Unbounded rm -rf outside workspace".to_string()
            }
        );

        assert!(should_trigger_shadow(
            "tinker",
            "Run rm -rf on target",
            false,
            false
        ));
        assert!(should_trigger_shadow(
            "tinker",
            "Normal edit",
            true, // cron job always triggers
            false
        ));
        assert!(!should_trigger_shadow(
            "whisker",
            "Search for rm -rf",
            false,
            false
        ));
    }

    #[tokio::test]
    async fn test_shadow_enforcement_across_modes() {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<dyn AuditSink> = Arc::new(CaptureSink {
            entries: entries.clone(),
        });
        let provider: Arc<dyn ModelProvider> = Arc::new(DummyProvider);

        let objection = ShadowVerdict::Object {
            reason: "Missing unit test for auth".to_string(),
        };

        // 1. Strict mode blocks without user approval
        let strict_auditor = ShadowAuditor::new(provider.clone(), "m", GuardrailMode::Strict)
            .with_audit_sink(sink.clone());
        assert!(strict_auditor
            .enforce_verdict(&objection, "s1", Some("try override"))
            .await
            .is_err());

        // 2. Default (Tabby) mode blocks without override reason, succeeds & logs `⚠ [shadow overridden]` with reason
        let tabby_auditor = ShadowAuditor::new(provider.clone(), "m", GuardrailMode::Default)
            .with_audit_sink(sink.clone());
        assert!(tabby_auditor
            .enforce_verdict(&objection, "s1", None)
            .await
            .is_err());
        let note = tabby_auditor
            .enforce_verdict(&objection, "s1", Some("Auth test is in step 2"))
            .await
            .unwrap()
            .unwrap();
        assert!(note.contains("⚠ [shadow overridden]"));

        // 3. God (Oyen) mode logs silently without blocking
        let god_auditor =
            ShadowAuditor::new(provider, "m", GuardrailMode::God).with_audit_sink(sink);
        assert!(god_auditor
            .enforce_verdict(&objection, "s1", None)
            .await
            .unwrap()
            .is_none());

        let logged = entries.lock().unwrap();
        assert!(logged
            .iter()
            .any(|(_, dec, det)| dec == "OVERRIDDEN" && det.contains("⚠ [shadow overridden]")));
        assert!(logged.iter().any(|(_, dec, _)| dec == "OBJECT_SILENT"));
    }
}
