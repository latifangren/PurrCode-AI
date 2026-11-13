// SPDX-License-Identifier: GPL-3.0-or-later

use crate::web::channel::SKILL_DRAFT_KEY_PREFIX;
use crate::web::html::web_index_html;
use crate::web::server::sessions::{
    handle_create_session, handle_list_sessions, handle_transcript, query_param,
};
use crate::web::sessions::{WebSession, WebSessionRegistry, DEFAULT_WEB_SESSION_ID};
use purrcode_api::error::{PurrError, Result};
use purrcode_config::jsonc::clean_jsonc;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

mod sessions;

#[async_trait::async_trait]
pub trait PromptDispatcher: Send + Sync {
    /// Run one turn of `session` for `prompt`. The session carries
    /// its own bus, channel, and turn lock, so the dispatcher never
    /// needs a separate channel argument.
    async fn dispatch(&self, session: Arc<WebSession>, prompt: String) -> Result<()>;
}

pub struct WebServer {
    host: String,
    port: u16,
    token: Option<String>,
    registry: Arc<WebSessionRegistry>,
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
}

impl WebServer {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        token: Option<String>,
        registry: Arc<WebSessionRegistry>,
        purrcode_dir: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            token,
            registry,
            purrcode_dir: purrcode_dir.into(),
            workspace_root: workspace_root.into(),
            dispatcher: None,
        }
    }

    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn PromptDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
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
                        let registry = self.registry.clone();
                        let token = self.token.clone();
                        let purrcode_dir = self.purrcode_dir.clone();
                        let workspace_root = self.workspace_root.clone();
                        let dispatcher = self.dispatcher.clone();

                        tokio::spawn(async move {
                            let _ = handle_http_connection(
                                stream,
                                registry,
                                token,
                                purrcode_dir,
                                workspace_root,
                                dispatcher,
                            ).await;
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

/// Maximum bytes for one HTTP request (headers + body).
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// Read one complete HTTP request from the stream: loop until the
/// header/body blank line is seen, then until `Content-Length` bytes
/// of body have arrived. Returns `Ok(None)` when the request exceeds
/// `MAX_REQUEST_BYTES` (the caller answers 413), and
/// `Ok(Some(String::new()))` on an immediate EOF.
async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Result<Option<String>> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST_BYTES {
            return Ok(None);
        }
        if let Some((headers_end, body_start)) = find_headers_end(&buf) {
            let content_length = headers_content_length(&buf[..headers_end]);
            if buf.len() >= body_start + content_length {
                break;
            }
        }
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Byte range of the header/body blank line, as
/// `(headers_end, body_start)`, when the blank line is present:
/// `\r\n\r\n` (standard) or a bare `\n\n` (tolerant clients).
fn find_headers_end(buf: &[u8]) -> Option<(usize, usize)> {
    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some((p, p + 4));
    }
    if let Some(p) = buf.windows(2).position(|w| w == b"\n\n") {
        return Some((p, p + 2));
    }
    None
}

/// `Content-Length` from a header block (0 when absent or
/// unparseable — the downstream JSON parse 400s those anyway).
fn headers_content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers);
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }
    0
}

