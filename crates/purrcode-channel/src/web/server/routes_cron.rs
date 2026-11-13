// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron scheduler endpoints (v0.4 B6).
//!
//! The read side (`GET /api/cron`, the plain job table) stays
//! in `routes.rs`. B6 adds the write side, all routed through
//! the `CronRegistry` trait — which the CLI implements on top
//! of the service cron store, reusing the CLI's `cron add` path
//! (same NL parser, same preset pinning, same allowlist
//! validation) — so these handlers never touch `jobs.jsonc`
//! themselves:
//!
//! * `POST /api/cron/parse {"schedule"}` — parse preview, no
//!   side effects (the web equivalent of the CLI's interactive
//!   confirm: the user sees what was understood first).
//! * `POST /api/cron/add {"name"?, "schedule", "prompt",
//!   "allow"?, "preset"?}` — create a job. 409 on a duplicate
//!   name, 400 on an unparsable schedule, a God/yolo preset, or
//!   a forbidden tool.
//! * `POST /api/cron/enable|disable|remove {"id"|"name"}` —
//!   flip the flag or delete the job. 404 when no job matches.
//!
//! Without a wired registry every endpoint answers 503, like
//! the B4 plugin and B5 skill endpoints (a 200 with an empty
//! result would hide a miswired server). Failed attempts audit
//! with decision "deny", successes with "allow" — the same rule
//! as plugin mount and skill writes.

use super::routes::record_admin;
use super::WebState;
use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use purrcode_api::cron::{CronAddInput, CronError};
use serde_json::{json, Value};
use std::sync::Arc;

fn cron_error_status(e: &CronError) -> StatusCode {
    match e {
        CronError::NotFound(_) => StatusCode::NOT_FOUND,
        CronError::AlreadyExists(_) => StatusCode::CONFLICT,
        CronError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        CronError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn no_registry() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "cron registry not configured" })),
    )
}

/// Hand-parsed body, same style as the other POST endpoints:
/// no `Content-Type` requirement, malformed JSON is a 400.
/// A field present with the wrong JSON type is a 400 too — a
/// non-string `schedule` must not silently become a default
/// (same rule as the B5 memory `file` fix, r1 P3-4).
fn str_field(val: &Value, key: &str) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let err = |msg: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })));
    match val.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(err(format!("{key} must be a string"))),
    }
}

fn required_str_field(val: &Value, key: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let err = |msg: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })));
    match val.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(err(format!("{key} must not be empty"))),
        Some(_) => Err(err(format!("{key} must be a string"))),
        None => Err(err(format!("{key} is required"))),
    }
}

/// `allow`: absent/null → `None` (Meowstro read-only default);
/// an array of strings → the explicit allowlist (empty array =
/// explicitly empty, fail-closed); anything else is a 400.
fn allow_field(val: &Value) -> Result<Option<Vec<String>>, (StatusCode, Json<Value>)> {
    let err = |msg: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })));
    match val.get("allow") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut tools = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(s) => tools.push(s.to_string()),
                    None => return Err(err("allow must be an array of strings".to_string())),
                }
            }
            Ok(Some(tools))
        }
        Some(_) => Err(err("allow must be an array of strings".to_string())),
    }
}

/// The job needle for enable/disable/remove: `id` or `name`,
/// either key, must be a non-empty string.
fn needle_from_body(body: &Bytes) -> Result<String, (StatusCode, Json<Value>)> {
    let err = |msg: &str| (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })));
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        return Err(err("malformed JSON body"));
    };
    for key in ["id", "name"] {
        if let Some(s) = val.get(key).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return Ok(s.to_string());
            }
        }
    }
    Err(err("id or name is required"))
}

/// Audit a rejected cron request when it names the job (the B5
/// r1 P3-3 rule): a refusal the client can attribute to a job
/// belongs in the audit log; only a nameless or malformed body
/// has nothing to attribute.
fn audit_rejected(state: &Arc<WebState>, event: &str, name: &str, err: &(StatusCode, Json<Value>)) {
    let detail = err
        .1
         .0
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("rejected request body");
    record_admin(state, event, name, "deny", detail);
}

