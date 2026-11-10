use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;

pub struct AskTool;

#[derive(Deserialize)]
struct AskArgs {
    question: String,
    options: Vec<AskOption>,
}

#[derive(Deserialize, Clone)]
struct AskOption {
    id: String,
    label: String,
    description: Option<String>,
}

#[async_trait]
impl Tool for AskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ask".to_string(),
            description: "Ask user for a decision when requirements or options are ambiguous".to_string(),
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

    async fn execute(&self, arguments: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let args: AskArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "ask".to_string(),
                message: format!("Invalid ask arguments: {e}"),
            })?;

        if args.options.is_empty() {
            return Err(PurrError::Tool {
                tool: "ask".to_string(),
                message: "Options list must not be empty".to_string(),
            });
        }

        // In non-interactive one-shot CLI mode, select recommended/first option
        let selected = &args.options[0];
        let mut output = format!("❓ Question asked: {}\n", args.question);
        output.push_str("Options presented:\n");
        for opt in &args.options {
            if let Some(ref desc) = opt.description {
                output.push_str(&format!("  • [{}] {} — {}\n", opt.id, opt.label, desc));
            } else {
                output.push_str(&format!("  • [{}] {}\n", opt.id, opt.label));
            }
        }
        output.push_str(&format!("\nSelected decision: {} ({})\n", selected.id, selected.label));

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "selected_id": selected.id,
                "selected_label": selected.label
            })),
        })
    }
}