async fn handle_http_connection(
    mut stream: tokio::net::TcpStream,
    registry: Arc<WebSessionRegistry>,
    token: Option<String>,
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
) -> Result<()> {
    // R3 fix (P1-2): read one full HTTP request — headers until the
    // blank line, then Content-Length bytes of body — instead of a
    // single 8192-byte read. The single read dropped any request whose
    // headers and body arrived as two TCP segments, and rejected
    // legitimate prompts over ~8 KB. Bounded at 1 MiB so a client
    // cannot grow the buffer without limit.
    let request = match read_http_request(&mut stream).await? {
        Some(request) => request,
        None => {
            let resp = "HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };
    if request.is_empty() {
        return Ok(());
    }

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

    // Token authentication check if configured. Headers only, exact
    // match: `Authorization: Bearer <token>` or a `?token=<token>`
    // query parameter. A token that merely appears in the body (or
    // with trailing junk after it) does not authenticate. (R3
    // hardening over the dev-claude reference's `contains` check; the
    // headers-only + exact-match rules are the P2-1 fix.)
    if let Some(ref required_token) = token {
        if !request_has_token(&request, raw_uri, required_token) {
            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nUnauthorized\n";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    }

    match (method, uri) {
        ("GET", "/") | ("GET", "/index.html") => {
            let body = web_index_html();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/health") | ("GET", "/api/status") | ("GET", "/api/health") => {
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
            // Server-Sent Events (SSE) streaming, scoped to one
            // session (v0.4 B1): `?session=<id>` picks the chat; no
            // parameter means the legacy default `purr_web` (R3
            // clients subscribe without one). An explicit id is
            // materialized on demand when the session exists on
            // disk — after a restart a resumed chat has a file but
            // no live entry yet (re-checker P2-1). A valid id with
            // no file is a 404 instead of an empty stream, so a
            // typo or a stale tab is visible instead of silently
            // subscribing to nothing.
            let session = match query_param(raw_uri, "session") {
                Some(id) => match registry.ensure_session_if_exists(&id) {
                    Ok(s) => s,
                    Err(_) => {
                        let body = serde_json::json!({ "error": "session not found" }).to_string();
                        let resp = json_response(404, "Not Found", &body);
                        stream.write_all(resp.as_bytes()).await?;
                        return Ok(());
                    }
                },
                None => match registry.default_session() {
                    Ok(s) => s,
                    Err(e) => {
                        let body = serde_json::json!({ "error": e.to_string() }).to_string();
                        let resp = json_response(500, "Internal Server Error", &body);
                        stream.write_all(resp.as_bytes()).await?;
                        return Ok(());
                    }
                },
            };
            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
            stream.write_all(header.as_bytes()).await?;

            let mut rx = session.channel.subscribe();
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
        ("GET", "/api/sessions") => {
            let (status, body) = handle_list_sessions(&registry);
            let resp = json_response(status, reason_phrase(status), &body);
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/sessions") => {
            let (status, body) = handle_create_session(&registry);
            let resp = json_response(status, reason_phrase(status), &body);
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", _) if uri.starts_with("/api/sessions/") && uri.ends_with("/transcript") => {
            let (status, body) = handle_transcript(&registry, uri);
            let resp = json_response(status, reason_phrase(status), &body);
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/cron") => {
            // dev-muse stores jobs in `cron/jobs.jsonc` as
            // `{"jobs": [...]}` (JSONC — comments allowed); the WebUI
            // table wants a plain array with display fields.
            let jobs_path = purrcode_dir.join("cron").join("jobs.jsonc");
            let mut jobs: Vec<serde_json::Value> = Vec::new();
            if let Ok(raw) = tokio::fs::read_to_string(&jobs_path).await {
                let cleaned = clean_jsonc(&raw);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                    if let Some(list) = val.get("jobs").and_then(|j| j.as_array()) {
                        jobs = list
                            .iter()
                            .map(|j| {
                                let schedule = j
                                    .get("schedule")
                                    .and_then(|s| s.get("display"))
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let tools = j
                                    .get("tools")
                                    .cloned()
                                    .unwrap_or_else(|| serde_json::json!([]));
                                serde_json::json!({
                                    "id": j.get("id").cloned().unwrap_or(serde_json::Value::Null),
                                    "schedule": schedule,
                                    "prompt": j.get("prompt").cloned().unwrap_or(serde_json::Value::Null),
                                    "allowed_tools": tools,
                                    "next_run": j.get("next_run").cloned().unwrap_or(serde_json::Value::Null),
                                    "enabled": j.get("enabled").cloned().unwrap_or(serde_json::Value::Null),
                                })
                            })
                            .collect();
                    }
                }
            }
            let body = serde_json::to_string(&jobs).unwrap_or_else(|_| "[]".to_string());
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/skills") | ("GET", "/api/skills/candidates") => {
            // Staged curator drafts live in `skills/.drafts/` as
            // `<name>.<session>.md` files (F20) — NOT in a
            // `candidates/` directory (that was the dev-claude layout).
            // Name and description come from the draft's SKILL.md
            // frontmatter; the filename is only a fallback.
            let drafts_dir = purrcode_dir.join("skills").join(".drafts");
            let mut candidates = Vec::new();
            if drafts_dir.exists() {
                if let Ok(mut rd) = tokio::fs::read_dir(&drafts_dir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let path = entry.path();
                        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md")
                        {
                            let content =
                                tokio::fs::read_to_string(&path).await.unwrap_or_default();
                            let (name, desc) = draft_frontmatter(&content);
                            let name = name.unwrap_or_else(|| {
                                path.file_stem()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or_default()
                                    .split('.')
                                    .next()
                                    .unwrap_or_default()
                                    .to_string()
                            });
                            candidates.push(serde_json::json!({
                                "name": name,
                                "description": desc.unwrap_or_else(|| "Draft skill".to_string()),
                                "path": path.display().to_string()
                            }));
                        }
                    }
                }
            }

            let mut active_skills = Vec::new();
            let skills_dir = purrcode_dir.join("skills");
            if skills_dir.exists() {
                if let Ok(mut rd) = tokio::fs::read_dir(&skills_dir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let path = entry.path();
                        if path.is_dir() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or_default()
                                .to_string();
                            if name != "candidates" && name != "archive" && name != ".drafts" {
                                let skill_file = path.join("SKILL.md");
                                let desc = if skill_file.exists() {
                                    tokio::fs::read_to_string(&skill_file)
                                        .await
                                        .ok()
                                        .and_then(|c| {
                                            c.lines()
                                                .find(|l| l.trim().starts_with("description:"))
                                                .map(|l| {
                                                    l.trim()
                                                        .trim_start_matches("description:")
                                                        .trim()
                                                        .to_string()
                                                })
                                        })
                                        .unwrap_or_else(|| "Active custom skill".to_string())
                                } else {
                                    "Active custom skill".to_string()
                                };
                                active_skills.push(serde_json::json!({
                                    "name": name,
                                    "description": desc,
                                    "path": path.display().to_string()
                                }));
                            }
                        }
                    }
                }
            }

            let body = serde_json::json!({
                "candidates": candidates,
                "skills": active_skills
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/skills/approve") => {
            // Resolve-only (R3): the WebUI never promotes a draft by
            // moving files itself — that would bypass the curator's
            // validation and the Tinker save turn. Approving resolves
            // the pending `skill-draft:<name>` approval the
            // WebSkillDraftRequester registered; the curator hook in
            // the dispatcher performs the real save/discard after.
            // `resolved: false` means no draft approval was waiting
            // (stale tab, or the 120s card already expired).
            // v0.4 B1: the approval lives on one session's channel
            // (the body's optional `session_id`, default `purr_web`).
            let body_str = extract_body(&request);
            let mut resolved = false;
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                    let key = format!("{}{}", SKILL_DRAFT_KEY_PREFIX, name);
                    if let Some(session) = approval_session(&registry, &val) {
                        resolved = session.channel.resolve_approval(&key, true).await;
                    }
                }
            }
            let body = format!("{{\"resolved\":{}}}", resolved);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/skills/reject") => {
            // Resolve-only, same contract as approve (see above).
            let body_str = extract_body(&request);
            let mut resolved = false;
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                    let key = format!("{}{}", SKILL_DRAFT_KEY_PREFIX, name);
                    if let Some(session) = approval_session(&registry, &val) {
                        resolved = session.channel.resolve_approval(&key, false).await;
                    }
                }
            }
            let body = format!("{{\"resolved\":{}}}", resolved);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("GET", "/api/plugins") => {
            let mut plugins = Vec::new();
            let p_dir = purrcode_dir.join("plugins");
            if p_dir.exists() {
                if let Ok(mut rd) = tokio::fs::read_dir(&p_dir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let path = entry.path();
                        if path.is_dir() {
                            let manifest = path.join("plugin.yaml");
                            if manifest.exists() {
                                if let Ok(yaml) = tokio::fs::read_to_string(&manifest).await {
                                    plugins.push(serde_json::json!({
                                        "name": entry.file_name().to_string_lossy().to_string(),
                                        "path": path.display().to_string(),
                                        "manifest": yaml
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            let body = serde_json::json!({ "plugins": plugins }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        ("POST", "/api/prompt") => {
            let body_str = extract_body(&request);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                if let Some(prompt) = val.get("prompt").and_then(|p| p.as_str()) {
                    // v0.4 B1: the prompt goes to one session. No
                    // `session_id` means the legacy default
                    // `purr_web` (R3 clients send none). An explicit
                    // id is materialized on demand when the session
                    // exists on disk — a resumed chat after restart
                    // has a file but no live entry yet (re-checker
                    // P2-1); a valid id with no file is a 404, and
                    // a present-but-non-string id is a 400 (P3-3) —
                    // neither silently falls back to the default.
                    let session = match val.get("session_id") {
                        Some(v) => {
                            let Some(id) = v.as_str() else {
                                let body =
                                    serde_json::json!({ "error": "session_id must be a string" })
                                        .to_string();
                                let resp = json_response(400, "Bad Request", &body);
                                stream.write_all(resp.as_bytes()).await?;
                                return Ok(());
                            };
                            registry.ensure_session_if_exists(id).ok()
                        }
                        None => registry.default_session().ok(),
                    };
                    let Some(session) = session else {
                        let body = serde_json::json!({ "error": "session not found" }).to_string();
                        let resp = json_response(404, "Not Found", &body);
                        stream.write_all(resp.as_bytes()).await?;
                        return Ok(());
                    };
                    if let Some(ref disp) = dispatcher {
                        let disp = disp.clone();
                        let p = prompt.to_string();
                        tokio::spawn(async move {
                            if let Err(e) = disp.dispatch(session, p).await {
                                tracing::error!("Web prompt execution error: {}", e);
                            }
                        });
                        let body = serde_json::json!({ "status": "dispatched" }).to_string();
                        let resp = json_response(200, "OK", &body);
                        stream.write_all(resp.as_bytes()).await?;
                        return Ok(());
                    } else {
                        let body = serde_json::json!({
                            "error": "No prompt dispatcher registered on web server"
                        })
                        .to_string();
                        let resp = json_response(503, "Service Unavailable", &body);
                        stream.write_all(resp.as_bytes()).await?;
                        return Ok(());
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
                // v0.4 B1: resolve on the session's channel (the
                // body's optional `session_id`, default `purr_web`).
                // A session with no live entry has nothing pending,
                // so it resolves false instead of touching another
                // session's approvals.
                let resolved = match approval_session(&registry, &val) {
                    Some(session) => session.channel.resolve_approval(call_id, approved).await,
                    None => false,
                };
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

/// One-line JSON response (`Connection: close`); the new session
/// endpoints answer through this instead of repeating the format.
fn json_response(status: u16, reason: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        body.len(),
        body
    )
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

/// The live session an approval body belongs to (v0.4 B1): the
/// body's optional `session_id`, defaulting to the legacy `purr_web`.
/// An explicit id is materialized on demand when the session exists
/// on disk (a resumed chat after restart — re-checker P2-1); a
/// present-but-non-string id resolves nothing (P3-3), and a session
/// with no live entry has nothing pending, so the caller answers
/// `resolved: false` instead of resolving against another session's
/// approvals.
fn approval_session(
    registry: &Arc<WebSessionRegistry>,
    val: &serde_json::Value,
) -> Option<Arc<WebSession>> {
    let Some(v) = val.get("session_id") else {
        return registry.get_session(DEFAULT_WEB_SESSION_ID);
    };
    let id = v.as_str()?;
    registry.ensure_session_if_exists(id).ok()
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

/// Exact token check for a request: `Authorization: Bearer <token>`
/// header (case-insensitive scheme) or a `?token=<token>` query
/// parameter. Any other occurrence of the token does not authenticate.
/// Headers only (P2-1): the scan stops at the header/body blank line,
/// so a literal `Authorization: Bearer ...` line inside a POST body
/// cannot authenticate. The Bearer value must match exactly — trailing
/// junk after the token does not authenticate either.
fn request_has_token(request: &str, raw_uri: &str, required_token: &str) -> bool {
    // Headers only: everything before the first blank line.
    let headers = match request.split_once("\r\n\r\n") {
        Some((h, _)) => h,
        None => request,
    };
    let headers = match headers.split_once("\n\n") {
        Some((h, _)) => h,
        None => headers,
    };
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("authorization") {
                let mut parts = value.split_whitespace();
                let scheme_ok = parts
                    .next()
                    .is_some_and(|s| s.eq_ignore_ascii_case("bearer"));
                let token_ok = parts.next() == Some(required_token);
                let exact = parts.next().is_none();
                if scheme_ok && token_ok && exact {
                    return true;
                }
            }
        }
    }
    if let Some(query) = raw_uri.split_once('?').map(|(_, q)| q) {
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                if k == "token" && v == required_token {
                    return true;
                }
            }
        }
    }
    false
}

/// Read `name:` / `description:` from a SKILL.md frontmatter block
/// (between the leading `---` lines). Each is `None` when the key is
/// absent; the caller falls back to the staged filename / a default.
fn draft_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut desc = None;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if name.is_none() {
            if let Some(v) = line.strip_prefix("name:") {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    name = Some(v.to_string());
                }
            }
        }
        if desc.is_none() {
            if let Some(v) = line.strip_prefix("description:") {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    desc = Some(v.to_string());
                }
            }
        }
    }
    (name, desc)
}

#[cfg(test)]
mod tests;
