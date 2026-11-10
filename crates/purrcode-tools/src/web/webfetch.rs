use async_trait::async_trait;
use futures_util::StreamExt;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use reqwest::Url;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

const MAX_STREAM_BYTES: usize = 1_048_576; // 1 MB safety ceiling to prevent OOM
const MAX_OUTPUT_CHARS: usize = 25_000;

pub struct WebfetchTool {
    client: reqwest::Client,
}

impl WebfetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::limited(5))
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
            description: "Fetch web content or API documentation via HTTP/HTTPS GET with SSRF protection".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Public HTTP or HTTPS URL to fetch" }
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

        let parsed_url = Url::parse(&args.url).map_err(|e| PurrError::Tool {
            tool: "webfetch".to_string(),
            message: format!("Malformed URL '{}': {e}", args.url),
        })?;

        validate_destination(&parsed_url)?;

        let resp = self.client.get(parsed_url.clone()).send().await.map_err(|e| {
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

        // Bounded streaming: read at most MAX_STREAM_BYTES chunk-by-chunk to prevent memory exhaustion
        let mut byte_stream = resp.bytes_stream();
        let mut buffer = Vec::new();
        let mut was_truncated = false;

        while let Some(chunk_res) = byte_stream.next().await {
            let chunk = chunk_res.map_err(|e| PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Error reading response stream: {e}"),
            })?;

            if buffer.len() + chunk.len() > MAX_STREAM_BYTES {
                let remaining = MAX_STREAM_BYTES.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..remaining]);
                was_truncated = true;
                break;
            }

            buffer.extend_from_slice(&chunk);
        }

        let raw_body = String::from_utf8_lossy(&buffer);
        let clean_text = strip_html_tags(&raw_body);

        let final_text = if clean_text.len() > MAX_OUTPUT_CHARS {
            format!("{}\n\n(output truncated at {} characters)", &clean_text[..MAX_OUTPUT_CHARS], MAX_OUTPUT_CHARS)
        } else if was_truncated {
            format!("{}\n\n(stream truncated at 1 MB limit)", clean_text)
        } else {
            clean_text
        };

        Ok(ToolResult {
            output: final_text,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "url": args.url,
                "status": status.as_u16(),
                "downloaded_bytes": buffer.len(),
                "truncated": was_truncated
            })),
        })
    }
}

pub fn validate_destination(url: &Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Blocked scheme '{}': only http and https are allowed.", other),
            });
        }
    }

    let host = match url.host_str() {
        Some(h) => h.to_lowercase(),
        None => {
            return Err(PurrError::Tool {
                tool: "webfetch".to_string(),
                message: "Missing host in URL".to_string(),
            });
        }
    };

    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err(PurrError::Tool {
            tool: "webfetch".to_string(),
            message: format!("Access to local domain '{}' is blocked for security.", host),
        });
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_or_restricted_ip(ip) {
            return Err(PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Access to private/loopback/metadata IP '{}' is blocked for security.", ip),
            });
        }
    }

    Ok(())
}

fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_loopback() // 127.0.0.0/8
                || ipv4.is_private() // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || ipv4.is_link_local() // 169.254.0.0/16 (includes cloud metadata 169.254.169.254)
                || ipv4.is_broadcast()
                || ipv4 == Ipv4Addr::UNSPECIFIED // 0.0.0.0
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() // ::1
                || ipv6 == Ipv6Addr::UNSPECIFIED // ::
                || is_unique_local_ipv6(ipv6) // fc00::/7
                || is_unicast_link_local_ipv6(ipv6) // fe80::/10
        }
    }
}

fn is_unique_local_ipv6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_unicast_link_local_ipv6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssrf_blocking() {
        // Loopback IPv4
        assert!(validate_destination(&Url::parse("http://127.0.0.1/api").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://127.0.0.2:8080").unwrap()).is_err());

        // Private LAN IPv4
        assert!(validate_destination(&Url::parse("http://192.168.1.1/admin").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://10.0.0.1/secret").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://172.16.0.5").unwrap()).is_err());

        // Cloud Metadata IP
        assert!(validate_destination(&Url::parse("http://169.254.169.254/latest/meta-data").unwrap()).is_err());

        // Localhost domain
        assert!(validate_destination(&Url::parse("http://localhost:3000").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://myhost.local/").unwrap()).is_err());

        // Block non-HTTP schemes
        assert!(validate_destination(&Url::parse("ftp://example.com/file").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("file:///etc/passwd").unwrap()).is_err());

        // Valid public URLs
        assert!(validate_destination(&Url::parse("https://raw.githubusercontent.com/test").unwrap()).is_ok());
        assert!(validate_destination(&Url::parse("https://crates.io/api/v1/crates").unwrap()).is_ok());
    }
}
