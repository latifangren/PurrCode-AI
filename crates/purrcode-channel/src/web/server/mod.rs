// SPDX-License-Identifier: GPL-3.0-or-later

//! WebUI HTTP server (v0.4 B2: Axum routing over the tokio runtime
//! we already run).
//!
//! The hand-rolled `TcpListener` + manual request parser from R3
//! is gone: Axum owns routing, header parsing, and the 1 MiB
//! request-body limit (`tower-http`'s `RequestBodyLimitLayer` →
//! 413), and the handlers live in `routes.rs`, `sse.rs`, and
//! `sessions.rs` so this file stays under the ADR-011 limit.
//!
//! Observable behavior is unchanged from R3 + B1 (the same
//! tripwire tests guard it):
//!
//! * Token auth is a middleware: exact `Authorization: Bearer
//!   <token>` or `?token=<token>`, headers + query only (see
//!   `auth.rs`); a token in the body never authenticates.
//! * `POST` bodies are read as raw bytes and parsed as JSON by
//!   hand — no `Content-Type: application/json` requirement, and a
//!   malformed body is a 400.
//! * Unknown routes are 404; a known path with a wrong method is
//!   now 405 (Axum's default) instead of the old 404 — the only
//!   deliberate status change, and no client or test sends those
//!   methods.
//! * `/api/sessions/:id/transcript` matches one path segment; a
//!   multi-segment id falls through to 404 instead of the old 400
//!   (the id validation itself is byte-identical).

use crate::web::sessions::{WebSession, WebSessionRegistry, DEFAULT_WEB_SESSION_ID};
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use purrcode_api::cron::CronRegistry;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginRegistry;
use purrcode_api::skill::SkillRegistry;
use purrcode_api::tool::AuditSink;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::limit::RequestBodyLimitLayer;

mod auth;
mod routes;
mod routes_cron;
mod routes_memory;
mod routes_overview;
mod routes_skills;
mod sessions;
mod sse;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cron;
#[cfg(test)]
mod tests_overview;
#[cfg(test)]
mod tests_plugins;
#[cfg(test)]
mod tests_skills;

#[async_trait::async_trait]
pub trait PromptDispatcher: Send + Sync {
    /// Run one turn of `session` for `prompt`. The session carries
    /// its own bus, channel, and turn lock, so the dispatcher never
    /// needs a separate channel argument.
    async fn dispatch(&self, session: Arc<WebSession>, prompt: String) -> Result<()>;
}

/// Shared state for every handler: the session registry, the
/// directories the handlers read from, and the prompt dispatcher
/// (absent in tests that only exercise routing). `shutdown_rx`
/// carries the same cancel signal `run()` watches: the SSE handler
/// ends its stream on it (re-checker P1 — without it, graceful
/// shutdown waits on open browser tabs forever and Ctrl-C hangs).
/// `plugin_registry` (v0.4 B4) is the live plugin manager the
/// mount/unmount endpoints mutate; `audit_sink` records those
/// admin actions. Both are absent in tests that do not wire them
/// (GET /api/plugins then lists nothing; mount/unmount answer 503).
/// `skill_registry` (v0.4 B5) is the same shape for skills: the
/// list/detail/create/update/delete endpoints route through
/// `manage_skill` via this trait, and answer 503 without it. The
/// memory endpoints need no registry — they read/write
/// `purrcode_dir` directly.
pub struct WebState {
    pub registry: Arc<WebSessionRegistry>,
    pub purrcode_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub dispatcher: Option<Arc<dyn PromptDispatcher>>,
    pub plugin_registry: Option<Arc<dyn PluginRegistry>>,
    pub skill_registry: Option<Arc<dyn SkillRegistry>>,
    pub cron_registry: Option<Arc<dyn CronRegistry>>,
    pub audit_sink: Option<Arc<dyn AuditSink>>,
    pub shutdown_rx: watch::Receiver<bool>,
}

