// SPDX-License-Identifier: GPL-3.0-or-later

//! Overview endpoint (v0.4 B7): one read-only aggregate for the
//! dashboard canvas — gateway health, active sessions, the next
//! cron job, and the last cron delivery.
//!
//! Every section reuses an existing reader: the session registry
//! (the same list the sidebar shows), the shared cron projection
//! (`read_cron_jobs` — the overview never re-parses `jobs.jsonc`
//! itself), and the deliveries JSONL tail. A section that cannot
//! be read degrades to null and lands in `warnings` instead of
//! failing the whole response: the dashboard is a health view,
//! and a missing file must not hide the sections that did load.
//! (The cron *table* still 500s on a corrupt store — B6 r1 P3-4 —
//! because its job is to report store state; the overview's job
//! is to show what is known.)

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
    // can differ (hand edits to `jobs.jsonc`, or jobs written on
    // either side of a DST transition) — and RFC3339 strings with
    // different offsets do NOT sort chronologically (re-checker
    // P1-1, round 1). Compare parsed instants; a value that does
    // not parse falls back to lexicographic order so it is still
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

    // Last delivery: tail of `cron/deliveries.jsonl`.
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

/// Last delivery from `cron/deliveries.jsonl` (append-only JSONL,
/// one line per event — see `purrcode-service::cron::queue`).
/// The channel crate must not depend on the service crate (the
/// chain is one-way: service depends on channel), so this reads
/// the file directly, like the cron table reads `jobs.jsonc`.
/// A line that does not parse is skipped; a missing file means
/// no delivery yet (`Ok(None)`). Only a present-but-unreadable
/// file (permissions) is an `Err` — reported as a warning, never
/// a 500.
async fn read_last_delivery(purrcode_dir: &Path) -> Result<Option<Value>, String> {
    let path = purrcode_dir.join("cron").join("deliveries.jsonl");
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read deliveries file: {e}")),
    };
    let mut last: Option<Value> = None;
    let mut last_status = "pending";
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match val.get("type").and_then(|t| t.as_str()) {
            Some("created") => {
                // Delivery fields ride alongside the tag
                // (internally tagged enum in queue.rs).
                last = Some(val);
                last_status = "pending";
            }
            Some("claimed") | Some("delivered") => {
                // A claimed record without a later delivered is
                // treated as delivered (queue.rs): losing a
                // delivery is safer than duplicating a
                // possibly-completed send.
                let matches = val
                    .get("delivery_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| {
                        last.as_ref()
                            .and_then(|d| d.get("id"))
                            .and_then(|v| v.as_str())
                            == Some(id)
                    });
                if matches {
                    last_status = "delivered";
                }
            }
            _ => {}
        }
    }
    Ok(last.map(|delivery| {
        json!({
            "id": delivery.get("id").cloned().unwrap_or(Value::Null),
            "job_id": delivery.get("job_id").cloned().unwrap_or(Value::Null),
            "job_name": delivery.get("job_name").cloned().unwrap_or(Value::Null),
            "summary": delivery.get("summary").cloned().unwrap_or(Value::Null),
            "kind": delivery.get("kind").cloned().unwrap_or(Value::Null),
            "created_at": delivery.get("created_at").cloned().unwrap_or(Value::Null),
            "status": last_status,
        })
    }))
}
