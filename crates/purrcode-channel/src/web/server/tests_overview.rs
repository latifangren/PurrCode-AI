// SPDX-License-Identifier: GPL-3.0-or-later

//! Overview endpoint tests (v0.4 B7): the aggregate shape, the
//! earliest-enabled next-cron pick, delivery statuses from the
//! `deliveries` table, and graceful degradation (a corrupt store warns
//! here while the cron table still 500s — by design).

use super::test_support::*;
use purrcode_store::rusqlite::params;
use serde_json::Value;

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// One cron job row to seed (ADR-015: the channel reads `cron_jobs`,
/// it never parses a jobs file anymore).
struct JobSeed {
    id: &'static str,
    name: &'static str,
    display: &'static str,
    next_run: &'static str,
    enabled: bool,
}

fn seed_jobs(dir: &std::path::Path, jobs: &[JobSeed]) {
    let store = purrcode_store::Store::open(dir).unwrap();
    let guard = store.lock().unwrap();
    guard.execute("DELETE FROM cron_jobs", []).unwrap();
    for job in jobs {
        guard
            .execute(
                "INSERT INTO cron_jobs
                   (id, name, schedule_display, prompt, tools_json, preset_json,
                    delivery_json, enabled, created_at, next_run_at)
                 VALUES (?1, ?2, ?3, 'x', '[\"read\"]', 'null', 'null', ?4,
                         '2026-11-14T00:00:00+07:00', ?5)",
                params![
                    job.id,
                    job.name,
                    job.display,
                    i64::from(job.enabled),
                    job.next_run
                ],
            )
            .unwrap();
    }
}

/// One delivery row to seed. Seeding replaces the whole table, like
/// the old file-based seed replaced the whole JSONL file.
struct DeliverySeed {
    id: &'static str,
    job_id: &'static str,
    job_name: &'static str,
    created_at: &'static str,
    kind: &'static str,
    summary: &'static str,
    status: &'static str,
}

fn seed_deliveries(dir: &std::path::Path, deliveries: &[DeliverySeed]) {
    let store = purrcode_store::Store::open(dir).unwrap();
    let guard = store.lock().unwrap();
    guard.execute("DELETE FROM deliveries", []).unwrap();
    for d in deliveries {
        guard
            .execute(
                "INSERT INTO deliveries
                   (id, job_id, job_name, kind, status, summary, detail, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7)",
                params![
                    d.id,
                    d.job_id,
                    d.job_name,
                    d.kind,
                    d.status,
                    d.summary,
                    d.created_at
                ],
            )
            .unwrap();
    }
}

