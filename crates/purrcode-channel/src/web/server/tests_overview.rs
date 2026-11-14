// SPDX-License-Identifier: GPL-3.0-or-later

//! Overview endpoint tests (v0.4 B7): the aggregate shape, the
//! earliest-enabled next-cron pick, delivery statuses from the
//! JSONL tail, and graceful degradation (a corrupt store warns
//! here while the cron table still 500s — by design).

use super::test_support::*;
use serde_json::Value;

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn seed_jobs(dir: &std::path::Path, jobs_json: &str) {
    std::fs::create_dir_all(dir.join("cron")).unwrap();
    std::fs::write(dir.join("cron").join("jobs.jsonc"), jobs_json).unwrap();
}

fn seed_deliveries(dir: &std::path::Path, lines: &str) {
    std::fs::create_dir_all(dir.join("cron")).unwrap();
    std::fs::write(dir.join("cron").join("deliveries.jsonl"), lines).unwrap();
}

const JOBS: &str = r#"{"jobs": [
  {"id": "cron_disabled", "name": "mati", "schedule": {"display": "every morning at 06:00"}, "prompt": "x", "tools": ["read"], "next_run": "2026-11-14T06:00:00+07:00", "enabled": false},
  {"id": "cron_late", "name": "siang", "schedule": {"display": "every day at 12:00"}, "prompt": "x", "tools": ["read"], "next_run": "2026-11-14T12:00:00+07:00", "enabled": true},
  {"id": "cron_early", "name": "pagi", "schedule": {"display": "every morning at 08:00"}, "prompt": "x", "tools": ["read"], "next_run": "2026-11-14T08:00:00+07:00", "enabled": true}
]}"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_empty_state() {
    let dir = test_dir("overview-empty");
    let (port, _reg, cancel, handle) = start_server(None, dir, None).await;
    let body = get(port, "/api/overview");
    assert!(body.contains("200 OK"), "resp: {body}");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert_eq!(val["gateway"]["status"].as_str().unwrap(), "ok");
    assert_eq!(
        val["gateway"]["workspace"].as_str().unwrap(),
        "/tmp/ws",
        "test server workspace"
    );
    assert_eq!(val["sessions"]["active"].as_u64().unwrap(), 0);
    assert_eq!(val["sessions"]["items"].as_array().unwrap().len(), 0);
    assert!(val["next_cron"].is_null(), "no jobs yet: {val}");
    assert!(val["last_delivery"].is_null(), "no deliveries yet: {val}");
    assert_eq!(val["warnings"].as_array().unwrap().len(), 0);
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_next_cron_picks_earliest_enabled() {
    let dir = test_dir("overview-next-cron");
    seed_jobs(&dir, JOBS);
    let (port, _reg, cancel, handle) = start_server(None, dir, None).await;

    // One live session: the overview counts the registry, not files.
    let created = post(port, "/api/sessions", "{}");
    assert!(created.contains("201 Created"), "resp: {created}");
    let created_val: Value = serde_json::from_str(body_of(&created)).unwrap();
    let session_id = created_val["id"].as_str().unwrap().to_string();

    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    // The disabled 06:00 job must not win; the earliest *enabled*
    // job is the 08:00 one, not the 12:00 one.
    assert_eq!(val["next_cron"]["id"].as_str().unwrap(), "cron_early");
    assert_eq!(val["next_cron"]["name"].as_str().unwrap(), "pagi");
    assert_eq!(
        val["next_cron"]["next_run"].as_str().unwrap(),
        "2026-11-14T08:00:00+07:00"
    );
    assert_eq!(val["sessions"]["active"].as_u64().unwrap(), 1);
    assert_eq!(
        val["sessions"]["items"][0]["id"].as_str().unwrap(),
        session_id,
        "overview lists the session just created"
    );
    assert!(val["warnings"].as_array().unwrap().is_empty());
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_last_delivery_statuses() {
    let dir = test_dir("overview-delivery");
    seed_deliveries(
        &dir,
        "{\"type\":\"created\",\"id\":\"dlv_0000000001\",\"job_id\":\"cron_early\",\"job_name\":\"pagi\",\"created_at\":\"2026-11-14T07:00:00+07:00\",\"kind\":\"result\",\"summary\":\"ringkasan pagi\",\"detail\":\"...\"}\n\
         {\"type\":\"claimed\",\"delivery_id\":\"dlv_0000000001\"}\n\
         {\"type\":\"delivered\",\"delivery_id\":\"dlv_0000000001\"}\n\
         {\"type\":\"created\",\"id\":\"dlv_0000000002\",\"job_id\":\"cron_late\",\"job_name\":\"siang\",\"created_at\":\"2026-11-14T11:00:00+07:00\",\"kind\":\"failure\",\"summary\":\"gagal siang\",\"detail\":\"...\"}\n",
    );
    let (port, _reg, cancel, handle) = start_server(None, dir.clone(), None).await;
    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    // The last *created* delivery wins, even though an earlier one
    // was delivered; it is still pending (no claim/deliver after it).
    assert_eq!(
        val["last_delivery"]["id"].as_str().unwrap(),
        "dlv_0000000002"
    );
    assert_eq!(val["last_delivery"]["job_name"].as_str().unwrap(), "siang");
    assert_eq!(val["last_delivery"]["status"].as_str().unwrap(), "pending");

    // Claimed without a later delivered counts as delivered
    // (queue.rs: losing a delivery is safer than duplicating one).
    seed_deliveries(
        &dir,
        "{\"type\":\"created\",\"id\":\"dlv_0000000003\",\"job_id\":\"cron_early\",\"job_name\":\"pagi\",\"created_at\":\"2026-11-14T07:05:00+07:00\",\"kind\":\"heartbeat\",\"summary\":\"denyut\",\"detail\":\"...\"}\n\
         {\"type\":\"claimed\",\"delivery_id\":\"dlv_0000000003\"}\n",
    );
    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert_eq!(
        val["last_delivery"]["id"].as_str().unwrap(),
        "dlv_0000000003"
    );
    assert_eq!(
        val["last_delivery"]["status"].as_str().unwrap(),
        "delivered"
    );
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_degrades_on_corrupt_store() {
    let dir = test_dir("overview-corrupt");
    seed_jobs(&dir, "{ this is not json");
    let (port, _reg, cancel, handle) = start_server(None, dir, None).await;
    // The overview stays 200: the corrupt store is a warning,
    // not a failed response — the dashboard must still show the
    // sections that did load.
    let body = get(port, "/api/overview");
    assert!(body.contains("200 OK"), "resp: {body}");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert!(val["next_cron"].is_null());
    assert_eq!(val["gateway"]["status"].as_str().unwrap(), "ok");
    let warnings = val["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].as_str().unwrap().contains("invalid JSON"),
        "warnings: {warnings:?}"
    );
    // ...while the cron table itself still 500s on the same file
    // (B6 r1 P3-4: its job is to report store state, not to hide it).
    let cron = get(port, "/api/cron");
    assert!(cron.contains("500 Internal Server Error"), "resp: {cron}");
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_skips_corrupt_delivery_lines() {
    let dir = test_dir("overview-delivery-corrupt");
    seed_deliveries(
        &dir,
        "this line is not json\n\
         {\"type\":\"created\",\"id\":\"dlv_0000000009\",\"job_id\":\"cron_early\",\"job_name\":\"pagi\",\"created_at\":\"2026-11-14T07:00:00+07:00\",\"kind\":\"result\",\"summary\":\"tetap kebaca\",\"detail\":\"...\"}\n\
         {\"type\":\"bogus\",\"delivery_id\":\"dlv_0000000009\"}\n",
    );
    let (port, _reg, cancel, handle) = start_server(None, dir, None).await;
    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert_eq!(
        val["last_delivery"]["id"].as_str().unwrap(),
        "dlv_0000000009"
    );
    assert_eq!(
        val["last_delivery"]["status"].as_str().unwrap(),
        "pending",
        "unknown event types must not flip the status"
    );
    assert!(val["warnings"].as_array().unwrap().is_empty());
    let _ = cancel.send(true);
    handle.await.unwrap();
}
