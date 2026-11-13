// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP handlers for the web-session endpoints (v0.4 B1, Axum in B2).
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
//! return Axum `(StatusCode, Json<Value>)` pairs.

use crate::web::server::WebState;
use crate::web::sessions::{is_valid_web_session_id, WebSessionRegistry};
use axum::{
    extract::State,
    http::{StatusCode, Uri},
    Json,
};
use serde_json::json;
use std::sync::Arc;

/// Axum handler for `GET /api/sessions`.
pub async fn list(State(state): State<Arc<WebState>>) -> (StatusCode, Json<serde_json::Value>) {
    handle_list_sessions(&state.registry)
}

/// Axum handler for `POST /api/sessions`.
pub async fn create(State(state): State<Arc<WebState>>) -> (StatusCode, Json<serde_json::Value>) {
    handle_create_session(&state.registry)
}

/// Axum handler for `GET /api/sessions/:id/transcript`. The id is
/// taken from the raw request path (not Axum's `Path` extractor,
/// which percent-decodes first) so the B1 validation stays
/// byte-identical.
pub async fn transcript(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<serde_json::Value>) {
    handle_transcript(&state.registry, uri.path())
}

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
pub fn handle_list_sessions(
    registry: &Arc<WebSessionRegistry>,
) -> (StatusCode, Json<serde_json::Value>) {
    let list = registry.list_sessions();
    (StatusCode::OK, Json(json!(list)))
}

/// `POST /api/sessions` → `201 {"id": "<new id>"}`.
pub fn handle_create_session(
    registry: &Arc<WebSessionRegistry>,
) -> (StatusCode, Json<serde_json::Value>) {
    match registry.create_session() {
        Ok(session) => (StatusCode::CREATED, Json(json!({ "id": session.id }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

/// `GET /api/sessions/<id>/transcript` → `200 {"id","messages":[...]}`.
///
/// The id is the path segment after `/api/sessions/` and before
/// `/transcript`. Invalid shape → `400`; no file on disk → `404`.
/// The caller passes the raw request path (Axum's `Path` extractor
/// would percent-decode it first; the raw form keeps the B1
/// validation byte-identical).
pub fn handle_transcript(
    registry: &Arc<WebSessionRegistry>,
    uri: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(id) = uri
        .strip_prefix("/api/sessions/")
        .and_then(|rest| rest.strip_suffix("/transcript"))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed transcript path" })),
        );
    };
    if !is_valid_web_session_id(id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid session id" })),
        );
    }
    match registry.transcript(id) {
        Ok(messages) => (
            StatusCode::OK,
            Json(json!({ "id": id, "messages": messages })),
        ),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        ),
    }
}