pub struct WebServer {
    host: String,
    port: u16,
    token: Option<String>,
    registry: Arc<WebSessionRegistry>,
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
    plugin_registry: Option<Arc<dyn PluginRegistry>>,
    skill_registry: Option<Arc<dyn SkillRegistry>>,
    cron_registry: Option<Arc<dyn CronRegistry>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
}

/// Maximum bytes for one HTTP request body. The old hand-rolled
/// server counted headers + body; the Axum limit layer counts the
/// body only, and is inclusive: a body of exactly 1 MiB is
/// accepted, where the old code rejected any body in
/// (1 MiB − headers, 1 MiB]. The boundary moved; no client or test
/// depends on it. Two more limit-layer deltas, both cosmetic: an
/// oversize body answers 413 with the plain-text body
/// `length limit exceeded` (the old 413 had an empty body), and
/// giant headers are rejected by hyper's default ~400 KB header
/// buffer with 431 (the old code counted headers toward the 1 MiB
/// and answered 413).
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// How long `run()` waits for graceful shutdown after cancel
/// before returning anyway. The old server returned as soon as the
/// accept loop broke, dropping in-flight connections; the Axum
/// server gives them this long to finish (the SSE streams end on
/// cancel, so in practice it returns in milliseconds). Without
/// this backstop, a single hung non-SSE connection would block
/// Ctrl-C forever.
const SHUTDOWN_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(5);