const JOBS: &[JobSeed] = &[
    JobSeed {
        id: "cron_disabled",
        name: "mati",
        display: "every morning at 06:00",
        next_run: "2026-11-14T06:00:00+07:00",
        enabled: false,
    },
    JobSeed {
        id: "cron_late",
        name: "siang",
        display: "every day at 12:00",
        next_run: "2026-11-14T12:00:00+07:00",
        enabled: true,
    },
    JobSeed {
        id: "cron_early",
        name: "pagi",
        display: "every morning at 08:00",
        next_run: "2026-11-14T08:00:00+07:00",
        enabled: true,
    },
];

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
    assert_eq!(val["sessions"]["count"].as_u64().unwrap(), 0);
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
    assert_eq!(val["sessions"]["count"].as_u64().unwrap(), 1);
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
async fn test_overview_next_cron_mixed_offsets() {
    // Re-checker P1-1, round 1: RFC3339 strings with different
    // offsets do NOT sort chronologically — "2026-11-15T01:00:00
    // +00:00" sorts before "2026-11-15T07:00:00+07:00" as text,
    // but 01:00Z is an hour AFTER 00:00Z. The pick must compare
    // parsed instants, not strings.
    let dir = test_dir("overview-mixed-offsets");
    seed_jobs(
        &dir,
        &[
            JobSeed {
                id: "cron_x",
                name: "x-wib",
                display: "07:00 WIB",
                next_run: "2026-11-15T07:00:00+07:00",
                enabled: true,
            },
            JobSeed {
                id: "cron_y",
                name: "y-utc",
                display: "01:00 UTC",
                next_run: "2026-11-15T01:00:00+00:00",
                enabled: true,
            },
        ],
    );
    let (port, _reg, cancel, handle) = start_server(None, dir.clone(), None).await;
    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert_eq!(
        val["next_cron"]["id"].as_str().unwrap(),
        "cron_x",
        "00:00Z is earlier than 01:00Z: {val}"
    );

    // DST-style pair: same wall-clock time, different offsets —
    // the +02:00 one (06:00Z) is earlier than the +01:00 one (07:00Z).
    seed_jobs(
        &dir,
        &[
            JobSeed {
                id: "cron_winter",
                name: "winter",
                display: "08:00 +01",
                next_run: "2026-11-15T08:00:00+01:00",
                enabled: true,
            },
            JobSeed {
                id: "cron_summer",
                name: "summer",
                display: "08:00 +02",
                next_run: "2026-11-15T08:00:00+02:00",
                enabled: true,
            },
        ],
    );
    let body = get(port, "/api/overview");
    let val: Value = serde_json::from_str(body_of(&body)).unwrap();
    assert_eq!(
        val["next_cron"]["id"].as_str().unwrap(),
        "cron_summer",
        "06:00Z is earlier than 07:00Z: {val}"
    );
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_last_delivery_statuses() {
    let dir = test_dir("overview-delivery");
    seed_deliveries(
        &dir,
        &[
            DeliverySeed {
                id: "dlv_0000000001",
                job_id: "cron_early",
                job_name: "pagi",
                created_at: "2026-11-14T07:00:00+07:00",
                kind: "result",
                summary: "ringkasan pagi",
                status: "delivered",
            },
            DeliverySeed {
                id: "dlv_0000000002",
                job_id: "cron_late",
                job_name: "siang",
                created_at: "2026-11-14T11:00:00+07:00",
                kind: "failure",
                summary: "gagal siang",
                status: "pending",
            },
        ],
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
        &[DeliverySeed {
            id: "dlv_0000000003",
            job_id: "cron_early",
            job_name: "pagi",
            created_at: "2026-11-14T07:05:00+07:00",
            kind: "heartbeat",
            summary: "denyut",
            status: "claimed",
        }],
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
    let (port, _reg, cancel, handle) = start_server(None, dir.clone(), None).await;
    // Corruption introduced AFTER startup (a row with unparseable
    // `tools_json`): the registry opens the store at startup and
    // fails closed, so a garbage `purrcode.db` would keep the server
    // from starting at all — the overview's degrade path is what
    // this test covers.
    {
        let store = purrcode_store::Store::open(&dir).unwrap();
        let guard = store.lock().unwrap();
        guard
            .execute(
                "INSERT INTO cron_jobs
                   (id, name, schedule_display, prompt, tools_json, preset_json,
                    delivery_json, enabled, created_at)
                 VALUES ('cron_bad', 'bad', 'hourly', 'hi', '{not json', 'null',
                         'null', 1, '2026-11-14T00:00:00+07:00')",
                [],
            )
            .unwrap();
    }
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
        warnings[0].as_str().unwrap().contains("corrupt cron store"),
        "warnings: {warnings:?}"
    );
    // ...while the cron table itself still 500s on the same store
    // (B6 r1 P3-4: its job is to report store state, not to hide it).
    let cron = get(port, "/api/cron");
    assert!(cron.contains("500 Internal Server Error"), "resp: {cron}");
    let _ = cancel.send(true);
    handle.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_overview_skips_corrupt_delivery_rows() {
    // The SQL equivalent of the old corrupt-JSONL-line test: a row
    // with an unparseable kind (direct DB tampering) is skipped and
    // the latest *valid* delivery still shows; the overview stays
    // warning-free.
    let dir = test_dir("overview-delivery-corrupt");
    seed_deliveries(
        &dir,
        &[
            DeliverySeed {
                id: "dlv_0000000009",
                job_id: "cron_early",
                job_name: "pagi",
                created_at: "2026-11-14T07:00:00+07:00",
                kind: "result",
                summary: "tetap kebaca",
                status: "pending",
            },
            DeliverySeed {
                id: "dlv_bad0000001",
                job_id: "cron_early",
                job_name: "pagi",
                created_at: "2026-11-14T08:00:00+07:00",
                kind: "bogus",
                summary: "baris rusak",
                status: "pending",
            },
        ],
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
        "a row with an unknown kind must not become the last delivery"
    );
    assert!(val["warnings"].as_array().unwrap().is_empty());
    let _ = cancel.send(true);
    handle.await.unwrap();
}
