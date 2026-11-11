// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use futures_util::StreamExt;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use reqwest::Url;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const MAX_STREAM_BYTES: usize = 1_048_576; // 1 MB safety ceiling to prevent OOM
const MAX_OUTPUT_CHARS: usize = 25_000;
/// Maximum redirect hops followed manually (see `get_pinned`).
const MAX_REDIRECT_HOPS: usize = 5;

/// HTTP(S) fetch with DNS-rebinding (TOCTOU) protection.
///
/// The tool holds no client: every request builds a fresh client whose DNS
/// for the target host is pinned to the IP addresses that were just
/// resolved *and* validated (`resolve_validated_addrs` +
/// `reqwest::ClientBuilder::resolve_to_addrs`). The connect therefore uses
/// the validated IPs, never a second lookup that attacker-controlled DNS
/// could answer differently between the check and the connect (pattern
/// from zeroclaw's `web_fetch.rs`: resolve → validate → pin).
pub struct WebfetchTool;

impl WebfetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebfetchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve a URL's host once and validate every resolved IP is public.
/// IP-literal hosts are checked directly (no DNS). DNS failure is
/// **fail-closed**: the fetch is refused instead of proceeding unverified.
pub async fn resolve_validated_addrs(url: &Url, tool_name: &str) -> Result<Vec<SocketAddr>> {
    let host = url.host_str().unwrap_or("").to_lowercase();
    let clean_host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url.port_or_known_default().unwrap_or(80);
    if let Ok(ip) = clean_host.parse::<IpAddr>() {
        if is_private_or_restricted_ip(ip) {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!(
                    "Access to private/loopback/metadata IP '{ip}' is blocked for security."
                ),
            });
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((clean_host, port))
        .await
        .map_err(|e| PurrError::Tool {
            tool: tool_name.to_string(),
            message: format!(
                "DNS lookup for host '{clean_host}' failed ({e}); refusing fetch (fail-closed)."
            ),
        })?
        .collect();
    validate_resolved_ips_for_tool(clean_host, addrs.iter().map(|a| a.ip()), tool_name)?;
    if addrs.is_empty() {
        return Err(PurrError::Tool {
            tool: tool_name.to_string(),
            message: format!("DNS lookup for host '{clean_host}' returned no addresses."),
        });
    }
    Ok(addrs)
}

/// Build a client whose DNS for `host` is pinned to the validated
/// addresses. Redirects are disabled: the caller follows them manually
/// (see `get_pinned`), re-validating and re-pinning every hop.
/// Build a client whose DNS for `host` is pinned to the validated
/// addresses. Redirects are disabled: the caller follows them manually
/// (see `get_pinned`), re-validating and re-pinning every hop.
///
/// Fail-closed: a builder failure is an error, never a fallback to a plain
/// client — a plain client would connect unpinned (and follow redirects),
/// voiding the pinning guarantee this function exists to provide. With the
/// constant inputs used here the builder practically cannot fail.
fn build_pinned_client(host: &str, addrs: &[SocketAddr]) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("PurrCode-AI/0.1.0")
        .resolve_to_addrs(host, addrs)
        .build()
        .map_err(|e| PurrError::Tool {
            tool: "webfetch".to_string(),
            message: format!("Failed to build pinned HTTP client for '{host}': {e}"),
        })
}

/// Resolve a redirect `Location` against the current URL (absolute or
/// relative). Pure helper, unit-tested.
fn resolve_redirect(current: &Url, location: &str, tool_name: &str) -> Result<Url> {
    current.join(location).map_err(|e| PurrError::Tool {
        tool: tool_name.to_string(),
        message: format!("Invalid redirect Location '{location}': {e}"),
    })
}

/// Join a redirect `Location` against the current URL and validate the hop
/// target (scheme, host, IP literal) BEFORE following it. The next loop
/// iteration re-resolves and re-pins the target's DNS, so a redirect to a
/// private/loopback host is rejected here and never connected to.
fn next_hop_url(current: &Url, location: &str, tool_name: &str) -> Result<Url> {
    let next = resolve_redirect(current, location, tool_name)?;
    validate_destination_for_tool(&next, tool_name)?;
    Ok(next)
}