impl WebServer {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        token: Option<String>,
        registry: Arc<WebSessionRegistry>,
        purrcode_dir: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            token,
            registry,
            purrcode_dir: purrcode_dir.into(),
            workspace_root: workspace_root.into(),
            dispatcher: None,
            plugin_registry: None,
            skill_registry: None,
            cron_registry: None,
            audit_sink: None,
        }
    }

    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn PromptDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Wire the live plugin registry (v0.4 B4): the mount/unmount
    /// endpoints mutate this same instance the loops dispatch
    /// against, so a mount takes effect without restarting the
    /// server.
    pub fn with_plugin_registry(mut self, registry: Arc<dyn PluginRegistry>) -> Self {
        self.plugin_registry = Some(registry);
        self
    }

    /// Wire the skill registry (v0.4 B5): the skill endpoints
    /// route through `manage_skill` via this trait.
    pub fn with_skill_registry(mut self, registry: Arc<dyn SkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    /// Wire the cron registry (v0.4 B6): the scheduler endpoints
    /// route through the CLI's `cron add` path via this trait.
    pub fn with_cron_registry(mut self, registry: Arc<dyn CronRegistry>) -> Self {
        self.cron_registry = Some(registry);
        self
    }

    /// Wire the audit sink (v0.4 B4): mount/unmount actions are
    /// recorded as `plugin_mount` / `plugin_unmount` events.
    pub fn with_audit_sink(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = Some(sink);
        self
    }

    pub async fn run(&self, cancel_rx: watch::Receiver<bool>) -> Result<()> {
        let bind_addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| PurrError::Channel {
                channel: "web".to_string(),
                message: format!("Failed to bind {}: {}", bind_addr, e),
            })?;

        println!("🌐 WebUI server running at http://{}", bind_addr);
        if self.token.is_some() {
            println!("🔒 Token authentication enabled for remote access.");
        }

        let state = Arc::new(WebState {
            registry: self.registry.clone(),
            purrcode_dir: self.purrcode_dir.clone(),
            workspace_root: self.workspace_root.clone(),
            dispatcher: self.dispatcher.clone(),
            plugin_registry: self.plugin_registry.clone(),
            skill_registry: self.skill_registry.clone(),
            cron_registry: self.cron_registry.clone(),
            audit_sink: self.audit_sink.clone(),
            shutdown_rx: cancel_rx.clone(),
        });
        // Layers run outermost-first: the auth middleware (added
        // last) rejects unauthenticated requests before any body is
        // read; the body limit then answers 413 while a handler
        // reads an oversize body.
        let app = Router::new()
            .route("/", get(routes::index))
            .route("/index.html", get(routes::index))
            .route("/health", get(routes::status))
            .route("/api/status", get(routes::status))
            .route("/api/health", get(routes::status))
            .route("/api/events", get(sse::events))
            .route("/api/sessions", get(sessions::list).post(sessions::create))
            .route("/api/sessions/{id}/transcript", get(sessions::transcript))
            .route("/api/cron", get(routes::cron))
            .route("/api/overview", get(routes_overview::overview))
            .route("/api/cron/parse", post(routes_cron::cron_parse))
            .route("/api/cron/add", post(routes_cron::cron_add))
            .route("/api/cron/enable", post(routes_cron::cron_enable))
            .route("/api/cron/disable", post(routes_cron::cron_disable))
            .route("/api/cron/remove", post(routes_cron::cron_remove))
            .route("/api/skills", get(routes::skills))
            .route("/api/skills/candidates", get(routes::skills))
            .route("/api/skills/all", get(routes_skills::skills_all))
            .route("/api/skills/{name}", get(routes_skills::skill_detail))
            .route("/api/skills/approve", post(routes::skills_approve))
            .route("/api/skills/reject", post(routes::skills_reject))
            .route("/api/skills/create", post(routes_skills::skills_create))
            .route("/api/skills/update", post(routes_skills::skills_update))
            .route("/api/skills/delete", post(routes_skills::skills_delete))
            .route("/api/plugins", get(routes::plugins))
            .route("/api/plugins/mount", post(routes::plugins_mount))
            .route("/api/plugins/unmount", post(routes::plugins_unmount))
            .route("/api/prompt", post(routes::prompt))
            .route(
                "/api/memory",
                get(routes::memory).post(routes_memory::memory_save),
            )
            .route("/api/memory/info", get(routes_memory::memory_info))
            .route("/api/approval", post(routes::approval))
            .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
            .layer(middleware::from_fn_with_state(
                self.token.clone(),
                auth::require_token,
            ))
            .with_state(state);

        let mut shutdown_rx = cancel_rx.clone();
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
        // Backstop: if graceful shutdown has not finished
        // SHUTDOWN_GRACE_PERIOD after cancel (e.g. a hung non-SSE
        // connection), return anyway instead of hanging Ctrl-C
        // forever. This future never fires unless cancel was
        // signaled: a dropped sender (no cancel) parks it forever.
        let mut backstop_rx = cancel_rx.clone();
        let backstop = async move {
            loop {
                if *backstop_rx.borrow() {
                    break;
                }
                if backstop_rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
            tokio::time::sleep(SHUTDOWN_GRACE_PERIOD).await;
        };
        tokio::select! {
            res = axum::serve(listener, app).with_graceful_shutdown(shutdown) => {
                res.map_err(|e| PurrError::Channel {
                    channel: "web".to_string(),
                    message: format!("WebUI server error: {}", e),
                })?;
            }
            _ = backstop => {
                // Graceful shutdown exceeded the grace period;
                // return anyway, like the old server did.
            }
        }

        Ok(())
    }
}

/// The live session an approval body belongs to (v0.4 B1): the
/// body's optional `session_id`, defaulting to the legacy `purr_web`.
/// An explicit id is materialized on demand when the session exists
/// on disk (a resumed chat after restart — re-checker P2-1); a
/// present-but-non-string id resolves nothing (P3-3), and a session
/// with no live entry has nothing pending, so the caller answers
/// `resolved: false` instead of resolving against another session's
/// approvals.
fn approval_session(
    registry: &Arc<WebSessionRegistry>,
    val: &serde_json::Value,
) -> Option<Arc<WebSession>> {
    let Some(v) = val.get("session_id") else {
        return registry.get_session(DEFAULT_WEB_SESSION_ID);
    };
    let id = v.as_str()?;
    registry.ensure_session_if_exists(id).ok()
}
