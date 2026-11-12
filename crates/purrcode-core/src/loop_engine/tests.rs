// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use purrcode_api::provider::StreamChunk;
use purrcode_api::tool::{Tool, ToolDefinition, ToolResult};
use std::sync::Mutex;

struct DryRunMockTool;

#[async_trait]
impl Tool for DryRunMockTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_mock".to_string(),
            description: "Mock write tool".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            tier: ApprovalTier::Write,
        }
    }

    async fn execute(
        &self,
        _arguments: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        if ctx.is_dry_run {
            Ok(ToolResult {
                output: "dry-run".to_string(),
                diff_preview: Some("+added line".to_string()),
                metadata: None,
            })
        } else {
            Ok(ToolResult {
                output: "written".to_string(),
                diff_preview: Some("+added line".to_string()),
                metadata: None,
            })
        }
    }
}

struct CaptureApprover {
    captured_preview: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl ApprovalRequester for CaptureApprover {
    async fn request_approval(
        &self,
        _call_id: &str,
        _tool: &str,
        _description: &str,
        preview: Option<&str>,
    ) -> bool {
        if let Ok(mut g) = self.captured_preview.lock() {
            *g = preview.map(|s| s.to_string());
        }
        true
    }
}

struct CaptureAuditSink {
    decisions: Arc<Mutex<Vec<(String, String)>>>,
}

impl AuditSink for CaptureAuditSink {
    fn record_event(
        &self,
        _session_id: &str,
        _agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        _detail: &str,
    ) {
        if let Ok(mut g) = self.decisions.lock() {
            g.push((
                format!("{}:{}", event, tool.unwrap_or("")),
                decision.to_string(),
            ));
        }
    }
}

struct DenyGate;
impl PolicyGate for DenyGate {
    fn evaluate_tool(
        &self,
        _tool_name: &str,
        _tier: ApprovalTier,
        _is_outside_workspace: bool,
    ) -> ApprovalDecision {
        ApprovalDecision::Deny
    }
}

struct VetoPluginDispatcher;
#[async_trait]
impl PluginHookDispatcher for VetoPluginDispatcher {
    async fn dispatch_session_start(&self, _session_id: &str) {}
    async fn dispatch_pre_tool_use(&self, _tool: &str, _args: &serde_json::Value) -> bool {
        false
    }
    async fn dispatch_post_tool_use(&self, _tool: &str, _result: &serde_json::Value) {}
    async fn dispatch_session_stop(&self, _session_id: &str) {}
}

struct SingleToolTurnProvider {
    step: AtomicBool,
}

#[async_trait]
impl ModelProvider for SingleToolTurnProvider {
    fn provider_name(&self) -> &str {
        "mock"
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn stream_completion(
        &self,
        _req: CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        if !self.step.swap(true, Ordering::SeqCst) {
            let _ = tx.try_send(StreamChunk {
                delta: String::new(),
                reasoning_delta: None,
                tool_call_delta: Some(ToolCallDelta {
                    index: 0,
                    id: Some("call_1".to_string()),
                    name: Some("write_mock".to_string()),
                    arguments_delta: Some("{}".to_string()),
                }),
                is_finished: true,
                usage: None,
            });
        } else {
            let _ = tx.try_send(StreamChunk {
                delta: "Done!".to_string(),
                reasoning_delta: None,
                tool_call_delta: None,
                is_finished: true,
                usage: None,
            });
        }
        Ok(rx)
    }
}

#[tokio::test]
async fn test_dry_run_diff_preview_and_audit_sink() {
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(DryRunMockTool)).unwrap();
    let captured = Arc::new(Mutex::new(None));
    let approver = Arc::new(CaptureApprover {
        captured_preview: captured.clone(),
    });
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(CaptureAuditSink {
        decisions: decisions.clone(),
    });
    let provider = Arc::new(SingleToolTurnProvider {
        step: AtomicBool::new(false),
    });

    let engine = AgentLoop::new(provider, "m", Arc::new(reg))
        .with_approval_requester(approver)
        .with_audit_sink(sink);
    let ctx = ToolContext {
        session_id: "s1".to_string(),
        agent_role: "tinker".to_string(),
        workspace_root: std::path::PathBuf::from("."),
        is_dry_run: false,
        is_cron: false,
    };

    let out = engine
        .run_turn("write file", None, &ctx, |_, _| {})
        .await
        .unwrap();
    assert_eq!(out, "Done!");
    assert_eq!(captured.lock().unwrap().as_deref(), Some("+added line"));
    assert_eq!(
        decisions.lock().unwrap()[0],
        (
            "tool_approval:write_mock".to_string(),
            "approved_by_user".to_string()
        )
    );
}

#[tokio::test]
async fn test_policy_gate_deny_blocks_tool_and_audits() {
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(DryRunMockTool)).unwrap();
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(CaptureAuditSink {
        decisions: decisions.clone(),
    });
    let provider = Arc::new(SingleToolTurnProvider {
        step: AtomicBool::new(false),
    });

    let engine = AgentLoop::new(provider, "m", Arc::new(reg))
        .with_policy_gate(Arc::new(DenyGate))
        .with_audit_sink(sink);
    let ctx = ToolContext {
        session_id: "s1".to_string(),
        agent_role: "tinker".to_string(),
        workspace_root: std::path::PathBuf::from("."),
        is_dry_run: false,
        is_cron: false,
    };

    let mut history = Vec::new();
    let _ = engine
        .run_turn_with_history(&mut history, "write file", None, &ctx, None)
        .await
        .unwrap();
    assert_eq!(
        decisions.lock().unwrap()[0],
        ("tool_call:write_mock".to_string(), "deny".to_string())
    );
    assert!(history
        .iter()
        .any(|m| m.role == MessageRole::Tool && m.content.contains("Action denied")));
}

