use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;

pub struct WebsearchTool {
    client: reqwest::Client,
}

impl WebsearchTool {
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

impl Default for WebsearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct WebsearchArgs {
    query: String,
}

#[async_trait]
impl Tool for WebsearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "websearch".to_string(),
            description: "Search the web for programming questions, library documentation, and error solutions".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query terms" }
                },
                "required": ["query"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let args: WebsearchArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "websearch".to_string(),
                message: format!("Invalid websearch arguments: {e}"),
            })?;

        // Query duckduckgo html endpoint
        let search_url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            encode_query(&args.query)
        );

        let resp_res = self.client.get(&search_url).send().await;

        let output = match resp_res {
            Ok(resp) => {
                if let Ok(html) = resp.text().await {
                    let text = extract_search_snippets(&html);
                    if text.is_empty() {
                        format!("Search completed for query '{}', but no direct snippets found.", args.query)
                    } else {
                        text
                    }
                } else {
                    format!("Failed to parse search response for query '{}'.", args.query)
                }
            }
            Err(e) => {
                format!("Web search request failed: {e}. Check network connection or use webfetch directly.")
            }
        };

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "query": args.query
            })),
        })
    }
}

fn extract_search_snippets(html: &str) -> String {
    let mut snippets = Vec::new();
    let mut count = 0;

    for part in html.split("<a class=\"result__snippet\"") {
        if count >= 5 {
            break;
        }
        if let Some(start_idx) = part.find('>') {
            if let Some(end_idx) = part.find("</a>") {
                let snippet_raw = &part[start_idx + 1..end_idx];
                let clean = strip_tags(snippet_raw);
                if !clean.trim().is_empty() {
                    count += 1;
                    snippets.push(format!("{}. {}", count, clean.trim()));
                }
            }
        }
    }

    if snippets.is_empty() {
        String::new()
    } else {
        format!("Search results:\n{}", snippets.join("\n\n"))
    }
}

fn encode_query(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            encoded.push(b as char);
        } else if b == b' ' {
            encoded.push('+');
        } else {
            encoded.push_str(&format!("%{:02X}", b));
        }
    }
    encoded
}

fn strip_tags(input: &str) -> String {
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
