// SPDX-License-Identifier: GPL-3.0-or-later

//! Overview endpoint (v0.4 B7): one read-only aggregate for the
//! dashboard canvas — gateway health, sessions, the next cron
//! job, and the last cron delivery.
//!
//! Every section reuses an existing reader: the session registry
//! (the same list the sidebar shows), the shared cron projection
//! (`read_cron_jobs` — the overview never re-reads the store
//! itself), and the latest row of the `deliveries` table. A section
//! that cannot be read degrades to null and lands in `warnings`
//! instead of failing the whole response: the dashboard is a health
//! view, and a missing database must not hide the sections that did
//! load. (The cron *table* still 500s on a corrupt store — B6 r1
//! P3-4 — because its job is to report store state; the overview's
//! job is to show what is known.)

use super::routes::read_cron_jobs;
use super::WebState;
use axum::{extract::State, http::StatusCode, Json};
use chrono::DateTime;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Sort key for a `next_run` value: the parsed instant when the
/// value is valid RFC3339, otherwise the raw string. Parsed
/// instants sort before unparsed strings (a real time beats a
/// malformed one); two unparsed strings sort lexicographically.
/// Derived `Ord` on the enum gives exactly that: variants order
/// by declaration, instants by instant.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum NextRunKey {
    Instant(DateTime<chrono::FixedOffset>),
    Raw(String),
}

impl NextRunKey {
    fn from(value: &str) -> Self {
        match DateTime::parse_from_rfc3339(value) {
            Ok(instant) => Self::Instant(instant),
            Err(_) => Self::Raw(value.to_string()),
        }
    }
}

/// `GET /api/overview`.
pub async fn overview(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    let mut warnings: Vec<String> = Vec::new();

    // Sessions: live registry entries (same list the sidebar shows).
    let sessions = state.registry.list_sessions();

    // Next cron: the enabled job with the earliest `next_run`.
    // `next_run` values are RFC3339 timestamps, but their offsets
    // can differ (jobs written on either side of a DST transition)
    // — and RFC3339 strings with different offsets do NOT sort
    // chronologically (re-checker P1-1, round 1). Compare parsed
    // instants; a value that does not parse falls back to
    // lexicographic order so it is still
    // comparable, never skipped.
    let mut next_cron: Option<Value> = None;
    match read_cron_jobs(&state.purrcode_dir).await {
        Ok(jobs) => {
            let mut best: Option<(NextRunKey, &Value)> = None;
            for job in &jobs {
                if job.get("enabled").and_then(|e| e.as_bool()) != Some(true) {
                    continue;
                }
                let Some(next_run) = job.get("next_run").and_then(|v| v.as_str()) else {
                    continue;
                };
                let key = NextRunKey::from(next_run);
                let earlier = match &best {
                    Some((best_key, _)) => key < *best_key,
                    None => true,
                };
                if earlier {
                    best = Some((key, job));
                }
            }
            next_cron = best.map(|(_, job)| {
                json!({
                    "id": job.get("id").cloned().unwrap_or(Value::Null),
                    "name": job.get("name").cloned().unwrap_or(Value::Null),
                    "schedule": job.get("schedule").cloned().unwrap_or(Value::Null),
                    "next_run": job.get("next_run").cloned().unwrap_or(Value::Null),
                })
            });
        }
        Err(msg) => warnings.push(msg),
    }

    // Last delivery: newest valid row of the `deliveries` table.
    let last_delivery = match read_last_delivery(&state.purrcode_dir).await {
        Ok(delivery) => delivery,
        Err(msg) => {
            warnings.push(msg);
            None
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "gateway": {
                "status": "ok",
                "workspace": state.workspace_root.display().to_string(),
            },
            "sessions": {
                "count": sessions.len(),
                "items": sessions,
            },
            "next_cron": next_cron,
            "last_delivery": last_delivery,
            "warnings": warnings,
        })),
    )
}

/// Last delivery from the `deliveries` table (one row per delivery
/// — see `purrcode-service::cron::queue`). The channel crate must
/// not depend on the service crate (the chain is one-way: service
/// depends on channel), so this queries the store directly, like
/// the cron table reads `cron_jobs`. The query runs on a blocking
/// thread: SQLite calls must not stall the async runtime. A
/// missing database means no delivery yet (`Ok(None)`). Only a
/// present-but-unreadable store is an `Err` — reported as a
/// warning, never a 500.
async fn read_last_delivery(purrcode_dir: &Path) -> Result<Option<Value>, String> {
    let dir = purrcode_dir.to_path_buf();
    tokio::task::spawn_blocking(move || read_last_delivery_sync(&dir))
        .await
        .map_err(|e| format!("last delivery read task failed: {e}"))?
}

fn read_last_delivery_sync(purrcode_dir: &Path) -> Result<Option<Value>, String> {
    use purrcode_store::{sql, Store};

    let store =
        Store::open(purrcode_dir).map_err(|e| format!("cannot open delivery store: {e}"))?;
    let guard = store
        .lock()
        .map_err(|e| format!("cannot lock delivery store: {e}"))?;
    let mut stmt = sql(guard.prepare(
        "SELECT id, job_id, job_name, summary, kind, created_at, status
         FROM deliveries ORDER BY created_at DESC, rowid DESC",
    ))
    .map_err(|e| format!("cannot read last delivery: {e}"))?;
    let rows = sql(stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    }))
    .map_err(|e| format!("cannot read last delivery: {e}"))?;
    // The newest *valid* row wins: a row with an unknown kind or
    // status is skipped (the queue's `list_all` does the same),
    // instead of letting a corrupt row become the dashboard's last
    // delivery.
    for row in rows {
        let (id, job_id, job_name, summary, kind, created_at, status) =
            row.map_err(|e| format!("cannot read last delivery: {e}"))?;
        if !matches!(kind.as_str(), "result" | "failure" | "heartbeat") {
            continue;
        }
        if !matches!(status.as_str(), "pending" | "claimed" | "delivered") {
            continue;
        }
        // A claimed delivery without a later delivered is treated
        // as delivered (queue.rs): losing a delivery is safer than
        // duplicating a possibly-completed send.
        let status = if status == "claimed" {
            "delivered"
        } else {
            status.as_str()
        };
        return Ok(Some(json!({
            "id": id,
            "job_id": job_id,
            "job_name": job_name,
            "summary": summary,
            "kind": kind,
            "created_at": created_at,
            "status": status,
        })));
    }
    Ok(None)
}
