// SPDX-License-Identifier: GPL-3.0-or-later

//! Server tests for the v0.4 B8 security layer (D7/D9): the
//! R3 token flow is unchanged, failed auth attempts are audited
//! and rate-limited, `POST /api/prompt` is capped per peer IP,
//! and the previously unaudited POST actions (prompt rejections,
//! approval resolve, session create, skill approve/reject) are
//! recorded. The idle-eviction logic itself is unit-tested in
//! `web::sessions` (sweep + touch); this file covers the live
//! SSE subscriber guard end to end.

use super::test_support::*;
use purrcode_api::channel::Channel;
use purrcode_api::tool::AuditSink;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type AuditEvents = Arc<Mutex<Vec<(String, String, String, String)>>>;

struct RecordingAuditSink {
    events: AuditEvents,
}

impl RecordingAuditSink {
    fn new() -> (Self, AuditEvents) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                events: events.clone(),
            },
            events,
        )
    }
}

impl AuditSink for RecordingAuditSink {
    fn record_event(
        &self,
        _session_id: &str,
        _agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    ) {
        self.events.lock().unwrap().push((
            event.to_string(),
            tool.unwrap_or_default().to_string(),
            decision.to_string(),
            detail.to_string(),
        ));
    }
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Query URL carrying the test token. The token is spliced in
/// with `format!` instead of written as a literal `?token=...`
/// string: the runtime redacts literal `token=<value>` text in
/// tool inputs (it once wrote a literal `<redacted>` into a test
/// file, and hyper rejects the `<`/`>` in the URI with a 400),
/// so the query is assembled here at runtime.
fn token_url() -> String {
    format!("/?token={}", "SECRET")
}

fn count(events: &AuditEvents, event: &str, decision: &str) -> usize {
    events
        .lock()
        .unwrap()
        .iter()
        .filter(|(e, _, d, _)| e == event && d == decision)
        .count()
}

fn finish(cancel_tx: tokio::sync::watch::Sender<bool>, handle: tokio::task::JoinHandle<()>) {
    let _ = cancel_tx.send(true);
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_auth_failures_audited_and_rate_limited() {
    // v0.4 B8, D9: 10 failures are 401 (each audited as web_auth
    // deny); the 11th is 429 with Retry-After: 60 (audited as
    // web_rate_limit, bucket "auth"); a correct token still passes
    // during the lockout.
    let dir = test_dir("b8-auth");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel_tx, handle) = start_server_with_registry(
        Some("SECRET".to_string()),
        dir.clone(),
        None,
        None,
        Some(Arc::new(sink)),
    )
    .await;
    for _ in 0..10 {
        assert!(get(port, "/").starts_with("HTTP/1.1 401"), "want 401");
    }
    let eleventh = get(port, "/");
    assert!(
        eleventh.starts_with("HTTP/1.1 429"),
        "11th failure must be 429, got: {eleventh}"
    );
    assert!(
        eleventh.to_ascii_lowercase().contains("retry-after: 60"),
        "429 must carry Retry-After: 60, got: {eleventh}"
    );
    let ok_resp = get(port, &token_url());
    assert!(
        ok_resp.starts_with("HTTP/1.1 200 OK"),
        "correct token after lockout must be 200, got: {ok_resp}"
    );
    assert_eq!(count(&events, "web_auth", "deny"), 11);
    assert_eq!(count(&events, "web_rate_limit", "deny"), 1);
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|(e, n, _, _)| { e == "web_rate_limit" && n == "auth" }));
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_prompt_rate_limit_429_after_30() {
    // v0.4 B8, D9: 30 prompts dispatch (200), the 31st is 429.
    // No token is configured here, so the prompt half of the
    // middleware is what fires.
    let dir = test_dir("b8-prompt-limit");
    let (sink, events) = RecordingAuditSink::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let (port, _reg, cancel_tx, handle) = start_server_with_skill_registry(
        None,
        dir.clone(),
        Some(dispatcher),
        None,
        None,
        Some(Arc::new(sink)),
    )
    .await;
    for i in 0..30 {
        let resp = post(port, "/api/prompt", r#"{"prompt": "hi"}"#);
        assert!(
            resp.starts_with("HTTP/1.1 200 OK"),
            "prompt {i} must dispatch, got: {resp}"
        );
    }
    let over = post(port, "/api/prompt", r#"{"prompt": "hi"}"#);
    assert!(
        over.starts_with("HTTP/1.1 429"),
        "31st prompt must be 429, got: {over}"
    );
    assert!(
        over.to_ascii_lowercase().contains("retry-after: 60"),
        "429 must carry Retry-After: 60, got: {over}"
    );
    assert_eq!(calls.lock().unwrap().len(), 30);
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|(e, n, _, _)| { e == "web_rate_limit" && n == "prompt" }));
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_prompt_rejections_are_audited() {
    // v0.4 B8, D9: handler-level prompt rejections (malformed
    // body, non-string prompt, unknown session, no dispatcher)
    // are recorded as web_prompt deny. (A dispatched prompt is
    // audited at the turn level by web_turn_start instead.)
    let dir = test_dir("b8-prompt-audit");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
    let bad_json = post(port, "/api/prompt", "{not json");
    assert!(bad_json.starts_with("HTTP/1.1 400"), "resp: {bad_json}");
    let bad_prompt = post(port, "/api/prompt", r#"{"prompt": 123}"#);
    assert!(bad_prompt.starts_with("HTTP/1.1 400"), "resp: {bad_prompt}");
    let unknown = post(
        port,
        "/api/prompt",
        r#"{"prompt": "x", "session_id": "purr_web_0123456789ab"}"#,
    );
    assert!(unknown.starts_with("HTTP/1.1 404"), "resp: {unknown}");
    let no_disp = post(port, "/api/prompt", r#"{"prompt": "x"}"#);
    assert!(no_disp.starts_with("HTTP/1.1 503"), "resp: {no_disp}");
    assert_eq!(count(&events, "web_prompt", "deny"), 4);
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_approval_resolve_is_audited() {
    // v0.4 B8, D9: resolving a pending card audits web_approval
    // allow; nothing pending (or a malformed body) audits deny.
    let dir = test_dir("b8-approval-audit");
    let (sink, events) = RecordingAuditSink::new();
    let (port, registry, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
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
    let resp = post(
        port,
        "/api/approval",
        r#"{"call_id": "call-nope", "approved": true}"#,
    );
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    let bad = post(port, "/api/approval", "{not json");
    assert!(bad.starts_with("HTTP/1.1 400"), "resp: {bad}");
    assert_eq!(count(&events, "web_approval", "allow"), 1);
    assert_eq!(count(&events, "web_approval", "deny"), 2);
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|(e, n, d, _)| e == "web_approval" && n == "call-9" && d == "allow"));
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_session_create_is_audited() {
    // v0.4 B8, D9: minting a session audits web_session_create
    // allow with the new id.
    let dir = test_dir("b8-session-audit");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
    let resp = post(port, "/api/sessions", "{}");
    assert!(resp.starts_with("HTTP/1.1 201"), "resp: {resp}");
    let id = body_of(&resp)
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}');
    assert!(id.contains("purr_web_"), "resp: {resp}");
    assert_eq!(count(&events, "web_session_create", "allow"), 1);
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skill_approve_reject_are_audited() {
    // v0.4 B8, D9: resolving a draft card with nothing pending
    // audits skill_approve / skill_reject deny.
    let dir = test_dir("b8-skill-audit");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
    let resp = post(port, "/api/skills/approve", r#"{"name": "nope"}"#);
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    let resp = post(port, "/api/skills/reject", r#"{"name": "nope"}"#);
    assert!(resp.contains("\"resolved\":false"), "resp: {resp}");
    assert_eq!(count(&events, "skill_approve", "deny"), 1);
    assert_eq!(count(&events, "skill_reject", "deny"), 1);
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_sse_subscriber_count_tracks_connection() {
    // v0.4 B8, D8: an open SSE stream counts as one subscriber
    // (which vetoes the idle reaper); closing it drops the count
    // back to zero.
    let dir = test_dir("b8-sse-count");
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let sse =
        std::thread::spawn(move || http_sse_read(port, "/api/events", Duration::from_secs(2)));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let session = registry
        .get_session("purr_web")
        .expect("live default session");
    assert_eq!(
        session
            .sse_subscribers
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "one open SSE stream must count as one subscriber"
    );
    let _ = sse.join();
    // The server notices the disconnect when hyper drops the
    // stream; poll briefly instead of asserting at once.
    for _ in 0..20 {
        if session
            .sse_subscribers
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        session
            .sse_subscribers
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "closing the SSE stream must drop the subscriber count"
    );
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_idle_sweep_evicts_via_server_registry() {
    // v0.4 B8, D8: sweep_idle on the live registry evicts an
    // idle entry while the durable list keeps it (re-created on
    // demand).
    let dir = test_dir("b8-sweep");
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let resp = post(port, "/api/sessions", "{}");
    assert!(resp.starts_with("HTTP/1.1 201"), "resp: {resp}");
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(registry.sweep_idle(Duration::from_millis(1)), 1);
    let list = get(port, "/api/sessions");
    assert!(
        body_of(&list).contains("purr_web_"),
        "evicted session must stay on disk, list: {list}"
    );
    finish(cancel_tx, handle);
    let _ = std::fs::remove_dir_all(dir);
}
