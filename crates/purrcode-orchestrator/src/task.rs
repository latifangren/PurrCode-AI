// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use purrcode_core::{AgentLoop, ApprovalRequester};
use purrcode_tools::{AskRequester, ToolRegistry, ToolRegistryExt};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::SubagentRegistry;

pub struct TaskTool {
    registry: Arc<Mutex<SubagentRegistry>>,
    provider: Arc<dyn ModelProvider>,
    model: String,
    session_dir: PathBuf,
    approval_requester: Option<Arc<dyn ApprovalRequester>>,
    ask_requester: Option<Arc<dyn AskRequester>>,
    yolo: bool,
}

impl TaskTool {
    pub fn new(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::with_options(registry, provider, model, session_dir, None, None, false)
    }

    pub fn with_options(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        yolo: bool,
    ) -> Self {
        Self {
            registry,
            provider,
            model: model.into(),
            session_dir: session_dir.into(),
            approval_requester,
            ask_requester,
            yolo,
        }
    }

    fn build_role_tools(&self, role: &str) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        match role {
            "whisker" => {
                // Explorer: read, grep, glob, find, webfetch, websearch
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
                let _ = reg.register(Arc::new(purrcode_tools::web::WebfetchTool::new()));
                let _ = reg.register(Arc::new(purrcode_tools::web::WebsearchTool::new()));
            }
            "sphinx" | "tuxeshadow" => {
                // Oracle / Shadow Auditor: read, grep, glob, find, ask
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
                let ask_tool = match self.ask_requester.clone() {
                    Some(req) => purrcode_tools::coordination::AskTool::with_requester(req),
                    None => purrcode_tools::coordination::AskTool::new(),
                };
                let _ = reg.register(Arc::new(ask_tool));
            }
            "calico" => {
                // Designer: read, write, edit, grep, glob, find (NO bash!)
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::fs::WriteTool));
                let _ = reg.register(Arc::new(purrcode_tools::fs::EditTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
            }
            "clowder" => {
                // Council: read, grep, glob, find
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
            }
            _ => {
                // Tinker (Fixer): all v0.1 implementation tools (depth max 1 -> no task tool)
                reg = ToolRegistry::with_defaults_and_ask(self.ask_requester.clone());
            }
        }
        reg.freeze();
        reg
    }
}

#[derive(Deserialize)]
struct TaskArgs {
    role: String,
    id: Option<String>,
    brief: String,
}

