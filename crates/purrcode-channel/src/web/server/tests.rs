use super::test_support::*;
use crate::web::html::web_index_html;
use purrcode_api::channel::Channel;
use purrcode_api::event::AgentEvent;
use purrcode_api::provider::{ChatMessage, MessageRole};
use purrcode_core::SessionStore;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_webui_template_has_no_inline_model_data_handlers() {
    // P1-3 tripwire: model-controlled values (call_id, skill name/description,
    // cron prompt) must never be interpolated into an inline onclick or
    // unescaped into innerHTML. The banner/skills buttons use
    // addEventListener + dataset, and every interpolated field goes
    // through escapeHtml (which escapes ' as well).
    assert!(
        !web_index_html().contains("onclick=\"resolveApproval("),
        "approval banner must not use an inline onclick with the call_id"
    );
    assert!(
        !web_index_html().contains("onclick=\"approveSkill("),
        "skills tab must not use an inline onclick with the skill name"
    );
    assert!(
        web_index_html().contains(".replace(/'/g, '&#39;')"),
        "escapeHtml must escape single quotes"
    );
    assert!(web_index_html().contains("escapeHtml(c.description"));
    assert!(web_index_html().contains("escapeHtml(j.prompt"));
    assert!(web_index_html().contains("addEventListener('click'"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_serves_index_and_status() {
    let dir = test_dir("index");
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
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
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    // No `?session=` subscribes to the legacy default session.
    let channel = registry.default_session().unwrap().channel.clone();
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
    // The SSE response still carries the CORS header the hand-rolled
    // server set (B2: the Axum handler sets it explicitly).
    assert!(out.contains("access-control-allow-origin: *"), "out: {out}");
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
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let channel = registry.default_session().unwrap().channel.clone();
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
        start_server(Some("SECRET".to_string()), dir.clone(), None).await;
    assert!(get(port, "/").starts_with("HTTP/1.1 401"));
    assert!(get(port, "/?token=SECRET").starts_with("HTTP/1.1 200 OK"));
    let with_header = http_raw(
        port,
        "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\nAuthorization: Bearer SECRET\r\n\r\n",
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
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
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
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
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
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let channel = registry.default_session().unwrap().channel.clone();
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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn user_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: MessageRole::User,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn assistant_msg(content: &str) -> ChatMessage {
    ChatMessage {
        role: MessageRole::Assistant,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
    }
}

/// v0.4 B1: create → list → transcript round-trip. A foreign
/// (cron) session on disk never appears in the web list, and the
/// legacy `purr_web` session does.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_sessions_create_list_transcript() {
    let dir = test_dir("sessions-crud");
    let store = SessionStore::new(&dir);
    store
        .append_message("purr_web", &user_msg("hello from legacy"))
        .unwrap();
    store
        .append_message("purr_web", &assistant_msg("hi"))
        .unwrap();
    store
        .append_message("purr_cron_0123456789ab", &user_msg("cron"))
        .unwrap();

    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    let resp = get(port, "/api/sessions");
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");
    let list: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1, "list: {list}");
    assert_eq!(list[0]["id"], "purr_web");
    assert_eq!(list[0]["message_count"], 2);
    assert_eq!(list[0]["title"], "hello from legacy");

    let resp = post(port, "/api/sessions", "{}");
    assert!(resp.starts_with("HTTP/1.1 201"), "resp: {resp}");
    let created: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    let new_id = created["id"].as_str().unwrap().to_string();
    assert!(
        crate::web::sessions::is_valid_web_session_id(&new_id) && new_id != "purr_web",
        "id: {new_id}"
    );

    // The new (empty) session lists at once, before its first message.
    let resp = get(port, "/api/sessions");
    let list: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 2, "list: {list}");
    let fresh = list
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == new_id)
        .unwrap();
    assert_eq!(fresh["message_count"], 0);
    assert_eq!(fresh["title"], "");

    // Transcript replays the legacy chat's history in order.
    let resp = get(port, "/api/sessions/purr_web/transcript");
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["id"], "purr_web");
    assert_eq!(val["messages"].as_array().unwrap().len(), 2);
    assert_eq!(val["messages"][0]["role"], "user");
    assert_eq!(val["messages"][0]["content"], "hello from legacy");
    assert_eq!(val["messages"][1]["role"], "assistant");

    // The fresh session's transcript is empty, not a 404.
    let resp = get(port, &format!("/api/sessions/{new_id}/transcript"));
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["messages"].as_array().unwrap().len(), 0);

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B1: transcript errors — invalid shape is a 400 (including
/// traversal and foreign session ids), a valid-but-unknown id is a 404.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_sessions_transcript_errors() {
    let dir = test_dir("sessions-err");
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    let resp = get(port, "/api/sessions/purr_web_0123456789ab/transcript");
    assert!(resp.starts_with("HTTP/1.1 404"), "resp: {resp}");

    let resp = get(port, "/api/sessions/purr_web/transcript");
    assert!(resp.starts_with("HTTP/1.1 404"), "resp: {resp}");

    for bad in [
        "purr_web_..%2F..%2Fetc",
        "purr_web_0123456789AB",
        "purr_cron_0123456789ab",
        "purr_hb_0123456789ab",
        "..%2F..%2Fetc",
    ] {
        let resp = get(port, &format!("/api/sessions/{bad}/transcript"));
        assert!(resp.starts_with("HTTP/1.1 400"), "id {bad}: resp: {resp}");
    }

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B1: prompts route to the session in the body; no session
/// means the legacy default; an unknown explicit id is a 404 and
/// dispatches nothing.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_prompt_routes_to_session() {
    let dir = test_dir("sessions-prompt");
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let disp = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), Some(disp)).await;
    let session_a = registry.create_session().unwrap();

    let resp = post(port, "/api/prompt", r#"{"prompt": "hi default"}"#);
    assert!(resp.contains("\"status\":\"dispatched\""), "resp: {resp}");
    let resp = post(
        port,
        "/api/prompt",
        &format!(r#"{{"prompt": "hi a", "session_id": "{}"}}"#, session_a.id),
    );
    assert!(resp.contains("\"status\":\"dispatched\""), "resp: {resp}");
    let resp = post(
        port,
        "/api/prompt",
        r#"{"prompt": "hi x", "session_id": "purr_web_0123456789ab"}"#,
    );
    assert!(resp.starts_with("HTTP/1.1 404"), "resp: {resp}");

    // The dispatcher runs in spawned tasks, so the two calls
    // can land in either order — assert the set, not the order
    // (fix pass B5 r1 P1: the ordered assert flaked under
    // full-suite load).
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut calls = calls.lock().unwrap().clone();
    calls.sort();
    assert_eq!(calls.len(), 2, "calls: {calls:?}");
    assert_eq!(calls[0], ("purr_web".to_string(), "hi default".to_string()));
    assert_eq!(calls[1], (session_a.id.clone(), "hi a".to_string()));

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B1: SSE is scoped per session — an event sent to session A
/// reaches A's subscribers only, and an unknown session id is a 404.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_sse_isolated_per_session() {
    let dir = test_dir("sessions-sse");
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let a = registry.create_session().unwrap();
    let b = registry.create_session().unwrap();

    let a_ch = a.channel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = a_ch
            .send_event(&AgentEvent::TextChunk {
                text: "for-a-only".to_string(),
            })
            .await;
    });

    let out_a = http_sse_read(
        port,
        &format!("/api/events?session={}", a.id),
        Duration::from_secs(5),
    );
    assert!(out_a.contains("for-a-only"), "out_a: {out_a}");

    // B's stream stays silent within a full second.
    let out_b = http_sse_read(
        port,
        &format!("/api/events?session={}", b.id),
        Duration::from_secs(1),
    );
    assert!(out_b.contains("text/event-stream"), "out_b: {out_b}");
    assert!(!out_b.contains("data:"), "out_b: {out_b}");

    let resp = get(port, "/api/events?session=purr_web_0123456789ab");
    assert!(resp.starts_with("HTTP/1.1 404"), "resp: {resp}");

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B1: an approval pending on session A cannot be resolved
/// through session B's id — it stays pending until resolved with
/// the right session (or the default when the body carries none).
#[tokio::test(flavor = "multi_thread")]
async fn test_server_approval_isolated_per_session() {
    let dir = test_dir("sessions-approval");
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let a = registry.create_session().unwrap();
    let b = registry.create_session().unwrap();

    let waiter = tokio::spawn({
        let ch = a.channel.clone();
        async move {
            ch.request_approval("call-1", "bash", "run", None)
                .await
                .unwrap()
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = post(
        port,
        "/api/approval",
        &format!(
            r#"{{"call_id": "call-1", "approved": true, "session_id": "{}"}}"#,
            b.id
        ),
    );
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");

    let resp = post(
        port,
        "/api/approval",
        &format!(
            r#"{{"call_id": "call-1", "approved": true, "session_id": "{}"}}"#,
            a.id
        ),
    );
    assert!(resp.contains("\"resolved\":true"), "resp: {resp}");
    assert!(waiter.await.unwrap());

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// Re-checker P2-1: a session that exists on disk but has no live
/// entry yet (exactly the state after a daemon restart — the
/// server's registry starts empty) must accept prompts and SSE.
/// The file is seeded before the server starts, so the registry
/// has never seen the id.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_resumed_session_accepts_prompt_and_sse() {
    let dir = test_dir("sessions-resume");
    let store = SessionStore::new(&dir);
    store
        .append_message("purr_web_0123456789ab", &user_msg("hello before restart"))
        .unwrap();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let disp = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), Some(disp)).await;

    let resp = post(
        port,
        "/api/prompt",
        r#"{"prompt": "jawab satu kata: apel", "session_id": "purr_web_0123456789ab"}"#,
    );
    assert!(resp.contains("\"status\":\"dispatched\""), "resp: {resp}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(
        calls[0],
        (
            "purr_web_0123456789ab".to_string(),
            "jawab satu kata: apel".to_string()
        )
    );
    drop(calls);

    // SSE to the resumed session streams (200), it is not a 404.
    let out = http_sse_read(
        port,
        "/api/events?session=purr_web_0123456789ab",
        Duration::from_secs(1),
    );
    assert!(out.starts_with("HTTP/1.1 200 OK"), "out: {out}");

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// Re-checker P3-3: a present-but-non-string `session_id` is a
/// 400 on prompt and resolves nothing on approval — it must not
/// silently fall back to the default session.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_nonstring_session_id_is_not_default() {
    let dir = test_dir("sessions-nonstr");
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let disp = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), Some(disp)).await;

    let resp = post(
        port,
        "/api/prompt",
        r#"{"prompt": "jawab satu kata: apel", "session_id": 123}"#,
    );
    assert!(resp.starts_with("HTTP/1.1 400"), "resp: {resp}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(calls.lock().unwrap().is_empty(), "nothing dispatched");

    let resp = post(
        port,
        "/api/approval",
        r#"{"call_id": "call-1", "approved": true, "session_id": 123}"#,
    );
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B2: a request body over the 1 MiB limit is a 413. The
/// hand-rolled server counted headers + body; the Axum limit layer
/// counts the body only — the observable contract (oversize body →
/// 413) is the same, and this test locks it.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_oversize_body_is_413() {
    let dir = test_dir("oversize");
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let big = "x".repeat(1024 * 1024 + 1024);
    let body = format!(r#"{{"prompt": "{}"}}"#, big);
    let resp = post(port, "/api/prompt", &body);
    assert!(
        resp.starts_with("HTTP/1.1 413"),
        "resp head: {}",
        &resp[..resp.len().min(60)]
    );
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B2: a large prompt whose headers and body arrive as two
/// TCP segments still dispatches. This replaces the R3 P1-2
/// coverage (the hand-rolled server once dropped such requests
/// with a single 8192-byte read); hyper's parser handles split
/// reads natively, and this test proves the Axum server does too.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_split_read_large_prompt_dispatches() {
    let dir = test_dir("split-read");
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let disp = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), Some(disp)).await;

    let prompt = "y".repeat(100_000);
    let body = format!(r#"{{"prompt": "{}"}}"#, prompt);
    let headers = format!(
        "POST /api/prompt HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(headers.as_bytes()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    stream.write_all(body.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    assert!(
        buf.contains("\"status\":\"dispatched\""),
        "resp: {}",
        &buf[..buf.len().min(200)]
    );

    tokio::time::sleep(Duration::from_millis(200)).await;
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "calls: {calls:?}");
    assert_eq!(calls[0].1.len(), 100_000);

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// v0.4 B2 fix (re-checker P1): Ctrl-C must stop the server even
/// while a browser tab holds an SSE stream open. The old server
/// returned as soon as the accept loop broke; the first Axum
/// version waited on graceful shutdown forever because the SSE
/// stream never ends. The server task must COMPLETE (not just be
/// abortable) within 3s of cancel, with the SSE socket still held
/// open by the client — 3s is under the 5s backstop, so a pass
/// proves the stream ends on cancel rather than the backstop
/// firing.
#[tokio::test(flavor = "multi_thread")]
async fn test_server_cancel_completes_with_open_sse() {
    let dir = test_dir("cancel-sse");
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    // Hold an SSE stream open, like a browser tab does.
    let mut sse_stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    sse_stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    sse_stream
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    // Read the response headers so the stream is established.
    let mut header_buf = [0u8; 512];
    let _ = sse_stream.read(&mut header_buf);

    let _ = cancel_tx.send(true);
    // No handle.abort() here: the point is that run() RETURNS.
    let completed = tokio::time::timeout(Duration::from_secs(3), handle).await;
    assert!(
        completed.is_ok(),
        "server task did not complete after cancel with an open SSE stream"
    );
    let _ = std::fs::remove_dir_all(dir);
}
