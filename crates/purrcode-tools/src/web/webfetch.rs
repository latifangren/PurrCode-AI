use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;

pub struct WebfetchTool {
    client: reqwest::Client,
}

impl WebfetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("PurrCode-AI/0.1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl Default for WebfetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct WebfetchArgs {
    url: String,
}

#[async_trait]
impl Tool for WebfetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "webfetch".to_string(),
            description: "Fetch web content or API documentation via HTTP/HTTPS GET".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "HTTP or HTTPS URL to fetch" }
                },
                "required": ["url"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let args: WebfetchArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Invalid webfetch arguments: {e}"),
            })?;

        if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
            return Err(PurrError::Tool {
                tool: "webfetch".to_string(),
                message: "URL must begin with http:// or https://".to_string(),
            });
        }

        let resp = self.client.get(&args.url).send().await.map_err(|e| {
            PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("HTTP request to '{}' failed: {e}", args.url),
            }
        })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Server returned error HTTP {status}"),
            });
        }

        let raw_body = resp.text().await.map_err(|e| PurrError::Tool {
            tool: "webfetch".to_string(),
            message: format!("Failed to read response body: {e}"),
        })?;

        // Simple tag stripping to reduce token usage
        let clean_text = strip_html_tags(&raw_body);
        let max_len = 25_000;
        let truncated = if clean_text.len() > max_len {
            format!("{}\n\n(output truncated at {} characters)", &clean_text[..max_len], max_len)
        } else {
            clean_text
        };

        Ok(ToolResult {
            output: truncated,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "url": args.url,
                "status": status.as_u16(),
                "content_length": raw_body.len()
            })),
        })
    }
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    out
}
