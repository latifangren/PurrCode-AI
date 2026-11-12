// SPDX-License-Identifier: GPL-3.0-or-later

use crate::web::channel::WebChannel;
use crate::web::html::WEB_INDEX_HTML;
use purrcode_api::error::{PurrError, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

pub struct WebServer {
    host: String,
    port: u16,
    token: Option<String>,
    channel: Arc<WebChannel>,
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
}

impl WebServer {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        token: Option<String>,
        channel: Arc<WebChannel>,
        purrcode_dir: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            token,
            channel,
            purrcode_dir: purrcode_dir.into(),
            workspace_root: workspace_root.into(),
        }
    }

    pub async fn run(&self, mut cancel_rx: watch::Receiver<bool>) -> Result<()> {
        let bind_addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| PurrError::Channel {
                channel: "web".to_string(),
                message: format!("Failed to bind {}: {}", bind_addr, e),
            })?;

        println!("🌐 WebUI server running at http://{}", bind_addr);
        if self.token.is_some() {
            println!("🔒 Token authentication enabled for remote access.");
        }

        loop {
            tokio::select! {
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        break;
                    }
                }
                accept_res = listener.accept() => {
                    if let Ok((stream, _)) = accept_res {
                        let channel = self.channel.clone();
                        let token = self.token.clone();
                        let purrcode_dir = self.purrcode_dir.clone();
                        let workspace_root = self.workspace_root.clone();

                        tokio::spawn(async move {
                            let _ = handle_http_connection(
                                stream,
                                channel,
                                token,
                                purrcode_dir,
                                workspace_root,
                            ).await;
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

async fn handle_http_connection(
    mut stream: tokio::net::TcpStream,
    channel: Arc<WebChannel>,
    token: Option<String>,
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
) -> Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let resp = "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let method = parts[0];
    let raw_uri = parts[1];
    let uri = raw_uri.split('?').next().unwrap_or(raw_uri);

    // Token authentication check if configured
    if let Some(ref required_token) = token {
        let auth_header = format!("Authorization: Bearer {}", required_token);
        let token_param = format!("token={}", required_token);
        let is_authed = request.contains(&auth_header) || raw_uri.contains(&token_param);
        if !is_authed {
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nUnauthorized\n";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    }

    match (method, uri) {
        ("GET", "/") | ("GET", "/index.html") => {
            let body = WEB_INDEX_HTML;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/status") | ("GET", "/api/health") => {
            let body = serde_json::json!({
                "status": "ok",
                "channel": "web",
                "workspace": workspace_root.display().to_string(),
                "purrcode_dir": purrcode_dir.display().to_string()
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/events") => {
            // Server-Sent Events (SSE) streaming
            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
            stream.write_all(header.as_bytes()).await?;

            let mut rx = channel.subscribe();
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Ok(data) = serde_json::to_string(&event) {
                            let msg = format!("data: {}\n\n", data);
                            if stream.write_all(msg.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
        ("GET", "/api/cron") => {
            let cron_path = purrcode_dir.join("cron_jobs.json");
            let body = if cron_path.exists() {
                tokio::fs::read_to_string(&cron_path)
                    .await
                    .unwrap_or_else(|_| "[]".to_string())
            } else {
                "[]".to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/skills") => {
            let candidates_dir = purrcode_dir.join("skills").join("candidates");
            let mut candidates = Vec::new();
            if candidates_dir.exists() {
                if let Ok(mut rd) = tokio::fs::read_dir(&candidates_dir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let path = entry.path();
                        if path.is_dir() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or_default()
                                .to_string();
                            let skill_file = path.join("SKILL.md");
                            if skill_file.exists() {
                                let desc = tokio::fs::read_to_string(&skill_file)
                                    .await
                                    .unwrap_or_default()
                                    .lines()
                                    .find(|l| l.trim().starts_with("description:"))
                                    .map(|l| {
                                        l.trim()
                                            .trim_start_matches("description:")
                                            .trim()
                                            .to_string()
                                    })
                                    .unwrap_or_else(|| "Candidate skill".to_string());
                                candidates.push(serde_json::json!({
                                    "name": name,
                                    "description": desc,
                                    "path": skill_file.display().to_string()
                                }));
                            }
                        }
                    }
                }
            }
            let body = serde_json::json!({ "candidates": candidates }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/skills/approve") => {
            let body_str = extract_body(&request);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                    let cand_path = purrcode_dir.join("skills").join("candidates").join(name);
                    let target_path = purrcode_dir.join("skills").join(name);
                    if cand_path.exists() {
                        if target_path.exists() {
                            let _ = tokio::fs::remove_dir_all(&target_path).await;
                        }
                        if tokio::fs::rename(&cand_path, &target_path).await.is_ok() {
                            let body = format!(
                                "{{\"approved\":true,\"path\":\"{}\"}}",
                                target_path.display()
                            );
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            stream.write_all(resp.as_bytes()).await?;
                            return Ok(());
                        }
                    }
                }
            }
            let resp = "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/memory") => {
            let file_name = if raw_uri.contains("SOUL.md") {
                "SOUL.md"
            } else if raw_uri.contains("USER.md") {
                "USER.md"
            } else {
                "MEMORY.md"
            };
            let mem_file = purrcode_dir.join(file_name);
            let body = if mem_file.exists() {
                tokio::fs::read_to_string(&mem_file)
                    .await
                    .unwrap_or_default()
            } else {
                format!("# {}\n\nNo memory record created yet.", file_name)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/approval") => {
            let body_str = extract_body(&request);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                let call_id = val
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let approved = val
                    .get("approved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let resolved = channel.resolve_approval(call_id, approved).await;
                let body = format!("{{\"resolved\":{}}}", resolved);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).await?;
            } else {
                let resp = "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n";
                stream.write_all(resp.as_bytes()).await?;
            }
        }
        _ => {
            let not_found =
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(not_found.as_bytes()).await?;
        }
    }

    Ok(())
}

fn extract_body(request: &str) -> String {
    if let Some(pos) = request.find("\r\n\r\n") {
        request[pos + 4..].to_string()
    } else if let Some(pos) = request.find("\n\n") {
        request[pos + 2..].to_string()
    } else {
        String::new()
    }
}
