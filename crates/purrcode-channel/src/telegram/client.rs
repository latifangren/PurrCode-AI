// SPDX-License-Identifier: GPL-3.0-or-later

//! Hand-rolled Telegram Bot API client (v0.2, ADR-013).
//!
//! Plain reqwest POSTs, no Telegram SDK. The token lives in the request
//! URL path (`/bot<TOKEN>/...`) by Bot API design — it must NEVER reach a
//! log line: reqwest errors are stripped of their URL (`without_url()`)
//! and the client redacts the token in `Debug`.
//!
//! Rate limits: a 429 returns [`TelegramError::RateLimited`] with the
//! `retry_after` the server asked for (default 1s). The SEND path
//! (`send_message`, `edit_message_text`) waits and retries (max 2,
//! wait capped at 60s); the poll path (`get_updates`) returns the error
//! to the loop, which waits before polling again.

use crate::telegram::types::{
    BotApiResponse, BotInfo, InlineKeyboardMarkup, Message, Update, User,
};
use purrcode_api::error::{PurrError, Result as PurrResult};
use std::fmt;
use std::time::Duration;

/// Honest default User-Agent (same literal as the provider crate's
/// `DEFAULT_USER_AGENT` in `purrcode-provider`; kept in sync by hand —
/// the channel crate must not depend on the provider crate).
pub const TELEGRAM_USER_AGENT: &str =
    "PurrCode-AI/0.1 (purrcode-ai; https://github.com/latifangren/PurrCode-AI)";

/// Longest a single retry-after wait may be (ZeroClaw's
/// `TELEGRAM_MAX_INLINE_RETRY_AFTER`).
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// How many times a send retries after a 429.
const SEND_RETRIES: u32 = 2;

/// Errors from the Bot API client. Never carries the token.
#[derive(Debug)]
pub enum TelegramError {
    /// HTTP 401 — the bot token is wrong. The daemon must stop, not retry.
    Unauthorized,
    /// HTTP 429 — wait `retry_after` before the next attempt.
    RateLimited { retry_after: Duration },
    /// Any other `ok: false` answer (or non-200 status).
    Api { code: i32, description: String },
    /// Transport failure (DNS, connect, TLS, timeout). The message never
    /// contains the request URL (the token lives in it).
    Transport(String),
    /// The server answered something that is not a Bot API envelope.
    InvalidResponse(String),
}

impl fmt::Display for TelegramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelegramError::Unauthorized => {
                write!(f, "Telegram rejected the bot token (401)")
            }
            TelegramError::RateLimited { retry_after } => write!(
                f,
                "Telegram rate limit (429); retry after {}s",
                retry_after.as_secs()
            ),
            TelegramError::Api { code, description } => {
                write!(f, "Telegram API error {code}: {description}")
            }
            TelegramError::Transport(msg) => write!(f, "Telegram request failed: {msg}"),
            TelegramError::InvalidResponse(msg) => {
                write!(f, "Telegram answered an invalid response: {msg}")
            }
        }
    }
}

impl std::error::Error for TelegramError {}

impl From<TelegramError> for PurrError {
    fn from(err: TelegramError) -> Self {
        PurrError::Provider {
            provider: "telegram".to_string(),
            message: err.to_string(),
        }
    }
}

/// Thin Bot API client. Created once per daemon run.
pub struct TelegramApiClient {
    http: reqwest::Client,
    api_base: String,
    token: String,
}

