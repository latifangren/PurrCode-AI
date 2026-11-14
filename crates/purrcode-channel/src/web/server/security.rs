// SPDX-License-Identifier: GPL-3.0-or-later

//! WebUI security middleware (v0.4 B8, D7/D9).
//!
//! One layer around the R3-verified token flow: the token check
//! itself is unchanged (exact `Bearer` or `?token=`, headers +
//! query only — see `auth::token_matches`; a token in the body
//! never authenticates), and this module adds what F25 was still
//! missing around it:
//!
//! * **Failed-attempt limiting:** auth failures share one global
//!   sliding window ([`AUTH_FAILURE_LIMIT`] per
//!   [`AUTH_FAILURE_WINDOW`]). The window is global, not per-IP,
//!   because per-IP collapses behind a same-host reverse proxy
//!   (D2: every proxied request arrives from 127.0.0.1). Only
//!   failed attempts are gated — a correct token always passes,
//!   so an attacker guessing cannot lock the owner out.
//! * **Prompt limiting:** `POST /api/prompt` is capped per peer IP
//!   ([`PROMPT_LIMIT`] per [`PROMPT_WINDOW`]). Behind a same-host
//!   proxy this is effectively global; the reverse-proxy doc says
//!   so. A limit hit answers 429 with `Retry-After`.
//! * **Audit:** every 401 is recorded as `web_auth` (detail
//!   "missing" vs "mismatch" — never the presented token value)
//!   and every 429 as `web_rate_limit` (detail names the bucket).
//!   The audit sink is the same one the admin endpoints use
//!   (`record_admin`); with no sink wired, limiting still works
//!   and nothing is recorded.

use crate::web::server::auth::token_matches;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use purrcode_api::tool::AuditSink;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Failed auth attempts allowed per [`AUTH_FAILURE_WINDOW`]
/// before further failures answer 429 instead of 401.
pub const AUTH_FAILURE_LIMIT: usize = 10;
/// Sliding window for [`AUTH_FAILURE_LIMIT`].
pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// `POST /api/prompt` calls allowed per peer IP per
/// [`PROMPT_WINDOW`].
pub const PROMPT_LIMIT: usize = 30;
/// Sliding window for [`PROMPT_LIMIT`].
pub const PROMPT_WINDOW: Duration = Duration::from_secs(60);
/// `Retry-After` value (seconds) on every 429 from this layer.
pub const RETRY_AFTER_SECS: u64 = 60;

/// Shared security state for the middleware: the configured
/// token (None = localhost default, no auth), the audit sink,
/// and the two sliding windows. All interior mutation is behind
/// one std mutex per window; the middleware holds no lock across
/// an await.
pub struct SecurityState {
    token: Option<String>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    auth_failures: Mutex<VecDeque<Instant>>,
    prompt_calls: Mutex<HashMap<String, VecDeque<Instant>>>,
    auth_limit: usize,
    auth_window: Duration,
    prompt_limit: usize,
    prompt_window: Duration,
}

impl SecurityState {
    pub fn new(token: Option<String>, audit_sink: Option<Arc<dyn AuditSink>>) -> Self {
        Self::with_limits(
            token,
            audit_sink,
            AUTH_FAILURE_LIMIT,
            AUTH_FAILURE_WINDOW,
            PROMPT_LIMIT,
            PROMPT_WINDOW,
        )
    }

    pub fn with_limits(
        token: Option<String>,
        audit_sink: Option<Arc<dyn AuditSink>>,
        auth_limit: usize,
        auth_window: Duration,
        prompt_limit: usize,
        prompt_window: Duration,
    ) -> Self {
        Self {
            token,
            audit_sink,
            auth_failures: Mutex::new(VecDeque::new()),
            prompt_calls: Mutex::new(HashMap::new()),
            auth_limit,
            auth_window,
            prompt_limit,
            prompt_window,
        }
    }

    fn record(&self, event: &str, name: &str, decision: &str, detail: &str) {
        if let Some(sink) = &self.audit_sink {
            sink.record_event("web", "web", event, Some(name), decision, detail);
        }
    }

    /// Outcome of the auth check for one request. `Allowed` means
    /// the request may proceed (no token configured, or the token
    /// matched). `Unauthorized` records one failure; once the
    /// window holds more than `auth_limit` failures, further
    /// failures are `RateLimited` instead of 401.
    pub fn check_auth(&self, headers: &HeaderMap, query: Option<&str>, peer: &str) -> AuthOutcome {
        let Some(required) = &self.token else {
            return AuthOutcome::Allowed;
        };
        if token_matches(headers, query, required) {
            return AuthOutcome::Allowed;
        }
        let present = auth_present(headers, query);
        let detail = if present {
            "token mismatch"
        } else {
            "missing token"
        };
        self.record("web_auth", peer, "deny", detail);
        let mut failures = self.auth_failures.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        while failures
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.auth_window)
        {
            failures.pop_front();
        }
        failures.push_back(now);
        if failures.len() > self.auth_limit {
            self.record(
                "web_rate_limit",
                "auth",
                "deny",
                &format!("peer={peer} failures={}", failures.len()),
            );
            AuthOutcome::RateLimited
        } else {
            AuthOutcome::Unauthorized
        }
    }

    /// Prompt-bucket check for one `POST /api/prompt` from `peer`.
    /// `false` means the call is over the limit (and already
    /// audited as `web_rate_limit`); `true` means it may proceed
    /// and the call is counted.
    pub fn check_prompt(&self, peer: &str) -> bool {
        let mut buckets = self.prompt_calls.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let bucket = buckets.entry(peer.to_string()).or_default();
        while bucket
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.prompt_window)
        {
            bucket.pop_front();
        }
        if bucket.len() >= self.prompt_limit {
            self.record(
                "web_rate_limit",
                "prompt",
                "deny",
                &format!("peer={peer} calls={}", bucket.len()),
            );
            return false;
        }
        bucket.push_back(now);
        true
    }
}

