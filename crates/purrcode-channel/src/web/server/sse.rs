// SPDX-License-Identifier: GPL-3.0-or-later

//! Server-Sent Events streaming for the WebUI (v0.4 B2, Axum).
//!
//! `GET /api/events` streams one session's [`AgentEvent`]s as
//! `data: {...}\n\n` frames, exactly as the hand-rolled server did
//! (R3): `?session=<id>` picks the chat, no parameter means the
//! legacy default `purr_web`, an explicit id is materialized on
//! demand when the session exists on disk (a resumed chat after
//! restart — B1 re-checker P2-1), and a valid id with no file is a
//! 404 instead of an empty stream. Lagged broadcast messages are
//! skipped, as before; a closed bus ends the stream.

use crate::web::server::sessions::query_param;
use crate::web::server::WebState;
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{sse::Event, IntoResponse, Sse},
    Json,
};
use serde_json::json;
use std::{convert::Infallible, sync::Arc};
use tokio::sync::broadcast::error::RecvError;

pub async fn events(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let session = match query_param(&uri.to_string(), "session") {
        Some(id) => match state.registry.ensure_session_if_exists(&id) {
            Ok(s) => s,
            Err(_) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "session not found" })),
                ));
            }
        },
        None => match state.registry.default_session() {
            Ok(s) => s,
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                ));
            }
        },
    };

    let rx = session.channel.subscribe();
    // The stream ends when cancel is signaled (re-checker P1):
    // without this, an open browser tab holds an SSE stream open
    // forever and graceful shutdown — hence Ctrl-C — never
    // completes. `take_until` also ends the stream at once when
    // cancel was already signaled before this stream started, and
    // when the sender drops.
    let mut shutdown_rx = state.shutdown_rx.clone();
    let shutdown = async move {
        loop {
            if *shutdown_rx.borrow() {
                break;
            }
            if shutdown_rx.changed().await.is_err() {
                break;
            }
        }
    };
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let Ok(data) = serde_json::to_string(&event) else {
                        continue;
                    };
                    break Some((Ok::<_, Infallible>(Event::default().data(data)), rx));
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break None,
            }
        }
    });
    let stream = futures_util::StreamExt::take_until(stream, shutdown);

    Ok((
        [
            (header::CACHE_CONTROL, "no-cache"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        Sse::new(stream),
    ))
}