/// `POST /api/cron/parse {"schedule"}` — parse preview, no
/// side effects. 400 when the schedule does not parse.
pub async fn cron_parse(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.cron_registry else {
        return no_registry();
    };
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed JSON body" })),
        );
    };
    let schedule = match required_str_field(&val, "schedule") {
        Ok(s) => s,
        Err(e) => {
            // Audit every refusal (fix pass B6 r1 P3-5): attribute
            // to the schedule value when one is present — a
            // non-string value audits as its JSON rendering, a
            // missing one as "(missing schedule)", an empty one
            // as "(empty schedule)". Only a malformed body has
            // nothing to attribute, and stays unaudited (the B5
            // r1 P3-3 rule).
            let raw_name: String = match val.get("schedule") {
                Some(Value::String(s)) => s.chars().take(80).collect(),
                Some(other) => serde_json::to_string(other)
                    .unwrap_or_else(|_| "(invalid schedule)".to_string()),
                None => "(missing schedule)".to_string(),
            };
            let name = if raw_name.is_empty() {
                "(empty schedule)".to_string()
            } else {
                raw_name
            };
            audit_rejected(&state, "cron_parse", &name, &e);
            return e;
        }
    };
    match reg.parse_schedule(&schedule).await {
        Ok(parsed) => (StatusCode::OK, Json(json!({ "schedule": parsed }))),
        Err(e) => {
            let err = (
                cron_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            );
            audit_rejected(&state, "cron_parse", &schedule, &err);
            err
        }
    }
}

/// `POST /api/cron/add {"name"?, "schedule", "prompt",
/// "allow"?, "preset"?}` — create a job through the CLI's
/// `cron add` path. 409 on a duplicate name; 400 on an
/// unparsable schedule, a God/yolo preset, or a forbidden tool.
pub async fn cron_add(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.cron_registry else {
        return no_registry();
    };
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed JSON body" })),
        );
    };
    let input = match parse_add_body(&val) {
        Ok(input) => input,
        Err(e) => {
            if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                audit_rejected(&state, "cron_add", name, &e);
            }
            return e;
        }
    };
    let job_name = input.name.clone().unwrap_or_else(|| "(auto)".to_string());
    match reg.add_job(input).await {
        Ok(job) => {
            record_admin(
                &state,
                "cron_add",
                &job.name,
                "allow",
                &format!(
                    "schedule '{}', next run {}",
                    job.schedule_display,
                    job.next_run.as_deref().unwrap_or("—")
                ),
            );
            (StatusCode::OK, Json(json!({ "job": job })))
        }
        Err(e) => {
            let err = (
                cron_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            );
            audit_rejected(&state, "cron_add", &job_name, &err);
            err
        }
    }
}

fn parse_add_body(val: &Value) -> Result<CronAddInput, (StatusCode, Json<Value>)> {
    let name = str_field(val, "name")?;
    let schedule = required_str_field(val, "schedule")?;
    let prompt = required_str_field(val, "prompt")?;
    let allow = allow_field(val)?;
    let preset = str_field(val, "preset")?;
    Ok(CronAddInput {
        name,
        schedule,
        prompt,
        allow,
        preset,
    })
}

async fn cron_toggle(
    state: Arc<WebState>,
    body: Bytes,
    enabled: bool,
) -> (StatusCode, Json<Value>) {
    let event = if enabled {
        "cron_enable"
    } else {
        "cron_disable"
    };
    let Some(reg) = &state.cron_registry else {
        return no_registry();
    };
    let needle = match needle_from_body(&body) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match reg.set_enabled(&needle, enabled).await {
        Ok(job) => {
            record_admin(
                &state,
                event,
                &job.name,
                "allow",
                if enabled {
                    "job enabled"
                } else {
                    "job disabled"
                },
            );
            (StatusCode::OK, Json(json!({ "job": job })))
        }
        Err(e) => {
            let err = (
                cron_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            );
            audit_rejected(&state, event, &needle, &err);
            err
        }
    }
}

/// `POST /api/cron/enable {"id"|"name"}` — 404 when no job
/// matches the needle.
pub async fn cron_enable(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    cron_toggle(state, body, true).await
}

/// `POST /api/cron/disable {"id"|"name"}` — 404 when no job
/// matches the needle.
pub async fn cron_disable(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    cron_toggle(state, body, false).await
}

/// `POST /api/cron/remove {"id"|"name"}` — 404 when no job
/// matches the needle.
pub async fn cron_remove(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.cron_registry else {
        return no_registry();
    };
    let needle = match needle_from_body(&body) {
        Ok(n) => n,
        Err(e) => return e,
    };
    match reg.remove_job(&needle).await {
        Ok(job) => {
            record_admin(&state, "cron_remove", &job.name, "allow", "job removed");
            (StatusCode::OK, Json(json!({ "job": job })))
        }
        Err(e) => {
            let err = (
                cron_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            );
            audit_rejected(&state, "cron_remove", &needle, &err);
            err
        }
    }
}
