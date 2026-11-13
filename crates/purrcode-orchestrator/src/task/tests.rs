// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use purrcode_api::provider::{CompletionRequest, MessageRole, StreamChunk};
use purrcode_config::{AgentModelSpec, Config, ModelTarget};
use purrcode_provider::ProviderRegistry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

struct StatefulEchoProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl ModelProvider for StatefulEchoProvider {
    fn provider_name(&self) -> &str {
        "mock-stateful"
    }
    fn is_healthy(&self) -> bool {
        true
    }
    async fn stream_completion(
        &self,
        req: CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let user_turns = req
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .count();
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        let _ = tx.try_send(StreamChunk {
            delta: format!(
                "call={} user_turns={} model={} effort={:?}",
                n, user_turns, req.model, req.reasoning_effort
            ),
            reasoning_delta: None,
            tool_call_delta: None,
            is_finished: true,
            usage: None,
            stream_error: None,
        });
        Ok(rx)
    }
}

#[test]
fn test_per_role_model_preset_resolution_and_precedence() {
    let mut agents = HashMap::new();
    agents.insert(
        "tuxeshadow".to_string(),
        AgentModelSpec {
            model: ModelTarget::Chain(vec![
                "oracle-primary".to_string(),
                "oracle-backup".to_string(),
            ]),
            variant: Some("high".to_string()),
            temperature: Some(0.1),
            options: None,
            fallback: vec!["oracle-last".to_string()],
        },
    );
    let preset = ModelPreset {
        schema: "model-preset/v1".to_string(),
        name: "test".to_string(),
        description: String::new(),
        extends: None,
        agents,
    };

    // 1. ModelPreset resolves primary, fallback chain, temperature, and high variant
    let spec = resolve_role_model_spec(
        "tuxeshadow",
        None,
        Some(&preset),
        "session-default",
        Path::new("."),
    );
    assert_eq!(spec.model, "oracle-primary");
    assert_eq!(spec.fallback_candidates.len(), 3);
    assert_eq!(spec.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(spec.temperature, Some(0.1));

    // 2. Explicit model in task() wins over ModelPreset and disables fallback chain
    let exp = resolve_role_model_spec(
        "tuxeshadow",
        Some("override-model"),
        Some(&preset),
        "session-default",
        Path::new("."),
    );
    assert_eq!(exp.model, "override-model");
    assert_eq!(exp.fallback_candidates, vec!["override-model".to_string()]);
}

#[test]
fn test_unresolved_role_model_falls_back_to_session_model() {
    // Preset names a provider (ollama) that is NOT configured anywhere:
    // the fallback chain resolves to nothing, so the child must run on
    // the SESSION provider + SESSION model — not on the preset's raw
    // model id wired to the wrong provider (which a strict provider
    // would reject with an unknown-model error, and which mislabels
    // the task result). Regression test for checker finding F1 (batch 2c).
    let temp_dir = std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let mut agents = HashMap::new();
    agents.insert(
        "tinker".to_string(),
        AgentModelSpec {
            model: ModelTarget::Single("ollama:qwen2.5-coder:7b".to_string()),
            variant: None,
            temperature: None,
            options: None,
            fallback: vec![],
        },
    );
    let preset = ModelPreset {
        schema: "model-preset/v1".to_string(),
        name: "test".to_string(),
        description: String::new(),
        extends: None,
        agents,
    };
    let spec = resolve_role_model_spec(
        "tinker",
        None,
        Some(&preset),
        "session-model",
        Path::new("."),
    );
    assert_eq!(spec.model, "ollama:qwen2.5-coder:7b");

    // Registry holds ONLY the session (mock) provider; config holds NO
    // providers at all, so no preset candidate can resolve.
    let mut preg = ProviderRegistry::new();
    preg.register("mock".to_string(), provider.clone());
    let task_tool = TaskTool::with_full_options(
        reg,
        provider.clone(),
        "session-model",
        &temp_dir,
        None,
        None,
        None,
        None,
        false,
    )
    .with_provider_routing(Config::default(), Arc::new(preg));
    let (child_provider, child_model) = task_tool.resolve_provider_and_model(&spec);
    assert_eq!(child_model, "session-model");
    assert!(Arc::ptr_eq(&child_provider, &provider));
}

#[tokio::test]
async fn test_subagent_chain_handoff_and_resume_by_id() {
    let temp_dir = std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let gate = Arc::new(ApprovalGate::new("tabby"));
    let task_tool = TaskTool::with_full_options(
        reg.clone(),
        provider.clone(),
        "mock-model",
        &temp_dir,
        None,
        None,
        Some(gate),
        None,
        true,
    );

    let ctx = ToolContext {
        session_id: "root_sess".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: temp_dir.clone(),
        is_dry_run: false,
        is_cron: false,
        sandbox: None,
    };

    // Step 1: Delegate to Whisker
    let r1 = task_tool
        .execute(
            serde_json::json!({
                "role": "whisker",
                "brief": "Scout auth handler"
            }),
            &ctx,
        )
        .await
        .unwrap();
    let whisker_id = r1.metadata.unwrap()["subagent_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(whisker_id.starts_with("purr_"));
    assert!(r1.output.contains("user_turns=1"));

    // Step 2: Delegate to Sphinx (verifies high reasoning effort default)
    let r2 = task_tool
        .execute(
            serde_json::json!({
                "role": "sphinx",
                "brief": "Verify root cause in auth.rs"
            }),
            &ctx,
        )
        .await
        .unwrap();
    let sphinx_id = r2.metadata.unwrap()["subagent_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(whisker_id, sphinx_id);
    assert!(r2.output.contains("effort=Some(\"high\")"));

    // Step 3: Resume Whisker by exact continuation ID after simulated restart
    let reg_reloaded = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let task_tool_reloaded = TaskTool::with_options(
        reg_reloaded,
        provider,
        "mock-model",
        &temp_dir,
        None,
        None,
        true,
    );

    let r3 = task_tool_reloaded
        .execute(
            serde_json::json!({
                "role": "whisker",
                "id": whisker_id,
                "brief": "Follow up on second auth file"
            }),
            &ctx,
        )
        .await
        .unwrap();
    let resumed_id = r3.metadata.unwrap()["subagent_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(resumed_id, whisker_id);
    assert!(r3.output.contains("user_turns=2"));

    let res_md = std::fs::read_to_string(
        temp_dir
            .join("subagents")
            .join(&whisker_id)
            .join("result.md"),
    )
    .unwrap();
    assert!(res_md.contains("- **Changed Files:**"));
    assert!(res_md.contains("- **Verified Evidence:**"));
    assert!(res_md.contains("- **Failures / Blockers:**"));
    assert!(res_md.contains("- **Unverified Items:**"));

    assert!(temp_dir
        .join("subagents")
        .join(format!("{}.json", whisker_id))
        .exists());
    assert!(temp_dir.join("workgraph.json").exists());

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_format_validated_result_markdown_rejects_empty_and_normalizes() {
    // Empty summary is rejected
    let err = format_validated_result_markdown("sub_1", "tinker", "completed", "   \n\t  ");
    assert!(err.is_err());
    assert!(matches!(err.unwrap_err(), PurrError::Subagent { .. }));

    // Valid summary with missing sections gets normalized with standard placeholders
    let raw = "Refactored payment gateway handler in src/payment.rs";
    let res = format_validated_result_markdown("sub_1", "tinker", "completed", raw).unwrap();
    assert!(res.contains("# Result Report: sub_1"));
    assert!(res.contains("- **Status:** completed"));
    assert!(res.contains("- **Role:** tinker"));
    assert!(res.contains("- **Changed Files:** (see summary below)"));
    assert!(res.contains("- **Verified Evidence:** (see summary below)"));
    assert!(res.contains("- **Failures / Blockers:** None reported."));
    assert!(res.contains("- **Unverified Items:** None reported."));
    assert!(res.contains("## Summary\nRefactored payment gateway handler in src/payment.rs"));
}

#[test]
fn test_format_validated_result_markdown_dedups_summary_heading() {
    // The child brief asks for a leading `## Summary` heading; the
    // report must contain it exactly once, not twice.
    let raw = "## Summary\nRefactored payment gateway handler in src/payment.rs";
    let res = format_validated_result_markdown("sub_1", "tinker", "completed", raw).unwrap();
    assert_eq!(res.matches("## Summary").count(), 1);
    assert!(res.contains("## Summary\nRefactored payment gateway handler in src/payment.rs"));
    // A summary without the heading still gets exactly one.
    let plain =
        format_validated_result_markdown("sub_2", "tinker", "completed", "Did the thing").unwrap();
    assert_eq!(plain.matches("## Summary").count(), 1);
}

#[tokio::test]
async fn test_task_tool_dry_run_returns_preview_without_side_effects() {
    let temp_dir = std::env::temp_dir().join(format!("purr_dryrun_test_{}", uuid::Uuid::new_v4()));
    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let task_tool = TaskTool::with_options(
        reg.clone(),
        provider,
        "mock-model",
        &temp_dir,
        None,
        None,
        false,
    );

    let dry_ctx = ToolContext {
        session_id: "test_session".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: PathBuf::from("."),
        is_dry_run: true,
        is_cron: false,
        sandbox: None,
    };

    let result = task_tool
        .execute(
            serde_json::json!({
                "role": "tinker",
                "brief": "Implement auth middleware",
                "diff_preview": "--- a/auth.rs\n+++ b/auth.rs\n@@ -1 +1 @@\n+pub fn check() {}"
            }),
            &dry_ctx,
        )
        .await
        .unwrap();

    // 1. Dry-run must return a preview with role and diff/plan
    assert!(result.diff_preview.is_some());
    let preview = result.diff_preview.unwrap();
    assert!(preview.contains("Delegation Plan to Subagent:"));
    assert!(preview.contains("- **Role:** tinker"));
    assert!(preview.contains("- **Target Model:** mock-model"));
    assert!(preview.contains("pub fn check() {}"));

    // 2. Crucial: Registry must have ZERO records and disk must not have been created
    let reg_guard = reg.lock().await;
    let records = reg_guard.list_records();
    assert_eq!(records.len(), 0);
    drop(reg_guard);
    assert!(!temp_dir.join("subagents").exists());

    let _ = std::fs::remove_dir_all(temp_dir);
}

/// Drain only the subagent lifecycle events from a bus receiver —
/// the child loop also publishes its own TurnStarted/TextChunk/
/// TurnFinished to the same bus, which this helper ignores.
fn drain_subagent_events(rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if matches!(
            ev,
            AgentEvent::SubagentSpawned { .. } | AgentEvent::SubagentStatusChanged { .. }
        ) {
            out.push(ev);
        }
    }
    out
}

#[tokio::test]
async fn test_task_tool_publishes_subagent_lifecycle_events() {
    // v0.4 B3: a delegation announces itself on the session bus —
    // Spawned (with role + brief), Running (child loop starts),
    // Done (one-line summary) — so the WebUI Clowder panel can
    // render the subagent live without polling the registry.
    let temp_dir = std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let bus = Arc::new(EventBus::new(256));
    let mut rx = bus.subscribe();
    let task_tool = TaskTool::with_full_options(
        reg.clone(),
        provider.clone(),
        "mock-model",
        &temp_dir,
        None,
        None,
        Some(Arc::new(ApprovalGate::new("tabby"))),
        None,
        true,
    )
    .with_event_bus(bus);

    let ctx = ToolContext {
        session_id: "root_sess".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: temp_dir.clone(),
        is_dry_run: false,
        is_cron: false,
        sandbox: None,
    };
    let r = task_tool
        .execute(
            serde_json::json!({
                "role": "whisker",
                "brief": "Scout auth handler"
            }),
            &ctx,
        )
        .await
        .unwrap();
    let id = r.metadata.unwrap()["subagent_id"]
        .as_str()
        .unwrap()
        .to_string();

    let events = drain_subagent_events(&mut rx);
    assert_eq!(
        events.len(),
        3,
        "expected Spawned + Running + Done, got {events:?}"
    );
    match &events[0] {
        AgentEvent::SubagentSpawned {
            subagent_id,
            parent_id,
            role,
            brief,
        } => {
            assert_eq!(subagent_id, &id);
            assert_eq!(parent_id, "root_sess");
            assert_eq!(role, "whisker");
            assert_eq!(brief, "Scout auth handler");
        }
        other => panic!("first event must be SubagentSpawned, got {other:?}"),
    }
    match &events[1] {
        AgentEvent::SubagentStatusChanged {
            subagent_id,
            status,
            summary,
        } => {
            assert_eq!(subagent_id, &id);
            assert_eq!(*status, SubagentStatus::Running);
            assert!(summary.is_none());
        }
        other => panic!("second event must be Running, got {other:?}"),
    }
    match &events[2] {
        AgentEvent::SubagentStatusChanged {
            subagent_id,
            status,
            summary,
        } => {
            assert_eq!(subagent_id, &id);
            assert_eq!(*status, SubagentStatus::Done);
            let summary = summary.as_deref().unwrap_or("");
            assert!(
                summary.contains("call=1"),
                "Done summary must carry the child result, got {summary:?}"
            );
        }
        other => panic!("third event must be Done, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[tokio::test]
async fn test_task_tool_publishes_subagent_failed_on_shadow_block() {
    // v0.4 B3: a delegation the shadow auditor blocks announces
    // Spawned then Failed — never Running, never Done — so the
    // Clowder panel does not show a blocked subagent as stuck.
    let temp_dir = std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let bus = Arc::new(EventBus::new(256));
    let mut rx = bus.subscribe();
    let task_tool = TaskTool::with_full_options(
        reg.clone(),
        provider.clone(),
        "mock-model",
        &temp_dir,
        None,
        None,
        Some(Arc::new(ApprovalGate::new("tabby"))),
        None,
        true,
    )
    .with_event_bus(bus);

    let ctx = ToolContext {
        session_id: "root_sess".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: temp_dir.clone(),
        is_dry_run: false,
        is_cron: false,
        sandbox: None,
    };
    // "rm -rf" is a shadow trigger marker; the echo provider's
    // verdict text carries no APPROVE, so Tuxeshadow objects and
    // Tabby enforcement fails the delegation closed.
    let err = task_tool
        .execute(
            serde_json::json!({
                "role": "tinker",
                "brief": "run rm -rf /tmp/x to clean up"
            }),
            &ctx,
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Tuxeshadow"),
        "expected a shadow-block error, got {err}"
    );

    let events = drain_subagent_events(&mut rx);
    assert_eq!(events.len(), 2, "expected Spawned + Failed, got {events:?}");
    assert!(matches!(events[0], AgentEvent::SubagentSpawned { .. }));
    match &events[1] {
        AgentEvent::SubagentStatusChanged {
            status, summary, ..
        } => {
            assert_eq!(*status, SubagentStatus::Failed);
            assert!(
                summary.as_deref().unwrap_or("").contains("Tuxeshadow"),
                "Failed summary must name the blocker, got {summary:?}"
            );
        }
        other => panic!("second event must be Failed, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[tokio::test]
async fn test_task_tool_publishes_subagent_failed_on_persistence_failure() {
    // v0.4 B3 fix P2-1 (re-checker, live-reproduced via E2E): a
    // step-7 persistence failure used to propagate with `?` and
    // leave the Clowder panel stuck on "running". Plant a directory
    // named history.json in the handoff dir so save_history fails
    // with EISDIR, and expect Spawned → Running → Failed.
    let temp_dir = std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
    let id = "purr_persistfail";
    // Seed a registry record on disk so get_or_create returns this
    // exact continuation id instead of minting a fresh one.
    let sub_dir = temp_dir.join("subagents");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(
        sub_dir.join(format!("{id}.json")),
        serde_json::json!({
            "id": id,
            "parent_id": "meowstro_root",
            "role": "whisker",
            "status": "running",
            "created_at": "2026-11-13T00:00:00Z",
            "updated_at": "2026-11-13T00:00:00Z",
            "tokens_used": {"input": 0, "output": 0},
            "summary": null,
            "brief": "Scout auth handler"
        })
        .to_string(),
    )
    .unwrap();
    // A non-empty directory: save_history writes history.json
    // directly (std::fs::write), so even an empty directory would
    // fail with EISDIR; the dummy file also defeats any future
    // tmp+rename replacement of the path.
    let poison = sub_dir.join(id).join("history.json");
    std::fs::create_dir_all(&poison).unwrap();
    std::fs::write(poison.join("dummy.txt"), b"poison").unwrap();

    let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
        calls: AtomicUsize::new(0),
    });
    let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
    let bus = Arc::new(EventBus::new(256));
    let mut rx = bus.subscribe();
    let task_tool = TaskTool::with_full_options(
        reg.clone(),
        provider.clone(),
        "mock-model",
        &temp_dir,
        None,
        None,
        Some(Arc::new(ApprovalGate::new("tabby"))),
        None,
        true,
    )
    .with_event_bus(bus);

    let ctx = ToolContext {
        session_id: "root_sess".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: temp_dir.clone(),
        is_dry_run: false,
        is_cron: false,
        sandbox: None,
    };
    let err = task_tool
        .execute(
            serde_json::json!({
                "role": "whisker",
                "id": id,
                "brief": "Scout auth handler"
            }),
            &ctx,
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Is a directory"),
        "expected an EISDIR persistence error, got {err}"
    );

    let events = drain_subagent_events(&mut rx);
    assert_eq!(
        events.len(),
        3,
        "expected Spawned + Running + Failed, got {events:?}"
    );
    assert!(matches!(events[0], AgentEvent::SubagentSpawned { .. }));
    assert!(matches!(
        events[1],
        AgentEvent::SubagentStatusChanged {
            status: SubagentStatus::Running,
            ..
        }
    ));
    match &events[2] {
        AgentEvent::SubagentStatusChanged {
            status, summary, ..
        } => {
            assert_eq!(*status, SubagentStatus::Failed);
            assert!(
                summary.as_deref().unwrap_or("").contains("Is a directory"),
                "Failed summary must carry the IO error, got {summary:?}"
            );
        }
        other => panic!("third event must be Failed, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}