/// GET a URL with per-hop DNS pinning: resolve → validate → pin → request,
/// following redirects manually (max `MAX_REDIRECT_HOPS`), re-validating
/// and re-pinning every hop. Returns the final non-redirect response.
pub async fn get_pinned(url: &Url, tool_name: &str) -> Result<reqwest::Response> {
    let mut current = url.clone();
    for hop in 0..=MAX_REDIRECT_HOPS {
        validate_destination_for_tool(&current, tool_name)?;
        let addrs = resolve_validated_addrs(&current, tool_name).await?;
        let host = current.host_str().unwrap_or("").to_lowercase();
        let client = build_pinned_client(&host, &addrs)?;
        let resp = client
            .get(current.clone())
            .send()
            .await
            .map_err(|e| PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!("HTTP request to '{current}' failed: {e}"),
            })?;
        let status = resp.status();
        if !status.is_redirection() {
            return Ok(resp);
        }
        if hop == MAX_REDIRECT_HOPS {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!(
                    "Too many redirects (>{MAX_REDIRECT_HOPS}) while fetching '{url}'."
                ),
            });
        }
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!("Redirect response from '{current}' has no Location header."),
            })?
            .to_string();
        current = next_hop_url(&current, &location, tool_name)?;
    }
    unreachable!("redirect loop is bounded by MAX_REDIRECT_HOPS")
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
            description:
                "Fetch web content or API documentation via HTTP/HTTPS GET with SSRF protection"
                    .to_string(),
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

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let args: WebfetchArgs =
            serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
                tool: "webfetch".to_string(),
                message: format!("Invalid webfetch arguments: {e}"),
            })?;

        let parsed_url = Url::parse(&args.url).map_err(|e| PurrError::Tool {
            tool: "webfetch".to_string(),
            message: format!("Malformed URL '{}': {e}", args.url),
        })?;

        // Resolve → validate → pin → request, with per-hop re-pinning on
        // redirects. This replaces the old validate-then-send sequence
        // whose separate DNS lookups left the TOCTOU (DNS-rebinding) gap.
        let resp = get_pinned(&parsed_url, "webfetch").await?;

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
            let truncated_slice = safe_truncate_str(&clean_text, MAX_OUTPUT_CHARS);
            format!(
                "{}\n\n(output truncated at {} characters)",
                truncated_slice, MAX_OUTPUT_CHARS
            )
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
    validate_destination_for_tool(url, "webfetch")
}

pub fn validate_destination_for_tool(url: &Url, tool_name: &str) -> Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!(
                    "Blocked scheme '{}': only http and https are allowed.",
                    other
                ),
            });
        }
    }

    let host = match url.host_str() {
        Some(h) => h.to_lowercase(),
        None => {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: "Missing host in URL".to_string(),
            });
        }
    };

    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err(PurrError::Tool {
            tool: tool_name.to_string(),
            message: format!("Access to local domain '{}' is blocked for security.", host),
        });
    }

    let clean_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = clean_host.parse::<IpAddr>() {
        if is_private_or_restricted_ip(ip) {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!(
                    "Access to private/loopback/metadata IP '{}' is blocked for security.",
                    ip
                ),
            });
        }
        return Ok(());
    }

    // Hostname syntactic validation passed. Asynchronous DNS-rebinding verification is performed
    // by `validate_dns_lookup_async_for_tool` before the socket connection is made, and on every
    // redirect hop.
    Ok(())
}

pub async fn validate_dns_lookup_async(url: &Url) -> Result<()> {
    validate_dns_lookup_async_for_tool(url, "webfetch").await
}

