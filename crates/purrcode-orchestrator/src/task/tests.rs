// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use purrcode_api::provider::{CompletionRequest, MessageRole, StreamChunk};
use purrcode_config::{AgentModelSpec, ModelTarget};
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