impl fmt::Debug for TelegramApiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramApiClient")
            .field("api_base", &self.api_base)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl TelegramApiClient {
    /// Build a client for `api_base` (default `https://api.telegram.org`).
    /// Rejects a non-URL base and an empty token — fail fast at startup,
    /// not on the first poll.
    pub fn new(api_base: &str, token: &str) -> PurrResult<Self> {
        let base = api_base.trim_end_matches('/').to_string();
        let url = reqwest::Url::parse(&base).map_err(|e| PurrError::Provider {
            provider: "telegram".to_string(),
            message: format!("Invalid Telegram api_base '{base}': {e}"),
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(PurrError::Provider {
                provider: "telegram".to_string(),
                message: format!("Invalid Telegram api_base '{base}': only http(s) is allowed"),
            });
        }
        if token.trim().is_empty() {
            return Err(PurrError::Provider {
                provider: "telegram".to_string(),
                message: "Telegram bot token is empty. Set TELEGRAM_BOT_TOKEN in ~/.purrcode/credentials, then run `purrcode-ai gateway`.".to_string(),
            });
        }
        let http = reqwest::Client::builder()
            .user_agent(TELEGRAM_USER_AGENT)
            .build()
            .map_err(|e| PurrError::Provider {
                provider: "telegram".to_string(),
                message: format!("Failed to build Telegram HTTP client: {e}"),
            })?;
        Ok(Self {
            http,
            api_base: base,
            token: token.to_string(),
        })
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.token, method)
    }

    /// One POST to `{api_base}/bot<token>/{method}` with a JSON body.
    /// `timeout` bounds the whole request (long-poll needs a generous one).
    async fn post<T>(
        &self,
        method: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, TelegramError>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = self.method_url(method);
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| TelegramError::Transport(e.without_url().to_string()))?;
        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| TelegramError::Transport(e.without_url().to_string()))?;
        let envelope: BotApiResponse<T> = serde_json::from_str(&raw).map_err(|_| {
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return TelegramError::Unauthorized;
            }
            TelegramError::InvalidResponse(format!(
                "HTTP {} from method '{method}' is not a Bot API envelope",
                status.as_u16()
            ))
        })?;
        if !envelope.ok {
            let code = envelope.error_code.unwrap_or(status.as_u16() as i32);
            if code == 401 {
                return Err(TelegramError::Unauthorized);
            }
            if code == 429 {
                let secs = envelope
                    .parameters
                    .as_ref()
                    .and_then(|p| p.retry_after)
                    .unwrap_or(1);
                return Err(TelegramError::RateLimited {
                    retry_after: Duration::from_secs(secs),
                });
            }
            return Err(TelegramError::Api {
                code,
                description: envelope
                    .description
                    .unwrap_or_else(|| "unknown error".to_string()),
            });
        }
        envelope.result.ok_or_else(|| {
            TelegramError::InvalidResponse(format!(
                "method '{method}' answered ok:true without a result"
            ))
        })
    }

    /// `getUpdates`: long-poll. `offset` is the next update id to return
    /// (omit on the first probe). `timeout` is the server-side long-poll
    /// wait in seconds; the HTTP timeout is `timeout + 15s`.
    pub async fn get_updates(
        &self,
        offset: Option<i64>,
        limit: u32,
        timeout: u32,
    ) -> Result<Vec<Update>, TelegramError> {
        let mut body = serde_json::json!({
            "limit": limit,
            "timeout": timeout,
            "allowed_updates": ["message", "callback_query"],
        });
        if let Some(off) = offset {
            body["offset"] = serde_json::json!(off);
        }
        self.post(
            "getUpdates",
            body,
            Duration::from_secs(u64::from(timeout) + 15),
        )
        .await
    }

    /// `sendMessage`. Retries a 429 up to [`SEND_RETRIES`] times, waiting
    /// the server's `retry_after` (capped at [`MAX_RETRY_AFTER`]).
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<InlineKeyboardMarkup>,
    ) -> Result<Message, TelegramError> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(markup) = reply_markup {
            body["reply_markup"] = serde_json::to_value(markup).map_err(|e| {
                TelegramError::InvalidResponse(format!("failed to encode keyboard: {e}"))
            })?;
        }
        self.send_with_retry("sendMessage", body).await
    }

    /// `editMessageText`. Same 429 retry as send. A "message is not
    /// modified" answer is NOT an error (the draft converged).
    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        reply_markup: Option<InlineKeyboardMarkup>,
    ) -> Result<(), TelegramError> {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
        });
        if let Some(markup) = reply_markup {
            body["reply_markup"] = serde_json::to_value(markup).map_err(|e| {
                TelegramError::InvalidResponse(format!("failed to encode keyboard: {e}"))
            })?;
        }
        match self
            .send_with_retry::<serde_json::Value>("editMessageText", body)
            .await
        {
            Ok(_) => Ok(()),
            Err(TelegramError::Api { description, .. })
                if description.contains("message is not modified") =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// `answerCallbackQuery` (closes the loading spinner on the button).
    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<(), TelegramError> {
        let mut body = serde_json::json!({ "callback_query_id": callback_query_id });
        if let Some(t) = text {
            body["text"] = serde_json::json!(t);
        }
        let _: serde_json::Value = self
            .post("answerCallbackQuery", body, Duration::from_secs(30))
            .await?;
        Ok(())
    }

    /// `sendChatAction` (`typing`, ...).
    pub async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TelegramError> {
        let body = serde_json::json!({ "chat_id": chat_id, "action": action });
        let _: serde_json::Value = self
            .post("sendChatAction", body, Duration::from_secs(30))
            .await?;
        Ok(())
    }

    /// `getMe` — the bot's own id + username (needed for group mention
    /// gating).
    pub async fn get_me(&self) -> Result<BotInfo, TelegramError> {
        self.post("getMe", serde_json::json!({}), Duration::from_secs(30))
            .await
    }

    /// POST with 429 retry (used by the send path only). The poll path
    /// (`get_updates`) surfaces `RateLimited` to its loop instead.
    async fn send_with_retry<T>(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<T, TelegramError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut attempt = 0;
        loop {
            match self
                .post::<T>(method, body.clone(), Duration::from_secs(30))
                .await
            {
                Ok(out) => return Ok(out),
                Err(TelegramError::RateLimited { retry_after }) if attempt < SEND_RETRIES => {
                    attempt += 1;
                    let wait = retry_after.min(MAX_RETRY_AFTER);
                    tracing::warn!(
                        "Telegram 429 on {method}; waiting {}s before retry {attempt}/{SEND_RETRIES}",
                        wait.as_secs()
                    );
                    tokio::time::sleep(wait).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// The `from` of an incoming message, for allowlist logging. Only the
/// numeric id is ever recorded — never the message text.
pub fn describe_sender(user: Option<&User>) -> String {
    match user {
        Some(u) => format!("user {}", u.id),
        None => "unknown sender".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    /// Minimal canned HTTP server: answers each request from a scripted
    /// queue of (status, body) pairs, and records request bodies.
    struct MockBotApi {
        port: u16,
        requests: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl MockBotApi {
        async fn start(script: Vec<(u16, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let script = Arc::new(Mutex::new(script.into_iter().collect::<VecDeque<_>>()));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let requests_clone = requests.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let script = script.clone();
                    let requests = requests_clone.clone();
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        let mut tmp = [0u8; 4096];
                        // Read until the end of headers, then the body.
                        loop {
                            match socket.read(&mut tmp).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    buf.extend_from_slice(&tmp[..n]);
                                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                                Err(_) => return,
                            }
                        }
                        let text = String::from_utf8_lossy(&buf).to_string();
                        let (head, _) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                        let mut lines = head.lines();
                        let request_line = lines.next().unwrap_or("").to_string();
                        let mut content_length = 0usize;
                        for line in lines {
                            if let Some(v) = line.strip_prefix("Content-Length:") {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                        let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(text.len());
                        let mut body = text[body_start..].to_string();
                        while body.len() < content_length {
                            match socket.read(&mut tmp).await {
                                Ok(0) => break,
                                Ok(n) => body.push_str(&String::from_utf8_lossy(&tmp[..n])),
                                Err(_) => break,
                            }
                        }
                        requests.lock().await.push((request_line, body));
                        let (status, resp_body) = script
                            .lock()
                            .await
                            .pop_front()
                            .unwrap_or((200, r#"{"ok":true,"result":true}"#.to_string()));
                        let response = format!(
                            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            });
            Self { port, requests }
        }

        fn base(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    use std::collections::VecDeque;

    fn client_for(mock: &MockBotApi) -> TelegramApiClient {
        TelegramApiClient::new(&mock.base(), "TEST-TOKEN-123").unwrap()
    }

    #[tokio::test]
    async fn test_get_updates_sends_offset_limit_timeout_and_parses() {
        let mock = MockBotApi::start(vec![(
            200,
            r#"{"ok":true,"result":[{"update_id":7,"message":{"message_id":2,"chat":{"id":100,"type":"private"},"text":"hello"}}]}"#.to_string(),
        )])
        .await;
        let client = client_for(&mock);
        let updates = client.get_updates(Some(6), 100, 30).await.unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].update_id, 7);
        let reqs = mock.requests.lock().await;
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].0.contains("/botTEST-TOKEN-123/getUpdates"));
        let body: serde_json::Value = serde_json::from_str(&reqs[0].1).unwrap();
        assert_eq!(body["offset"], 6);
        assert_eq!(body["limit"], 100);
        assert_eq!(body["timeout"], 30);
        assert_eq!(
            body["allowed_updates"],
            serde_json::json!(["message", "callback_query"])
        );
    }

    #[tokio::test]
    async fn test_unauthorized_401_is_terminal() {
        let mock = MockBotApi::start(vec![(
            401,
            r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#.to_string(),
        )])
        .await;
        let client = client_for(&mock);
        let err = client.get_me().await.unwrap_err();
        assert!(matches!(err, TelegramError::Unauthorized));
    }

    #[tokio::test]
    async fn test_rate_limited_429_carries_retry_after() {
        let mock = MockBotApi::start(vec![(
            429,
            r#"{"ok":false,"error_code":429,"description":"Too Many Requests","parameters":{"retry_after":2}}"#.to_string(),
        )])
        .await;
        let client = client_for(&mock);
        let err = client.get_me().await.unwrap_err();
        match err {
            TelegramError::RateLimited { retry_after } => {
                assert_eq!(retry_after, Duration::from_secs(2));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_message_retries_once_after_429() {
        let mock = MockBotApi::start(vec![
            (
                429,
                r#"{"ok":false,"error_code":429,"description":"Too Many Requests","parameters":{"retry_after":0}}"#.to_string(),
            ),
            (
                200,
                r#"{"ok":true,"result":{"message_id":9,"chat":{"id":100,"type":"private"},"text":"hi"}}"#.to_string(),
            ),
        ])
        .await;
        let client = client_for(&mock);
        let sent = client.send_message(100, "hi", None).await.unwrap();
        assert_eq!(sent.message_id, 9);
        assert_eq!(mock.requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn test_edit_message_not_modified_is_ok() {
        let mock = MockBotApi::start(vec![(
            400,
            r#"{"ok":false,"error_code":400,"description":"Bad Request: message is not modified"}"#
                .to_string(),
        )])
        .await;
        let client = client_for(&mock);
        client
            .edit_message_text(100, 9, "same", None)
            .await
            .unwrap();
    }

    #[test]
    fn test_new_rejects_empty_token_and_bad_base() {
        assert!(TelegramApiClient::new("https://api.telegram.org", "").is_err());
        assert!(TelegramApiClient::new("not a url", "x").is_err());
        assert!(TelegramApiClient::new("ftp://example.com", "x").is_err());
        assert!(TelegramApiClient::new("https://api.telegram.org/", "x").is_ok());
    }

    #[test]
    fn test_debug_redacts_token() {
        let client = TelegramApiClient::new("https://api.telegram.org", "SECRET-TOKEN").unwrap();
        let debug = format!("{client:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("SECRET-TOKEN"));
    }

    #[tokio::test]
    async fn test_transport_error_never_contains_token() {
        // Point at a closed port: connect fails; the error must not carry
        // the URL (which contains the token).
        let client = TelegramApiClient::new("http://127.0.0.1:1", "SECRET-TOKEN").unwrap();
        let err = client.get_me().await.unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("SECRET-TOKEN"), "token leaked: {msg}");
        assert!(matches!(err, TelegramError::Transport(_)));
    }
}
