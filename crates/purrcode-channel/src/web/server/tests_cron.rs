// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron scheduler endpoint tests (v0.4 B6): status mapping, JSON
//! shape, and audit events against a contract-faithful fake
//! registry — not the store's NL parsing or file writes (those
//! are covered by the CLI registry tests + E2E).

use super::test_support::*;
use purrcode_api::cron::{CronAddInput, CronError, CronJobView, CronRegistry, ParsedScheduleView};
use purrcode_api::tool::AuditSink;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Contract-faithful fake registry: an in-memory job map with
/// the same 404/409/400 contract as the real registry.
struct FakeRegistry {
    jobs: Mutex<HashMap<String, CronJobView>>,
}

impl FakeRegistry {
    fn new(entries: Vec<CronJobView>) -> Self {
        Self {
            jobs: Mutex::new(entries.into_iter().map(|j| (j.name.clone(), j)).collect()),
        }
    }
}

fn view(id: &str, name: &str, enabled: bool) -> CronJobView {
    CronJobView {
        id: id.to_string(),
        name: name.to_string(),
        enabled,
        schedule_display: "every morning at 08:00".to_string(),
        schedule_expr: Some("0 8 * * *".to_string()),
        once: false,
        prompt: "ringkas email".to_string(),
        tools: vec!["read".to_string()],
        preset_name: "tabby".to_string(),
        next_run: Some("2026-11-14T08:00:00+07:00".to_string()),
        last_run: None,
        run_count: 0,
        failure_count: 0,
        last_status: None,
    }
}

#[async_trait::async_trait]
impl CronRegistry for FakeRegistry {
    async fn parse_schedule(&self, schedule: &str) -> Result<ParsedScheduleView, CronError> {
        if schedule.trim().is_empty() || schedule.contains("blablabla") {
            return Err(CronError::InvalidInput(format!(
                "cannot parse schedule '{schedule}'"
            )));
        }
        Ok(ParsedScheduleView {
            expr: Some("0 8 * * *".to_string()),
            display: "every morning at 08:00".to_string(),
            once: false,
            next_run: "2026-11-14T08:00:00+07:00".to_string(),
        })
    }

    async fn add_job(&self, input: CronAddInput) -> Result<CronJobView, CronError> {
        if input.schedule.contains("blablabla") {
            return Err(CronError::InvalidInput("cannot parse schedule".to_string()));
        }
        if input.prompt.trim().is_empty() {
            return Err(CronError::InvalidInput(
                "cron job prompt must not be empty".to_string(),
            ));
        }
        let name = input.name.clone().unwrap_or_else(|| "job-auto".to_string());
        let mut map = self.jobs.lock().unwrap();
        if map.contains_key(&name) {
            return Err(CronError::AlreadyExists(name));
        }
        let job = view("cron_new000001", &name, true);
        map.insert(name, job.clone());
        Ok(job)
    }

    async fn set_enabled(&self, needle: &str, enabled: bool) -> Result<CronJobView, CronError> {
        let mut map = self.jobs.lock().unwrap();
        let job = map
            .values_mut()
            .find(|j| j.id == needle || j.id.starts_with(needle) || j.name == needle)
            .ok_or_else(|| CronError::NotFound(needle.to_string()))?;
        job.enabled = enabled;
        Ok(job.clone())
    }

    async fn remove_job(&self, needle: &str) -> Result<CronJobView, CronError> {
        let mut map = self.jobs.lock().unwrap();
        let key = map
            .values()
            .find(|j| j.id == needle || j.id.starts_with(needle) || j.name == needle)
            .map(|j| j.name.clone())
            .ok_or_else(|| CronError::NotFound(needle.to_string()))?;
        Ok(map.remove(&key).unwrap())
    }
}

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

