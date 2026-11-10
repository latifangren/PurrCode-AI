use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use purrcode_core::AgentLoop;
use purrcode_tools::ToolRegistry;
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
}

impl TaskTool {
    pub fn new(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            registry,
            provider,
            model: model.into(),
            session_dir: session_dir.into(),
        }
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
            description: "Delegate a task to a specialized subagent (tinker, whisker, sphinx). Preserves context via continuation ID.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "enum": ["tinker", "whisker", "sphinx", "calico"],
                        "description": "Subagent role: 'tinker' (fixer/implementer with full write tools), 'whisker' (explorer/search read-only)"
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
        let args: TaskArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "task".to_string(),
                message: format!("Invalid task arguments: {e}"),
            })?;

        // 1. Resolve or allocate stable continuation ID
        let subagent_id = {
            let mut reg = self.registry.lock().await;
            reg.get_or_create(args.id.clone(), &args.role, &args.brief)
        };

        // 2. Prepare handoff directory: sessions/<session_id>/subagents/<id>/
        let subagent_dir = self.session_dir.join("subagents").join(&subagent_id);
        tokio::fs::create_dir_all(&subagent_dir).await.map_err(|e| PurrError::Subagent {
            id: subagent_id.clone(),
            message: format!("Failed to create subagent handoff directory: {e}"),
        })?;

        let brief_file = subagent_dir.join("brief.md");
        let result_file = subagent_dir.join("result.md");

        // Append to brief.md
        let brief_entry = format!(
            "\n## Task Delegation Turn ({})\nRole: {}\nBrief: {}\n",
            chrono::Utc::now().to_rfc3339(),
            args.role,
            args.brief
        );
        let mut existing_brief = tokio::fs::read_to_string(&brief_file).await.unwrap_or_default();
        existing_brief.push_str(&brief_entry);
        let _ = tokio::fs::write(&brief_file, existing_brief.as_bytes()).await;

        // 3. Assemble child toolset according to role
        let child_tools = match args.role.as_str() {
            "whisker" | "sphinx" => {
                // Read-only toolset
                let mut reg = ToolRegistry::new();
                reg.register(Arc::new(purrcode_tools::fs::ReadTool)).unwrap();
                reg.register(Arc::new(purrcode_tools::search::GrepTool)).unwrap();
                reg.register(Arc::new(purrcode_tools::search::FindTool)).unwrap();
                reg
            }
            _ => {
                // Tinker / Calico get full implementation tools
                ToolRegistry::with_defaults()
            }
        };

        let child_context = ToolContext {
            session_id: format!("{}:{}", ctx.session_id, subagent_id),
            agent_role: args.role.clone(),
            workspace_root: ctx.workspace_root.clone(),
            is_dry_run: ctx.is_dry_run,
        };

        let child_loop = AgentLoop::new(self.provider.clone(), &self.model, Arc::new(child_tools))
            .with_max_steps(15);

        let system_prompt = format!(
            "You are {}, a specialized subagent of PurrCode AI.\nYour ID is '{}'.\nYour task is:\n{}\nPerform the work thoroughly using your tools and write a structured summary of changes.",
            args.role, subagent_id, args.brief
        );

        // 4. Run child loop
        let child_result = child_loop
            .run_turn(&args.brief, Some(&system_prompt), &child_context, |_chunk, _reasoning| {})
            .await?;

        // 5. Write result.md
        let result_entry = format!(
            "# Result for {}\nStatus: completed\nTimestamp: {}\n\nSummary:\n{}\n",
            subagent_id,
            chrono::Utc::now().to_rfc3339(),
            child_result
        );
        let _ = tokio::fs::write(&result_file, result_entry.as_bytes()).await;

        // Update registry
        {
            let mut reg = self.registry.lock().await;
            let _ = reg.record_result(&subagent_id, &child_result);
        }

        // Return structured summary to parent (Meowstro)
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
