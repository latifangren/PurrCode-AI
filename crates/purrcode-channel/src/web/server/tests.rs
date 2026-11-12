use super::*;
use purrcode_api::channel::Channel;
use purrcode_api::event::AgentEvent;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn test_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("purrcode-web-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start_server(
    token: Option<String>,
    purrcode_dir: PathBuf,
) -> (
    u16,
    Arc<WebChannel>,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let port = free_port();
    let channel = Arc::new(WebChannel::new());
    let server = WebServer::new(
        "127.0.0.1",
        port,
        token,
        channel.clone(),
        purrcode_dir,
        PathBuf::from("/tmp/ws"),
    );
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move {
        let _ = server.run(cancel_rx).await;
    });
    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (port, channel, cancel_tx, handle)
}

/// One-shot raw HTTP request (Connection: close responses).
fn http_raw(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf
}

/// Read from an SSE stream until a `data:` line arrives (or the
/// read times out). Returns everything read so far.
fn http_sse_until_data(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if out.windows(5).any(|w| w == b"data:") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn get(port: u16, path: &str) -> String {
    http_raw(port, &format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path))
}

fn post(port: u16, path: &str, body: &str) -> String {
    http_raw(
            port,
            &format!(
                "POST {} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                path,
                body.len(),
                body
            ),
        )
}

#[test]
fn test_request_has_token_exact_match_only() {
    let req = "GET / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer SECRET\r\n\r\n";
    assert!(request_has_token(req, "/", "SECRET"));
    // Lower-case scheme is fine.
    let req = "GET / HTTP/1.1\r\nHost: x\r\nAuthorization: bearer SECRET\r\n\r\n";
    assert!(request_has_token(req, "/", "SECRET"));
    // Query parameter is fine.
    let req = "GET / HTTP/1.1\r\nHost: x\r\n\r\n";
    assert!(request_has_token(req, "/?token=SECRET", "SECRET"));
    // A token that merely appears in the body does NOT authenticate.
    let req = "POST /api/prompt HTTP/1.1\r\nHost: x\r\n\r\n{\"prompt\": \"SECRET\"}";
    assert!(!request_has_token(req, "/api/prompt", "SECRET"));
    // Wrong token, wrong scheme, token as a different param.
    assert!(!request_has_token(
        "GET / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer WRONG\r\n\r\n",
        "/",
        "SECRET"
    ));
    assert!(!request_has_token(
        "GET / HTTP/1.1\r\nHost: x\r\n\r\n",
        "/?other=SECRET",
        "SECRET"
    ));
    // P2-1: a literal Authorization line inside the BODY does not
    // authenticate — the scan stops at the header/body blank line.
    assert!(!request_has_token(
        "POST /api/prompt HTTP/1.1\r\nHost: x\r\nContent-Length: 40\r\n\r\n{\"prompt\": \"x\"}\nAuthorization: Bearer SECRET",
        "/api/prompt",
        "SECRET"
    ));
    // P2-1 (I-4): Bearer with trailing junk does not authenticate.
    assert!(!request_has_token(
        "GET / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer SECRET extra\r\n\r\n",
        "/",
        "SECRET"
    ));
}

#[test]
fn test_webui_template_has_no_inline_model_data_handlers() {
    // P1-3 tripwire: model-controlled values (call_id, skill name/description,
    // cron prompt) must never be interpolated into an inline onclick or
    // unescaped into innerHTML. The banner/skills buttons use
    // addEventListener + dataset, and every interpolated field goes
    // through escapeHtml (which escapes ' as well).
    assert!(
        !WEB_INDEX_HTML.contains("onclick=\"resolveApproval("),
        "approval banner must not use an inline onclick with the call_id"
    );
    assert!(
        !WEB_INDEX_HTML.contains("onclick=\"approveSkill("),
        "skills tab must not use an inline onclick with the skill name"
    );
    assert!(
        WEB_INDEX_HTML.contains(".replace(/'/g, '&#39;')"),
        "escapeHtml must escape single quotes"
    );
    assert!(WEB_INDEX_HTML.contains("escapeHtml(c.description"));
    assert!(WEB_INDEX_HTML.contains("escapeHtml(j.prompt"));
    assert!(WEB_INDEX_HTML.contains("addEventListener('click'"));
}

