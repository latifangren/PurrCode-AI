use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::SkillManager;

pub struct ReadSkillTool {
    manager: Arc<Mutex<SkillManager>>,
}

impl ReadSkillTool {
    pub fn new(manager: Arc<Mutex<SkillManager>>) -> Self {
        Self { manager }
    }
}

#[derive(Deserialize)]
struct ReadSkillArgs {
    name: String,
}

#[async_trait]
impl Tool for ReadSkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_skill".to_string(),
            description: "Load the full instructions and guidelines for one available skill by name".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill name to load" }
                },
                "required": ["name"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let args: ReadSkillArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Invalid read_skill arguments: {e}"),
            })?;

        let mgr = self.manager.lock().await;
        let content = mgr.read_skill_body(&args.name)?;

        Ok(ToolResult {
            output: content,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "skill_name": args.name
            })),
        })
    }
}
