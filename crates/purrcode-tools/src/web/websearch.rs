// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;

pub struct WebsearchTool {
    client: reqwest::Client,
    endpoint_base: String,
}

impl WebsearchTool {
    pub fn new() -> Self {
        Self::with_endpoint("https://html.duckduckgo.com/html/")
    }

    pub fn with_endpoint(endpoint_base: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("PurrCode-AI/0.1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            endpoint_base: endpoint_base.into(),
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
            description:
                "Search the web for programming questions, library documentation, and error solutions"
                    .to_string(),
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

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let args: WebsearchArgs =
            serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
                tool: "websearch".to_string(),
                message: format!("Invalid websearch arguments: {e}"),
            })?;

        if args.query.trim().is_empty() {
            return Err(PurrError::Tool {
                tool: "websearch".to_string(),
                message: "Search query must not be empty".to_string(),
            });
        }

        let search_url = format!("{}?q={}", self.endpoint_base, encode_query(&args.query));

        let parsed_url = reqwest::Url::parse(&search_url).map_err(|e| PurrError::Tool {
            tool: "websearch".to_string(),
            message: format!("Malformed search URL: {e}"),
        })?;

        super::webfetch::validate_destination_for_tool(&parsed_url, "websearch")?;
        super::webfetch::validate_dns_lookup_async_for_tool(&parsed_url, "websearch").await?;

        let resp = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| PurrError::Tool {
                tool: "websearch".to_string(),
                message: format!("Web search HTTP request failed: {e}"),
            })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(PurrError::Tool {
                tool: "websearch".to_string(),
                message: format!("Web search endpoint returned HTTP {status}"),
            });
        }

        let html = resp.text().await.map_err(|e| PurrError::Tool {
            tool: "websearch".to_string(),
            message: format!("Failed to read web search response body: {e}"),
        })?;

        let text = extract_search_snippets(&html);
        let output = if text.is_empty() {
            format!(
                "Search completed for query '{}', but no direct snippets found.",
                args.query
            )
        } else {
            text
        };

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "query": args.query,
                "status": status.as_u16()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websearch_propagates_network_error() {
        // Point to an unreachable port on loopback to verify network failure returns Err, not Ok
        let tool = WebsearchTool::with_endpoint("http://127.0.0.1:9/search");
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "whisker".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            is_dry_run: false,
        };
        let res = tool
            .execute(serde_json::json!({ "query": "rust tokio" }), &ctx)
            .await;
        assert!(res.is_err());
    }

    #[test]
    fn test_extract_snippets_and_encode() {
        assert_eq!(encode_query("rust async/await"), "rust+async%2Fawait");
        let html = r#"<div><a class="result__snippet" href="x">First <b>result</b> text</a></div>"#;
        let out = extract_search_snippets(html);
        assert!(out.contains("1. First result text"));
    }
}
