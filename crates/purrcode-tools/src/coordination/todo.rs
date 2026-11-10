use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

pub struct TodoTool {
    todos: Arc<Mutex<Vec<TodoItem>>>,
}

impl TodoTool {
    pub fn new() -> Self {
        Self {
            todos: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for TodoTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct TodoArgs {
    todos: Vec<TodoItem>,
}

#[async_trait]
impl Tool for TodoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "todo".to_string(),
            description: "Record and update task plan progress checklist. Keep exactly one item in_progress.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string", "description": "Todo description" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed", "cancelled"],
                                    "description": "Task status"
                                }
                            },
                            "required": ["text", "status"]
                        },
                        "description": "Full updated list of todo items"
                    }
                },
                "required": ["todos"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let args: TodoArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "todo".to_string(),
                message: format!("Invalid todo arguments: {e}"),
            })?;

        let mut output = String::new();
        output.push_str("📋 Task Plan Progress:\n");

        for item in &args.todos {
            let badge = match item.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[▶]",
                TodoStatus::Completed => "[✓]",
                TodoStatus::Cancelled => "[-]",
            };
            output.push_str(&format!("  {} {}\n", badge, item.text));
        }

        // Store updated list
        {
            let mut list = self.todos.lock().await;
            *list = args.todos;
        }

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_todo_tool() {
        let tool = TodoTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "meowstro".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            is_dry_run: false,
        };

        let res = tool
            .execute(
                serde_json::json!({
                    "todos": [
                        { "text": "Analyze architecture", "status": "completed" },
                        { "text": "Implement tools", "status": "in_progress" },
                        { "text": "Run tests", "status": "pending" }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res.output.contains("[✓] Analyze architecture"));
        assert!(res.output.contains("[▶] Implement tools"));
        assert!(res.output.contains("[ ] Run tests"));
    }
}