#[test]
fn test_draft_frontmatter_parsing() {
    let content = "---\nname: git-helper\ndescription: Helps with git\n---\n\n# Skill\n";
    assert_eq!(
        draft_frontmatter(content),
        (
            Some("git-helper".to_string()),
            Some("Helps with git".to_string())
        )
    );
    assert_eq!(draft_frontmatter("no frontmatter"), (None, None));
    assert_eq!(
        draft_frontmatter("---\nname: only-name\n---\n"),
        (Some("only-name".to_string()), None)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_serves_index_and_status() {
    let dir = test_dir("index");
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone()).await;
    let index = get(port, "/");
    assert!(index.starts_with("HTTP/1.1 200 OK"));
    assert!(index.contains("PurrCode AI"));
    assert!(index.contains("#11111b"));
    let status = get(port, "/api/status");
    assert!(status.starts_with("HTTP/1.1 200 OK"));
    assert!(status.contains("\"status\":\"ok\""));
    let missing = get(port, "/nope");
    assert!(missing.starts_with("HTTP/1.1 404"));
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_sse_streams_bus_events_in_purrcode_shape() {
    let dir = test_dir("sse");
    let (port, channel, cancel_tx, handle) = start_server(None, dir.clone()).await;
    let channel_clone = channel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = channel_clone
            .send_event(&AgentEvent::TextChunk {
                text: "hello sse".to_string(),
            })
            .await;
    });
    let out = http_sse_until_data(port);
    assert!(out.contains("text/event-stream"));
    // dev-muse serializes AgentEvent internally tagged
    // ({"type": "TextChunk", "payload": {...}}), not externally
    // tagged like the dev-claude reference.
    assert!(out.contains("\"type\":\"TextChunk\""));
    assert!(out.contains("hello sse"));
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_approval_post_resolves_pending() {
    let dir = test_dir("approval");
    let (port, channel, cancel_tx, handle) = start_server(None, dir.clone()).await;
    let waiter = tokio::spawn({
        let ch = channel.clone();
        async move {
            ch.request_approval("call-9", "bash", "run", None)
                .await
                .unwrap()
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let resp = post(
        port,
        "/api/approval",
        r#"{"call_id": "call-9", "approved": true}"#,
    );
    assert!(resp.contains("\"resolved\":true"), "resp: {resp}");
    assert!(waiter.await.unwrap());
    // Unknown call id resolves to false, still 200.
    let resp = post(
        port,
        "/api/approval",
        r#"{"call_id": "call-nope", "approved": true}"#,
    );
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_token_auth() {
    let dir = test_dir("token");
    let (port, _ch, cancel_tx, handle) =
        start_server(Some("SECRET".to_string()), dir.clone()).await;
    assert!(get(port, "/").starts_with("HTTP/1.1 401"));
    assert!(get(port, "/?token=SECRET").starts_with("HTTP/1.1 200 OK"));
    let with_header = http_raw(
        port,
        "GET / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer SECRET\r\n\r\n",
    );
    assert!(with_header.starts_with("HTTP/1.1 200 OK"));
    // Token hidden inside a JSON body does not authenticate.
    let trapped = post(port, "/api/prompt", r#"{"prompt": "SECRET"}"#);
    assert!(trapped.starts_with("HTTP/1.1 401"), "resp: {trapped}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_cron_lists_jobs_jsonc() {
    let dir = test_dir("cron");
    std::fs::create_dir_all(dir.join("cron")).unwrap();
    std::fs::write(
            dir.join("cron").join("jobs.jsonc"),
            "// a comment line\n{\"jobs\": [{\"id\": \"cron_abc\", \"name\": \"t\", \"enabled\": true, \"schedule\": {\"expr\": \"0 * * * *\", \"display\": \"hourly\", \"once\": false}, \"prompt\": \"hi\", \"tools\": [\"read\"], \"preset\": {\"name\": \"tabby\", \"approval\": \"default\", \"guardrail\": \"default\", \"sandbox\": \"workspace-write\"}, \"delivery\": {\"kind\": \"queue_only\"}, \"created_at\": \"2026-11-12T00:00:00+07:00\"}]}\n",
        )
        .unwrap();
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone()).await;
    let resp = get(port, "/api/cron");
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    let jobs: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(jobs[0]["id"], "cron_abc");
    assert_eq!(jobs[0]["schedule"], "hourly");
    assert_eq!(jobs[0]["prompt"], "hi");
    assert_eq!(jobs[0]["allowed_tools"], serde_json::json!(["read"]));
    assert_eq!(jobs[0]["enabled"], true);
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_skills_lists_drafts_and_live() {
    let dir = test_dir("skills");
    std::fs::create_dir_all(dir.join("skills").join(".drafts")).unwrap();
    std::fs::write(
        dir.join("skills").join(".drafts").join("draft1.purr_x.md"),
        "---\nname: draft-one\ndescription: A draft skill\n---\n\n# draft\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("skills").join("git")).unwrap();
    std::fs::write(
        dir.join("skills").join("git").join("SKILL.md"),
        "---\nname: git\ndescription: Git workflows\n---\n\n# git\n",
    )
    .unwrap();
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone()).await;
    let resp = get(port, "/api/skills");
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    let val: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(val["candidates"][0]["name"], "draft-one");
    assert_eq!(val["candidates"][0]["description"], "A draft skill");
    assert_eq!(val["skills"][0]["name"], "git");
    // .drafts itself must not appear as a live skill.
    assert!(!val["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == ".drafts"));
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_skills_approve_resolves_pending_draft() {
    let dir = test_dir("skills-approve");
    let (port, channel, cancel_tx, handle) = start_server(None, dir.clone()).await;
    // A curator hook waiting on this draft (mirrors the gateway's
    // post-turn hook path through the web channel).
    let waiter = tokio::spawn({
        let ch = channel.clone();
        async move {
            let req = ch.skill_draft_requester("s_web", None);
            req.request_approval("draft-one", "A draft skill", Some("body"))
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let resp = post(port, "/api/skills/approve", r#"{"name": "draft-one"}"#);
    assert!(resp.contains("\"resolved\":true"), "resp: {resp}");
    assert!(waiter.await.unwrap());
    // No pending draft anymore: the endpoint says so, still 200.
    let resp = post(port, "/api/skills/approve", r#"{"name": "draft-one"}"#);
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    let resp = post(port, "/api/skills/reject", r#"{"name": "draft-none"}"#);
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}
