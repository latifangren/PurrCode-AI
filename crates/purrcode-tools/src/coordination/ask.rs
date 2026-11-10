// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::io::{self, IsTerminal, Write};

use std::sync::Arc;

#[async_trait]
pub trait AskRequester: Send + Sync {
    async fn request_decision(&self, question: &str, options: &[AskOption]) -> Option<String>;
}

pub struct AskTool {
    requester: Option<Arc<dyn AskRequester>>,
}

impl AskTool {
    pub fn new() -> Self {
        Self { requester: None }
    }

    pub fn with_requester(requester: Arc<dyn AskRequester>) -> Self {
        Self {
            requester: Some(requester),
        }
    }
}

impl Default for AskTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct AskArgs {
    question: String,
    options: Vec<AskOption>,
}

#[derive(Deserialize, Clone)]
pub struct AskOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[async_trait]
impl Tool for AskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ask".to_string(),
            description: "Ask user for a decision when requirements or options are ambiguous. Pauses for real human input.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The clear plain-language question to ask" },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Machine identifier for option" },
                                "label": { "type": "string", "description": "Display label for option" },
                                "description": { "type": "string", "description": "Tradeoff description" }
                            },
                            "required": ["id", "label"]
                        },
                        "description": "Mutually exclusive options"
                    }
                },
                "required": ["question", "options"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let args: AskArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "ask".to_string(),
            message: format!("Invalid ask arguments: {e}"),
        })?;

        if args.options.is_empty() {
            return Err(PurrError::Tool {
                tool: "ask".to_string(),
                message: "Options list must not be empty".to_string(),
            });
        }

        // 1. If an explicit AskRequester is provided (e.g. from TUI channel), use it
        if let Some(ref req) = self.requester {
            if let Some(selected_id) = req.request_decision(&args.question, &args.options).await {
                let label = args
                    .options
                    .iter()
                    .find(|o| o.id == selected_id)
                    .map(|o| o.label.as_str())
                    .unwrap_or(&selected_id);
                return Ok(ToolResult {
                    output: format!("Selected decision: {} ({})", selected_id, label),
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "selected_id": selected_id,
                        "selected_label": label,
                        "source": "requester"
                    })),
                });
            }
        }

        // 2. Check if stdin is an interactive terminal
        if io::stdin().is_terminal() {
            println!();
            println!("❓ Decision Required: {}", args.question);
            for (idx, opt) in args.options.iter().enumerate() {
                if let Some(ref desc) = opt.description {
                    println!("  [{}] {} — {}", idx + 1, opt.label, desc);
                } else {
                    println!("  [{}] {}", idx + 1, opt.label);
                }
            }
            print!("Select option (1-{}) or type id: ", args.options.len());
            let _ = io::stdout().flush();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let trimmed = input.trim();
                // Check if user entered numeric index
                if let Ok(num) = trimmed.parse::<usize>() {
                    if num >= 1 && num <= args.options.len() {
                        let selected = &args.options[num - 1];
                        return Ok(ToolResult {
                            output: format!(
                                "Selected decision: {} ({})",
                                selected.id, selected.label
                            ),
                            diff_preview: None,
                            metadata: Some(serde_json::json!({
                                "selected_id": selected.id,
                                "selected_label": selected.label,
                                "source": "user_input"
                            })),
                        });
                    }
                }

                // Check if user typed matching ID
                for opt in &args.options {
                    if opt.id.eq_ignore_ascii_case(trimmed) {
                        return Ok(ToolResult {
                            output: format!("Selected decision: {} ({})", opt.id, opt.label),
                            diff_preview: None,
                            metadata: Some(serde_json::json!({
                                "selected_id": opt.id,
                                "selected_label": opt.label,
                                "source": "user_input"
                            })),
                        });
                    }
                }
            }
        }

        // Non-interactive / headless fallback: return explicit pending status without fabricating a decision
        let mut output = format!("❓ Decision Pending: {}\n", args.question);
        output.push_str("Available options:\n");
        for opt in &args.options {
            if let Some(ref desc) = opt.description {
                output.push_str(&format!("  • [{}] {} — {}\n", opt.id, opt.label, desc));
            } else {
                output.push_str(&format!("  • [{}] {}\n", opt.id, opt.label));
            }
        }
        output.push_str(
            "\n[Decision Pending: User input required. No option was automatically selected.]",
        );

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "status": "pending_user_decision",
                "options_count": args.options.len()
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ask_non_interactive() {
        let tool = AskTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "meowstro".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            is_dry_run: false,
        };

        let res = tool
            .execute(
                serde_json::json!({
                    "question": "Choose database type",
                    "options": [
                        { "id": "sqlite", "label": "SQLite", "description": "Local embedded" },
                        { "id": "postgres", "label": "PostgreSQL", "description": "Remote database" }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res
            .output
            .contains("Decision Pending: Choose database type"));
        assert!(res.output.contains("[sqlite] SQLite"));
        assert!(res.output.contains("No option was automatically selected"));
    }

    #[tokio::test]
    async fn test_ask_empty_options() {
        let tool = AskTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "meowstro".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            is_dry_run: false,
        };

        let res = tool
            .execute(
                serde_json::json!({
                    "question": "Invalid query",
                    "options": []
                }),
                &ctx,
            )
            .await;

        assert!(res.is_err());
    }
}