#[tokio::test]
async fn test_plugin_pre_tool_use_veto_blocks_execution() {
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(DryRunMockTool)).unwrap();
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(CaptureAuditSink {
        decisions: decisions.clone(),
    });
    let provider = Arc::new(SingleToolTurnProvider {
        step: AtomicBool::new(false),
    });

    let engine = AgentLoop::new(provider, "m", Arc::new(reg))
        .with_plugin_dispatcher(Arc::new(VetoPluginDispatcher))
        .with_audit_sink(sink)
        .with_yolo(true);
    let ctx = ToolContext {
        session_id: "s1".to_string(),
        agent_role: "tinker".to_string(),
        workspace_root: std::path::PathBuf::from("."),
        is_dry_run: false,
        is_cron: false,
    };

    let mut history = Vec::new();
    let _ = engine
        .run_turn_with_history(&mut history, "write file", None, &ctx, None)
        .await
        .unwrap();
    assert_eq!(
        decisions.lock().unwrap()[0],
        (
            "plugin_hook:write_mock".to_string(),
            "vetoed_by_plugin".to_string()
        )
    );
    assert!(history
        .iter()
        .any(|m| m.role == MessageRole::Tool && m.content.contains("vetoed by pre_tool_use")));
}

/// Approver that records the call id the loop hands it (R3): the card a
/// channel renders from the bus event must resolve to this wait, so the
/// two ids have to be the same value.
struct CaptureCallIdApprover {
    captured_call_id: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl ApprovalRequester for CaptureCallIdApprover {
    async fn request_approval(
        &self,
        call_id: &str,
        _tool: &str,
        _description: &str,
        _preview: Option<&str>,
    ) -> bool {
        if let Ok(mut g) = self.captured_call_id.lock() {
            *g = Some(call_id.to_string());
        }
        true
    }
}

#[tokio::test]
async fn test_event_bus_receives_turn_events() {
    let bus = Arc::new(crate::EventBus::new(32));
    let mut rx = bus.subscribe();

    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(DryRunMockTool)).unwrap();

    let provider = Arc::new(SingleToolTurnProvider {
        step: AtomicBool::new(false),
    });

    let engine = AgentLoop::new(provider, "m", Arc::new(reg))
        .with_event_bus(bus.clone())
        .with_yolo(true);

    let ctx = ToolContext {
        session_id: "s_event_test".to_string(),
        agent_role: "tinker".to_string(),
        workspace_root: std::path::PathBuf::from("."),
        is_dry_run: false,
        is_cron: false,
    };

    let mut history = Vec::new();
    let _ = engine
        .run_turn_with_history(&mut history, "write file", None, &ctx, None)
        .await
        .unwrap();

    let mut received = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        received.push(ev);
    }

    assert!(!received.is_empty());
    let has_turn_started = received
        .iter()
        .any(|e| matches!(e, AgentEvent::TurnStarted { .. }));
    let has_tool_started = received
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStarted { .. }));
    let has_tool_finished = received
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallFinished { .. }));
    let has_turn_finished = received
        .iter()
        .any(|e| matches!(e, AgentEvent::TurnFinished { .. }));

    assert!(has_turn_started, "EventBus should receive TurnStarted");
    assert!(has_tool_started, "EventBus should receive ToolCallStarted");
    assert!(
        has_tool_finished,
        "EventBus should receive ToolCallFinished"
    );
    assert!(has_turn_finished, "EventBus should receive TurnFinished");

    // TurnStarted and TurnFinished carry the SAME turn id so a
    // consumer can pair them (R3 improvement over the dev-claude
    // reference, which minted a second uuid at the end).
    let started_id = received.iter().find_map(|e| match e {
        AgentEvent::TurnStarted { turn_id, .. } => Some(turn_id.clone()),
        _ => None,
    });
    let finished_id = received.iter().find_map(|e| match e {
        AgentEvent::TurnFinished { turn_id, .. } => Some(turn_id.clone()),
        _ => None,
    });
    assert_eq!(started_id, finished_id);
    assert!(started_id.is_some());
}

#[tokio::test]
async fn test_event_bus_approval_carries_loop_call_id() {
    let bus = Arc::new(crate::EventBus::new(32));
    let mut rx = bus.subscribe();

    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(DryRunMockTool)).unwrap();

    let captured = Arc::new(Mutex::new(None));
    let approver = Arc::new(CaptureCallIdApprover {
        captured_call_id: captured.clone(),
    });
    let provider = Arc::new(SingleToolTurnProvider {
        step: AtomicBool::new(false),
    });

    // No yolo: the Write tool goes through Ask, so the loop publishes
    // ToolCallApprovalRequested and then calls the requester.
    let engine = AgentLoop::new(provider, "m", Arc::new(reg))
        .with_event_bus(bus.clone())
        .with_approval_requester(approver);

    let ctx = ToolContext {
        session_id: "s_event_approval".to_string(),
        agent_role: "tinker".to_string(),
        workspace_root: std::path::PathBuf::from("."),
        is_dry_run: false,
        is_cron: false,
    };

    let mut history = Vec::new();
    let _ = engine
        .run_turn_with_history(&mut history, "write file", None, &ctx, None)
        .await
        .unwrap();

    // The mock provider emits the tool call with id "call_1".
    assert_eq!(captured.lock().unwrap().as_deref(), Some("call_1"));

    let mut received = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        received.push(ev);
    }
    let approval_id = received.iter().find_map(|e| match e {
        AgentEvent::ToolCallApprovalRequested { call_id, .. } => Some(call_id.clone()),
        _ => None,
    });
    assert_eq!(approval_id.as_deref(), Some("call_1"));
}