#[async_trait]
impl Tool for TaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "task".to_string(),
            description: "Delegate a task to a specialized subagent (tinker, whisker, sphinx, calico, tuxeshadow, clowder). Preserves context via continuation ID.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "enum": ["tinker", "whisker", "sphinx", "calico", "tuxeshadow", "clowder"],
                        "description": "Subagent role: 'tinker' (fixer), 'whisker' (explorer), 'sphinx' (oracle), 'calico' (designer), 'tuxeshadow' (shadow auditor), 'clowder' (council)"
                    },
                    "id": {
                        "type": "string",
                        "description": "Optional continuation ID (purr_<id>) to resume an existing subagent without losing previous context"
                    },
                    "brief": {
                        "type": "string",
                        "description": "Clear instructions, target files/lines, constraints, and expected deliverables"
                    }
                },
                "required": ["role", "brief"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: TaskArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "task".to_string(),
            message: format!("Invalid task arguments: {e}"),
        })?;

        // Enforce depth max 1: subagents cannot spawn further subagents
        if ctx.agent_role != "meowstro" && !ctx.agent_role.is_empty() {
            return Err(PurrError::Subagent {
                id: ctx.session_id.clone(),
                message: "Subagent nesting is forbidden in v0.1 (max depth = 1)".to_string(),
            });
        }

        // 1. Resolve or allocate stable continuation ID + load prior conversation history
        let (subagent_id, mut history) = {
            let mut reg = self.registry.lock().await;
            let id = reg.get_or_create(args.id.clone(), &args.role, &args.brief);
            let hist = reg.take_history(&id);
            (id, hist)
        };

        // 2. Prepare handoff directory: sessions/<session_id>/subagents/<id>/
        let subagent_dir = self.session_dir.join("subagents").join(&subagent_id);
        tokio::fs::create_dir_all(&subagent_dir)
            .await
            .map_err(|e| PurrError::Subagent {
                id: subagent_id.clone(),
                message: format!("Failed to create subagent handoff directory: {e}"),
            })?;

        let brief_file = subagent_dir.join("brief.md");
        let result_file = subagent_dir.join("result.md");

        // Write or append structured brief.md
        let mut existing_brief = tokio::fs::read_to_string(&brief_file)
            .await
            .unwrap_or_default();
        if existing_brief.is_empty() {
            existing_brief.push_str(&format!(
                "# Delegation Brief: {}\n- **Subagent ID:** {}\n- **Role:** {}\n- **Objective:** {}\n",
                args.role, subagent_id, args.role, args.brief
            ));
        } else {
            existing_brief.push_str(&format!(
                "\n## Follow-Up Turn ({})\n- **Brief:** {}\n",
                chrono::Utc::now().to_rfc3339(),
                args.brief
            ));
        }
        let _ = tokio::fs::write(&brief_file, existing_brief.as_bytes()).await;

        // 3. Assemble child toolset according to role
        let child_tools = Arc::new(self.build_role_tools(&args.role));

        let child_context = ToolContext {
            session_id: format!("{}:{}", ctx.session_id, subagent_id),
            agent_role: args.role.clone(),
            workspace_root: ctx.workspace_root.clone(),
            is_dry_run: ctx.is_dry_run,
        };

        let mut child_loop = AgentLoop::new(self.provider.clone(), &self.model, child_tools)
            .with_max_steps(15)
            .with_yolo(self.yolo);
        if let Some(ref req) = self.approval_requester {
            child_loop = child_loop.with_approval_requester(req.clone());
        }

        let system_prompt = format!(
            "You are {}, a specialized subagent of PurrCode AI.\nYour continuation ID is '{}'.\nPerform your assigned task thoroughly using your allowed tools and return a concise structured summary (changed files:line, verified evidence, blockers, unverified items).",
            args.role, subagent_id
        );

        // 4. Run child loop (Clowder runs 2 council seats and merges verdicts)
        let child_result = if args.role == "clowder" {
            let mut h1 = history.clone();
            let mut h2 = history.clone();
            let seat1_prompt = format!(
                "[Council Seat 1 — Architecture & Safety Lens]\n{}",
                args.brief
            );
            let seat2_prompt = format!(
                "[Council Seat 2 — Pragmatism & Edge-Case Lens]\n{}",
                args.brief
            );
            let (r1, r2) = tokio::join!(
                child_loop.run_turn_with_history(
                    &mut h1,
                    &seat1_prompt,
                    Some(&system_prompt),
                    &child_context,
                    None
                ),
                child_loop.run_turn_with_history(
                    &mut h2,
                    &seat2_prompt,
                    Some(&system_prompt),
                    &child_context,
                    None
                )
            );
            let v1 = r1.unwrap_or_else(|e| format!("[Seat 1 failed: {e}]"));
            let v2 = r2.unwrap_or_else(|e| format!("[Seat 2 failed: {e}]"));
            history = h1;
            format!("### Council Seat 1\n{}\n\n### Council Seat 2\n{}", v1, v2)
        } else {
            child_loop
                .run_turn_with_history(
                    &mut history,
                    &args.brief,
                    Some(&system_prompt),
                    &child_context,
                    None,
                )
                .await?
        };

        // 5. Write result.md
        let result_entry = format!(
            "# Result Report: {}\n- **Status:** completed\n- **Role:** {}\n- **Timestamp:** {}\n\n## Summary\n{}\n",
            subagent_id,
            args.role,
            chrono::Utc::now().to_rfc3339(),
            child_result
        );
        let _ = tokio::fs::write(&result_file, result_entry.as_bytes()).await;

        // 6. Save updated history & registry record to memory and disk
        {
            let mut reg = self.registry.lock().await;
            reg.save_history(&subagent_id, history);
            let _ = reg.record_result(&subagent_id, &child_result);
        }

        let summary = format!(
            "Subagent '{}' (role: {}) completed its task.\nContinuation ID: {}\nHandoff result saved to: {}\n\nSummary Report:\n{}",
            subagent_id,
            args.role,
            subagent_id,
            result_file.display(),
            child_result
        );

        Ok(ToolResult {
            output: summary,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "subagent_id": subagent_id,
                "role": args.role,
                "status": "completed"
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::{CompletionRequest, MessageRole, StreamChunk};
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
                delta: format!("call={} user_turns={}", n, user_turns),
                reasoning_delta: None,
                tool_call_delta: None,
                is_finished: true,
                usage: None,
            });
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn test_subagent_chain_handoff_and_resume_by_id() {
        let temp_dir =
            std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
        let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
            calls: AtomicUsize::new(0),
        });
        let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
        let task_tool = TaskTool::with_options(
            reg.clone(),
            provider.clone(),
            "mock-model",
            &temp_dir,
            None,
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

        // Step 2: Delegate to Sphinx
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
        // Must have preserved prior conversation turn (user_turns=2)
        assert!(r3.output.contains("user_turns=2"));

        // Verify disk artifacts exist: brief.md, result.md, <id>.json, workgraph.json
        assert!(temp_dir
            .join("subagents")
            .join(&whisker_id)
            .join("brief.md")
            .exists());
        assert!(temp_dir
            .join("subagents")
            .join(&whisker_id)
            .join("result.md")
            .exists());
        assert!(temp_dir
            .join("subagents")
            .join(format!("{}.json", whisker_id))
            .exists());
        assert!(temp_dir.join("workgraph.json").exists());

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
