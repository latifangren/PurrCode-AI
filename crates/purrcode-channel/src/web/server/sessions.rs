// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP handlers for the web-session endpoints (v0.4 B1).
//!
//! The route table lives in `server/mod.rs`; this module owns the
//! session-specific work so `mod.rs` stays under the ADR-011 limit:
//!
//! * `GET /api/sessions` — list web sessions on disk, most recent first.
//! * `POST /api/sessions` — mint a fresh session (`201 {"id": ...}`).
//! * `GET /api/sessions/<id>/transcript` — replay one session's history.
//!
//! Session ids come from the client, so every handler validates the
//! id shape before it touches the registry or the store: an invalid
//! id is a `400`, a valid-but-unknown id is a `404`. The handlers
//! return `(status, body)` and the caller formats the HTTP response.

use crate::web::sessions::{is_valid_web_session_id, WebSessionRegistry};
use std::sync::Arc;

/// `?key=value` lookup on a raw request URI (no URL decoding: web
/// session ids are `[a-z0-9_]` only, so nothing in them needs it).
pub fn query_param(raw_uri: &str, key: &str) -> Option<String> {
    let query = raw_uri.split_once('?').map(|(_, q)| q)?;
    for param in query.split('&') {
        if let Some((k, v)) = param.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// `GET /api/sessions` → `200` + `[{"id","message_count","title"}]`.
pub fn handle_list_sessions(registry: &Arc<WebSessionRegistry>) -> (u16, String) {
    let list = registry.list_sessions();
    let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
    (200, body)
}

/// `POST /api/sessions` → `201 {"id": "<new id>"}`.
pub fn handle_create_session(registry: &Arc<WebSessionRegistry>) -> (u16, String) {
    match registry.create_session() {
        Ok(session) => (201, serde_json::json!({ "id": session.id }).to_string()),
        Err(e) => (
            500,
            serde_json::json!({ "error": e.to_string() }).to_string(),
        ),
    }
}

/// `GET /api/sessions/<id>/transcript` → `200 {"id","messages":[...]}`.
///
/// The id is the path segment after `/api/sessions/` and before
/// `/transcript`. Invalid shape → `400`; no file on disk → `404`.
pub fn handle_transcript(registry: &Arc<WebSessionRegistry>, uri: &str) -> (u16, String) {
    let Some(id) = uri
        .strip_prefix("/api/sessions/")
        .and_then(|rest| rest.strip_suffix("/transcript"))
    else {
        return (
            400,
            serde_json::json!({ "error": "malformed transcript path" }).to_string(),
        );
    };
    if !is_valid_web_session_id(id) {
        return (
            400,
            serde_json::json!({ "error": "invalid session id" }).to_string(),
        );
    }
    match registry.transcript(id) {
        Ok(messages) => (
            200,
            serde_json::json!({ "id": id, "messages": messages }).to_string(),
        ),
        Err(_) => (
            404,
            serde_json::json!({ "error": "session not found" }).to_string(),
        ),
    }
}