pub async fn validate_dns_lookup_async_for_tool(url: &Url, tool_name: &str) -> Result<()> {
    // Fail-closed: resolve once, validate every resolved IP, and refuse the
    // fetch on DNS failure. The returned addresses are the ones a pinned
    // client must connect to; callers that only need the check discard them.
    let _ = resolve_validated_addrs(url, tool_name).await?;
    Ok(())
}

pub fn validate_resolved_ips(host: &str, ips: impl IntoIterator<Item = IpAddr>) -> Result<()> {
    validate_resolved_ips_for_tool(host, ips, "webfetch")
}

pub fn validate_resolved_ips_for_tool(
    host: &str,
    ips: impl IntoIterator<Item = IpAddr>,
    tool_name: &str,
) -> Result<()> {
    for ip in ips {
        if is_private_or_restricted_ip(ip) {
            return Err(PurrError::Tool {
                tool: tool_name.to_string(),
                message: format!(
                    "DNS resolution for host '{}' returned restricted/private IP '{}' (DNS-rebinding blocked).",
                    host, ip
                ),
            });
        }
    }
    Ok(())
}

fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let oct = ipv4.octets();
            ipv4.is_loopback() // 127.0.0.0/8
                || ipv4.is_private() // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || ipv4.is_link_local() // 169.254.0.0/16 (includes cloud metadata 169.254.169.254)
                || ipv4.is_broadcast()
                || ipv4 == Ipv4Addr::UNSPECIFIED // 0.0.0.0
                || (oct[0] == 100 && (oct[1] & 0xc0) == 64) // 100.64.0.0/10 (Carrier Grade NAT)
                || (oct[0] == 198 && (oct[1] & 0xfe) == 18) // 198.18.0.0/15 (Benchmarking)
                || (oct[0] == 192 && oct[1] == 0 && oct[2] == 2) // 192.0.2.0/24 (TEST-NET-1)
                || (oct[0] == 198 && oct[1] == 51 && oct[2] == 100) // 198.51.100.0/24 (TEST-NET-2)
                || (oct[0] == 203 && oct[1] == 0 && oct[2] == 113) // 203.0.113.0/24 (TEST-NET-3)
                || ipv4.is_multicast()
                || oct[0] >= 240 // 240.0.0.0/4 (Reserved)
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

fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        s
    } else {
        let mut idx = max_bytes;
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
        &s[..idx]
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
        assert!(validate_destination(
            &Url::parse("http://169.254.169.254/latest/meta-data").unwrap()
        )
        .is_err());

        // Localhost domain
        assert!(validate_destination(&Url::parse("http://localhost:3000").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://myhost.local/").unwrap()).is_err());

        // Block non-HTTP schemes
        assert!(validate_destination(&Url::parse("ftp://example.com/file").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("file:///etc/passwd").unwrap()).is_err());

        // Carrier-Grade NAT (RFC 6598) and Benchmarking (RFC 2544)
        assert!(validate_destination(&Url::parse("http://100.64.0.1/").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://198.18.0.1/").unwrap()).is_err());
        assert!(validate_destination(&Url::parse("http://192.0.2.1/").unwrap()).is_err());

        // Valid public URLs
        assert!(validate_destination(
            &Url::parse("https://raw.githubusercontent.com/test").unwrap()
        )
        .is_ok());
        assert!(
            validate_destination(&Url::parse("https://crates.io/api/v1/crates").unwrap()).is_ok()
        );

        // DNS-rebinding check: if a domain resolves to any private/loopback/restricted IP, reject it
        let rebind_ips = vec![
            "93.184.216.34".parse::<IpAddr>().unwrap(),
            "127.0.0.1".parse::<IpAddr>().unwrap(),
        ];
        assert!(validate_resolved_ips("evil-rebind.example.com", rebind_ips).is_err());

        let benchmark_ips = vec!["198.18.0.5".parse::<IpAddr>().unwrap()];
        assert!(validate_resolved_ips("benchmark.example.com", benchmark_ips).is_err());

        let cgnat_ips = vec!["100.64.1.2".parse::<IpAddr>().unwrap()];
        assert!(validate_resolved_ips("cgnat.example.com", cgnat_ips).is_err());

        let public_ips = vec!["93.184.216.34".parse::<IpAddr>().unwrap()];
        assert!(validate_resolved_ips("example.com", public_ips).is_ok());
    }

    #[test]
    fn test_resolve_redirect() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        assert_eq!(
            resolve_redirect(&base, "https://other.example/x", "webfetch")
                .unwrap()
                .as_str(),
            "https://other.example/x"
        );
        assert_eq!(
            resolve_redirect(&base, "/c", "webfetch").unwrap().as_str(),
            "https://example.com/c"
        );
        assert_eq!(
            resolve_redirect(&base, "d", "webfetch").unwrap().as_str(),
            "https://example.com/a/d"
        );
    }

    #[test]
    fn test_redirect_hop_to_loopback_is_rejected() {
        // A redirect hop to a private/loopback/metadata host is rejected
        // before any connection is opened (proven at unit level; a live
        // hop-redirect test is impossible from this VM because any public
        // first hop is already blocked by the egress proxy's DNS).
        let current = Url::parse("https://example.com/a").unwrap();
        assert!(next_hop_url(&current, "http://127.0.0.1/secret", "webfetch").is_err());
        assert!(next_hop_url(&current, "http://localhost:3000/", "webfetch").is_err());
        assert!(next_hop_url(
            &current,
            "https://169.254.169.254/latest/meta-data",
            "webfetch"
        )
        .is_err());
        assert!(next_hop_url(&current, "ftp://example.com/file", "webfetch").is_err());
        // A redirect to another public host passes the hop check; its DNS
        // is re-resolved and re-pinned on the next hop.
        assert!(next_hop_url(&current, "https://other.example/x", "webfetch").is_ok());
        assert!(next_hop_url(&current, "/b", "webfetch").is_ok());
    }

    #[tokio::test]
    async fn test_resolve_validated_addrs_ip_literal() {
        // Loopback literal is rejected without any DNS lookup.
        assert!(
            resolve_validated_addrs(&Url::parse("http://127.0.0.1:9/").unwrap(), "webfetch")
                .await
                .is_err()
        );
        // Benchmarking range (198.18/15) is rejected.
        assert!(
            resolve_validated_addrs(&Url::parse("http://198.18.0.5/").unwrap(), "webfetch")
                .await
                .is_err()
        );
        // A public literal IP validates to itself (no DNS needed).
        let addrs =
            resolve_validated_addrs(&Url::parse("https://93.184.216.34/").unwrap(), "webfetch")
                .await
                .unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].ip(), "93.184.216.34".parse::<IpAddr>().unwrap());
        assert_eq!(addrs[0].port(), 443);
    }

    #[tokio::test]
    async fn test_dns_lookup_fail_closed() {
        // .invalid never resolves: a failed lookup must refuse the fetch
        // instead of proceeding unverified (previous behaviour).
        assert!(validate_dns_lookup_async(
            &Url::parse("https://nonexistent-12345.invalid/").unwrap()
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn test_get_pinned_rejects_private_destination() {
        // The pinned fetch refuses a loopback target before opening a socket.
        assert!(
            get_pinned(&Url::parse("http://127.0.0.1:9/").unwrap(), "webfetch")
                .await
                .is_err()
        );
    }

    #[test]
    fn test_utf8_safe_truncation() {
        // Multi-byte emoji "🐾" is 4 bytes: [0xF0, 0x9F, 0x90, 0xBE]
        let s = "Hello 🐾 World";
        // Trying to slice at byte index 7 (middle of emoji) would panic with s[..7]
        let safe = safe_truncate_str(s, 7);
        assert_eq!(safe, "Hello "); // Cleanly backed off to char boundary

        // Exactly after the emoji (byte index 10)
        let safe2 = safe_truncate_str(s, 10);
        assert_eq!(safe2, "Hello 🐾");
    }
}