fn seed_registry() -> Arc<FakeRegistry> {
    Arc::new(FakeRegistry::new(vec![
        view("cron_aaa000001", "pagi-ringkas", true),
        view("cron_bbb000002", "malam-ringkas", false),
    ]))
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn has_event(events: &AuditEvents, event: &str, name: &str, decision: &str) -> bool {
    events
        .lock()
        .unwrap()
        .iter()
        .any(|(e, n, d, _)| e == event && n == name && d == decision)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_parse_preview() {
    let dir = test_dir("cron-parse");
    let reg = seed_registry();
    let (port, _reg, cancel, handle) =
        start_server_with_cron_registry(None, dir, Some(reg), None).await;
    let body = post(
        port,
        "/api/cron/parse",
        r#"{"schedule": "tiap pagi jam 8"}"#,
    );
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert!(body.contains("200 OK"), "resp: {body}");
    assert_eq!(
        val["schedule"]["display"].as_str().unwrap(),
        "every morning at 08:00"
    );
    assert_eq!(
        val["schedule"]["next_run"].as_str().unwrap(),
        "2026-11-14T08:00:00+07:00"
    );

    // Unparsable schedule is a 400 with the error in the body.
    let bad = post(port, "/api/cron/parse", r#"{"schedule": "blablabla"}"#);
    assert!(bad.contains("400 Bad Request"), "resp: {bad}");
    assert!(bad.contains("cannot parse"));
    // Non-string schedule is a 400, not a silent default.
    let wrong_type = post(port, "/api/cron/parse", r#"{"schedule": 123}"#);
    assert!(wrong_type.contains("400 Bad Request"), "resp: {wrong_type}");
    // Missing schedule is a 400.
    let missing = post(port, "/api/cron/parse", r#"{}"#);
    assert!(missing.contains("400 Bad Request"), "resp: {missing}");
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_parse_nameless_failures_are_audited() {
    // Fix pass B6 r1 P3-5: a missing or non-string schedule
    // used to escape the audit (only failures carrying a
    // schedule string were recorded). A malformed body audits
    // as deny with "(unknown)" since fix pass B8 r1 P2-1 (see
    // test_cron_malformed_bodies_are_audited).
    let dir = test_dir("cron-parse-audit");
    let reg = seed_registry();
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel, handle) =
        start_server_with_cron_registry(None, dir, Some(reg), Some(Arc::new(sink))).await;
    let missing = post(port, "/api/cron/parse", r#"{}"#);
    assert!(missing.contains("400 Bad Request"), "resp: {missing}");
    let wrong_type = post(port, "/api/cron/parse", r#"{"schedule": 123}"#);
    assert!(wrong_type.contains("400 Bad Request"), "resp: {wrong_type}");
    let empty = post(port, "/api/cron/parse", r#"{"schedule": ""}"#);
    assert!(empty.contains("400 Bad Request"), "resp: {empty}");
    assert!(
        has_event(&events, "cron_parse", "(missing schedule)", "deny"),
        "missing-schedule deny missing: {:?}",
        events.lock().unwrap()
    );
    assert!(
        has_event(&events, "cron_parse", "123", "deny"),
        "non-string-schedule deny missing: {:?}",
        events.lock().unwrap()
    );
    assert!(
        has_event(&events, "cron_parse", "(empty schedule)", "deny"),
        "empty-schedule deny missing: {:?}",
        events.lock().unwrap()
    );
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_add_nonstr_name_and_toggle_nonstr_id_are_audited() {
    // Fix pass B6 r2 P3-1: `cron_add` with a non-string `name`
    // used to 400 without an audit entry (only string names
    // were attributed); the same gap existed in the needle
    // handlers (`needle_from_body` returned the 400 without
    // auditing). Both now attribute via the JSON rendering.
    let dir = test_dir("cron-nonstr-audit");
    let reg = seed_registry();
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel, handle) =
        start_server_with_cron_registry(None, dir, Some(reg), Some(Arc::new(sink))).await;
    let bad_name = post(
        port,
        "/api/cron/add",
        r#"{"name": 123, "schedule": "tiap pagi jam 8", "prompt": "x"}"#,
    );
    assert!(bad_name.contains("400 Bad Request"), "resp: {bad_name}");
    assert!(
        has_event(&events, "cron_add", "123", "deny"),
        "non-string-name deny missing: {:?}",
        events.lock().unwrap()
    );
    let bad_id = post(port, "/api/cron/enable", r#"{"id": 123}"#);
    assert!(bad_id.contains("400 Bad Request"), "resp: {bad_id}");
    assert!(
        has_event(&events, "cron_enable", "123", "deny"),
        "non-string-id deny missing: {:?}",
        events.lock().unwrap()
    );
    let missing_needle = post(port, "/api/cron/remove", r#"{}"#);
    assert!(
        missing_needle.contains("400 Bad Request"),
        "resp: {missing_needle}"
    );
    assert!(
        has_event(&events, "cron_remove", "(missing needle)", "deny"),
        "missing-needle deny missing: {:?}",
        events.lock().unwrap()
    );
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_add_enable_disable_remove_and_audit() {
    let dir = test_dir("cron-crud");
    let reg = seed_registry();
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel, handle) =
        start_server_with_cron_registry(None, dir, Some(reg), Some(Arc::new(sink))).await;

    // Add: 200 + the created job, audited allow.
    let body = post(
        port,
        "/api/cron/add",
        r#"{"name": "siang-ringkas", "schedule": "tiap pagi jam 8", "prompt": "ringkas email"}"#,
    );
    assert!(body.contains("\"job\""));
    assert!(body.contains("siang-ringkas"));
    assert!(has_event(&events, "cron_add", "siang-ringkas", "allow"));

    // Duplicate name is a 409, audited deny.
    let dup = post(
        port,
        "/api/cron/add",
        r#"{"name": "siang-ringkas", "schedule": "tiap pagi jam 8", "prompt": "lagi"}"#,
    );
    assert!(dup.contains("409 Conflict"), "resp: {dup}");
    assert!(has_event(&events, "cron_add", "siang-ringkas", "deny"));

    // Bad schedule is a 400, audited deny.
    let bad = post(
        port,
        "/api/cron/add",
        r#"{"name": "jelek", "schedule": "blablabla", "prompt": "x"}"#,
    );
    assert!(bad.contains("400 Bad Request"), "resp: {bad}");
    assert!(has_event(&events, "cron_add", "jelek", "deny"));

    // Non-string prompt is a 400, not a silent default.
    let wrong_type = post(
        port,
        "/api/cron/add",
        r#"{"name": "jelek2", "schedule": "tiap pagi jam 8", "prompt": 123}"#,
    );
    assert!(wrong_type.contains("400 Bad Request"), "resp: {wrong_type}");

    // Disable by name, then enable by id prefix — both 200.
    let dis = post(port, "/api/cron/disable", r#"{"name": "pagi-ringkas"}"#);
    assert!(dis.contains("\"enabled\":false"));
    assert!(has_event(&events, "cron_disable", "pagi-ringkas", "allow"));
    let en = post(port, "/api/cron/enable", r#"{"id": "cron_aaa"}"#);
    assert!(en.contains("\"enabled\":true"));

    // Unknown job is a 404, audited deny — and the message
    // wraps the needle exactly once (fix pass B6 r1 P3-1).
    let missing = post(port, "/api/cron/enable", r#"{"name": "tidak-ada"}"#);
    assert!(missing.contains("404 Not Found"), "resp: {missing}");
    assert!(
        missing.contains("no cron job matching 'tidak-ada'"),
        "resp: {missing}"
    );
    assert!(
        !missing.contains("no cron job matching 'no cron job"),
        "double-wrapped 404 message: {missing}"
    );
    assert!(has_event(&events, "cron_enable", "tidak-ada", "deny"));

    // Remove by id, then again → 404.
    let rm = post(port, "/api/cron/remove", r#"{"id": "cron_bbb000002"}"#);
    assert!(rm.contains("malam-ringkas"));
    assert!(has_event(&events, "cron_remove", "malam-ringkas", "allow"));
    let rm2 = post(port, "/api/cron/remove", r#"{"id": "cron_bbb000002"}"#);
    assert!(rm2.contains("404 Not Found"), "resp: {rm2}");
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_endpoints_503_without_registry() {
    let dir = test_dir("cron-503");
    let (port, _reg, cancel, handle) = start_server_with_cron_registry(None, dir, None, None).await;
    for path in [
        "/api/cron/parse",
        "/api/cron/add",
        "/api/cron/enable",
        "/api/cron/disable",
        "/api/cron/remove",
    ] {
        let body = post(
            port,
            path,
            r#"{"schedule": "x", "prompt": "y", "name": "z"}"#,
        );
        assert!(
            body.contains("503 Service Unavailable"),
            "{path} should be 503 without a registry: {body}"
        );
    }
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cron_malformed_bodies_are_audited() {
    // Fix pass B8 r1 P2-1: a malformed JSON body audits as deny
    // with "(unknown)" — the B5 r1 P3-3 carve-out ("nothing to
    // attribute") no longer stands; an authenticated client
    // probing with garbage leaves a trace. Covers parse, add,
    // and the needle handlers (enable/remove).
    let dir = test_dir("cron-malformed-audit");
    let reg = seed_registry();
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel, handle) =
        start_server_with_cron_registry(None, dir, Some(reg), Some(Arc::new(sink))).await;
    for (path, event) in [
        ("/api/cron/parse", "cron_parse"),
        ("/api/cron/add", "cron_add"),
        ("/api/cron/enable", "cron_enable"),
        ("/api/cron/remove", "cron_remove"),
    ] {
        let resp = post(port, path, "{not json");
        assert!(resp.contains("400 Bad Request"), "{path}: {resp}");
        assert!(
            has_event(&events, event, "(unknown)", "deny"),
            "{event} malformed deny missing: {:?}",
            events.lock().unwrap()
        );
    }
    let _ = cancel.send(true);
    handle.await.unwrap();
}