pub enum AuthOutcome {
    Allowed,
    Unauthorized,
    RateLimited,
}

/// Whether the request carried any auth material at all (an
/// `Authorization` header or a `token` query parameter). Used
/// only to label the audit detail ("missing" vs "mismatch") —
/// the presented value is never logged.
fn auth_present(headers: &HeaderMap, query: Option<&str>) -> bool {
    if headers.contains_key(header::AUTHORIZATION) {
        return true;
    }
    if let Some(query) = query {
        for param in query.split('&') {
            if let Some((k, _)) = param.split_once('=') {
                if k == "token" {
                    return true;
                }
            }
        }
    }
    false
}

fn peer_ip(request: &Request) -> String {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn rate_limited_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (header::CONTENT_TYPE, "text/plain"),
            (header::RETRY_AFTER, RETRY_AFTER_SECS.to_string().as_str()),
        ],
        "Too many requests\n",
    )
        .into_response()
}

/// Axum middleware (v0.4 B8): the R3 token check plus the D9
/// limits. Runs outermost (added last), so a rejected request
/// never reaches a handler or a body read. With no token
/// configured the auth half is a pass-through; the prompt half
/// still limits.
pub async fn require_token_and_limit(
    State(security): State<Arc<SecurityState>>,
    request: Request,
    next: Next,
) -> Response {
    let peer = peer_ip(&request);
    let query = request.uri().query().map(str::to_string);
    match security.check_auth(request.headers(), query.as_deref(), &peer) {
        AuthOutcome::Allowed => {}
        AuthOutcome::Unauthorized => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::CONTENT_TYPE, "text/plain")],
                "Unauthorized\n",
            )
                .into_response();
        }
        AuthOutcome::RateLimited => return rate_limited_response(),
    }
    if request.method() == Method::POST
        && request.uri().path() == "/api/prompt"
        && !security.check_prompt(&peer)
    {
        return rate_limited_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn state_with_token() -> SecurityState {
        SecurityState::with_limits(
            Some("SECRET".to_string()),
            None,
            3,
            Duration::from_secs(60),
            2,
            Duration::from_secs(60),
        )
    }

    #[test]
    fn test_auth_allows_without_token_and_with_correct_token() {
        let open = SecurityState::new(None, None);
        assert!(matches!(
            open.check_auth(&headers(&[]), None, "127.0.0.1"),
            AuthOutcome::Allowed
        ));
        let locked = state_with_token();
        assert!(matches!(
            locked.check_auth(
                &headers(&[("authorization", "Bearer SECRET")]),
                None,
                "127.0.0.1"
            ),
            AuthOutcome::Allowed
        ));
        assert!(matches!(
            locked.check_auth(&headers(&[]), Some("token=SECRET"), "127.0.0.1"),
            AuthOutcome::Allowed
        ));
    }

    #[test]
    fn test_auth_failures_rate_limit_after_limit() {
        // Limit 3: the first three failures are 401, the fourth
        // (and later) are 429 — and a correct token still passes
        // during the lockout.
        let state = state_with_token();
        for _ in 0..3 {
            assert!(matches!(
                state.check_auth(
                    &headers(&[("authorization", "Bearer WRONG")]),
                    None,
                    "127.0.0.1"
                ),
                AuthOutcome::Unauthorized
            ));
        }
        assert!(matches!(
            state.check_auth(
                &headers(&[("authorization", "Bearer WRONG")]),
                None,
                "127.0.0.1"
            ),
            AuthOutcome::RateLimited
        ));
        assert!(matches!(
            state.check_auth(
                &headers(&[("authorization", "Bearer SECRET")]),
                None,
                "127.0.0.1"
            ),
            AuthOutcome::Allowed
        ));
    }

    #[test]
    fn test_prompt_bucket_is_per_peer_and_caps() {
        // Limit 2 per peer: two calls pass, the third is denied;
        // another peer still has its own budget.
        let state = state_with_token();
        assert!(state.check_prompt("10.0.0.1"));
        assert!(state.check_prompt("10.0.0.1"));
        assert!(!state.check_prompt("10.0.0.1"));
        assert!(state.check_prompt("10.0.0.2"));
    }

    #[test]
    fn test_auth_present_labels_missing_vs_mismatch() {
        assert!(!auth_present(&headers(&[]), None));
        assert!(!auth_present(&headers(&[]), Some("other=1")));
        assert!(auth_present(
            &headers(&[("authorization", "Bearer WRONG")]),
            None
        ));
        assert!(auth_present(&headers(&[]), Some("token=WRONG")));
    }
}
