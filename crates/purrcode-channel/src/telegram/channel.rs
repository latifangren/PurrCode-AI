// SPDX-License-Identifier: GPL-3.0-or-later

//! Telegram channel transport (v0.2, ADR-013).
//!
//! [`TelegramChannel`] is the daemon-level transport: it long-polls
//! `getUpdates`, enforces the allowlist and the group mention rule, routes
//! commands, resolves callback taps into pending approvals/asks, and hands
//! plain prompts to the gateway as [`InboundMessage`]s. The gateway runs
//! the agent turn (per chat, sequentially) and renders through
//! [`TelegramRenderer`].
//!
//! [`TelegramChatChannel`] is the per-chat view that implements the
//! shared [`Channel`] trait (F16): it knows its destination chat, so
//! `send_event` can render an [`AgentEvent`] into it, and
//! `request_approval` delegates to the same pending-approval machinery
//! the poll loop resolves.

use crate::telegram::approval::{
    AlwaysSet, ApprovalDecision, Pending, TelegramApprovalRequester, TelegramAskRequester,
};
use crate::telegram::chunk::split_message;
use crate::telegram::client::{TelegramApiClient, TelegramError};
use crate::telegram::render::{render_event, TelegramRenderer};
use crate::telegram::sessions::ChatSessionIndex;
use crate::telegram::types::{BotInfo, CallbackQuery, Message, Update};
use purrcode_api::channel::{Channel, ChannelCapabilities};
use purrcode_api::error::Result as PurrResult;
use purrcode_api::event::AgentEvent;
use purrcode_api::tool::AuditSink;
use purrcode_config::TelegramConfig;
use purrcode_core::ApprovalRequester;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Mutex};

/// `getUpdates` page size (ZeroClaw's `TELEGRAM_POLL_LIMIT`).
const POLL_LIMIT: u32 = 100;
/// I/O error backoff between poll attempts.
const POLL_ERROR_BACKOFF: Duration = Duration::from_secs(5);
/// After this long without a successful poll, the loop logs a warning
/// (ZeroClaw's `POLL_HEALTH_STALE_AFTER`).
const POLL_HEALTH_STALE_AFTER: Duration = Duration::from_secs(90);
/// Cap on a single 429 wait in the poll loop.
const MAX_POLL_RETRY_AFTER: Duration = Duration::from_secs(60);

/// A prompt accepted from Telegram, ready for an agent turn.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub chat_id: i64,
    pub session_id: String,
    pub text: String,
}

/// The daemon-level Telegram transport.
pub struct TelegramChannel {
    api: Arc<TelegramApiClient>,
    allow_from: HashSet<i64>,
    bot_username: Mutex<String>,
    bot_id: Mutex<i64>,
    pending_approvals: Arc<Pending<ApprovalDecision>>,
    pending_asks: Arc<Pending<Option<usize>>>,
    always: Arc<AlwaysSet>,
    index: Arc<Mutex<ChatSessionIndex>>,
    approval_timeout: Duration,
    poll_timeout: u32,
    warned_unauthorized: Mutex<HashSet<i64>>,
    last_poll_success: Mutex<Option<Instant>>,
    health_warned: Mutex<bool>,
}

impl TelegramChannel {
    pub fn new(
        api: Arc<TelegramApiClient>,
        config: &TelegramConfig,
        state_dir: &std::path::Path,
    ) -> Self {
        Self {
            api,
            allow_from: config.allow_from.iter().copied().collect(),
            bot_username: Mutex::new(String::new()),
            bot_id: Mutex::new(0),
            pending_approvals: Pending::new(),
            pending_asks: Pending::new(),
            always: AlwaysSet::new(),
            index: Arc::new(Mutex::new(ChatSessionIndex::load(state_dir))),
            approval_timeout: Duration::from_secs(config.approval_timeout_secs()),
            poll_timeout: config.poll_timeout_secs(),
            warned_unauthorized: Mutex::new(HashSet::new()),
            last_poll_success: Mutex::new(None),
            health_warned: Mutex::new(false),
        }
    }

    pub fn api(&self) -> &Arc<TelegramApiClient> {
        &self.api
    }

    pub fn approval_timeout(&self) -> Duration {
        self.approval_timeout
    }

    /// Record the bot's own identity (from `getMe` at startup). Needed
    /// for group mention gating.
    pub async fn set_bot_info(&self, info: &BotInfo) {
        *self.bot_username.lock().await = info.username.clone();
        *self.bot_id.lock().await = info.id;
    }

    /// A renderer bound to one chat (one per turn).
    pub fn renderer(&self, chat_id: i64) -> Arc<TelegramRenderer> {
        TelegramRenderer::new(self.api.clone(), chat_id)
    }

    /// The most recently active private chat (an accepted message or
    /// command), if any. Heartbeat delivery routes here first, falling
    /// back to the first `allow_from` entry when no private chat has
    /// been active yet (v0.3 F22). Groups are never returned: a
    /// heartbeat summary may quote the user's private checks and must
    /// not land in a group (F22 fix, re-checker F1).
    pub async fn last_active_private_chat(&self) -> Option<i64> {
        self.index.lock().await.last_active_private_chat()
    }

    /// An approval requester bound to one chat + session.
    pub fn approval_requester(
        &self,
        chat_id: i64,
        session_id: impl Into<String>,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> Arc<TelegramApprovalRequester> {
        Arc::new(TelegramApprovalRequester::new(
            self.api.clone(),
            self.pending_approvals.clone(),
            self.always.clone(),
            chat_id,
            session_id,
            self.approval_timeout,
            audit_sink,
        ))
    }

    /// An ask requester bound to one chat.
    pub fn ask_requester(&self, chat_id: i64) -> Arc<TelegramAskRequester> {
        Arc::new(TelegramAskRequester::new(
            self.api.clone(),
            self.pending_asks.clone(),
            chat_id,
            self.approval_timeout,
        ))
    }

    /// The per-chat [`Channel`] view (F16): same pending machinery and
    /// renderer, destination fixed to this chat.
    pub async fn chat_channel(
        &self,
        chat_id: i64,
        session_id: impl Into<String>,
    ) -> TelegramChatChannel {
        TelegramChatChannel {
            renderer: self.renderer(chat_id),
            approval: self.approval_requester(chat_id, session_id, None),
            chat_id,
        }
    }

    /// Long-poll loop. Drains the startup backlog (messages sent while
    /// the daemon was down are skipped, not executed — a coding agent
    /// must not run stale commands after a restart), then processes
    /// each batch in order. Returns on shutdown signal, on a 401
    /// (bad token — never retry forever), or when the gateway drops the
    /// receiver.
    pub async fn listen(
        &self,
        tx: mpsc::Sender<InboundMessage>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), TelegramError> {
        // Startup probe (timeout 0): validate the token and drain the
        // backlog in one call. Retried on transient failure — entering
        // the poll loop with offset None after a failed probe would
        // EXECUTE the stale backlog instead of draining it.
        loop {
            match self.api.get_updates(None, POLL_LIMIT, 0).await {
                Ok(updates) => {
                    if !updates.is_empty() {
                        tracing::info!(
                            "telegram: skipping {} update(s) sent while the daemon was down",
                            updates.len()
                        );
                    }
                    *self.last_poll_success.lock().await = Some(Instant::now());
                    let mut offset = updates.last().map(|u| u.update_id + 1);
                    return self.poll_loop(tx, &mut shutdown, &mut offset).await;
                }
                Err(TelegramError::Unauthorized) => return Err(TelegramError::Unauthorized),
                Err(e) => {
                    tracing::warn!("telegram: startup probe failed ({e}); retrying probe in 5s");
                    if *shutdown.borrow() || tx.is_closed() {
                        return Ok(());
                    }
                    if wait_or_shutdown(POLL_ERROR_BACKOFF, &mut shutdown).await {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn poll_loop(
        &self,
        tx: mpsc::Sender<InboundMessage>,
        shutdown: &mut watch::Receiver<bool>,
        offset: &mut Option<i64>,
    ) -> Result<(), TelegramError> {
        loop {
            if *shutdown.borrow() || tx.is_closed() {
                return Ok(());
            }
            self.maybe_warn_stale_poll().await;
            match self
                .api
                .get_updates(*offset, POLL_LIMIT, self.poll_timeout)
                .await
            {
                Ok(updates) => {
                    *self.last_poll_success.lock().await = Some(Instant::now());
                    *self.health_warned.lock().await = false;
                    // The offset advances only after the WHOLE batch was
                    // processed — a transient failure mid-batch holds the
                    // offset, so the batch is retried, not lost.
                    let mut processed = 0usize;
                    for update in &updates {
                        self.process_update(update, &tx).await;
                        processed += 1;
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                    if let Some(last) = updates.get(processed.saturating_sub(1)) {
                        // Only advance past updates actually processed.
                        *offset = Some(last.update_id + 1);
                    }
                    if processed < updates.len() {
                        return Ok(()); // shutdown landed mid-batch
                    }
                }
                Err(TelegramError::RateLimited { retry_after }) => {
                    let wait = retry_after.min(MAX_POLL_RETRY_AFTER);
                    tracing::warn!("telegram: poll rate-limited; waiting {}s", wait.as_secs());
                    if wait_or_shutdown(wait, shutdown).await {
                        return Ok(());
                    }
                }
                Err(TelegramError::Unauthorized) => {
                    return Err(TelegramError::Unauthorized);
                }
                Err(e) => {
                    tracing::warn!("telegram: poll error ({e}); retrying in 5s");
                    if wait_or_shutdown(POLL_ERROR_BACKOFF, shutdown).await {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn maybe_warn_stale_poll(&self) {
        let last = *self.last_poll_success.lock().await;
        let stale = match last {
            Some(t) => t.elapsed() > POLL_HEALTH_STALE_AFTER,
            None => false,
        };
        if stale && !*self.health_warned.lock().await {
            tracing::warn!(
                "telegram: no successful poll for over {}s (listener may be stuck)",
                POLL_HEALTH_STALE_AFTER.as_secs()
            );
            *self.health_warned.lock().await = true;
        }
    }

    /// Route one update. Callback taps resolve pending requests;
    /// messages go through allowlist → group mention → command → turn.
    async fn process_update(&self, update: &Update, tx: &mpsc::Sender<InboundMessage>) {
        if let Some(cb) = &update.callback_query {
            self.process_callback(cb).await;
            return;
        }
        let Some(msg) = &update.message else {
            return;
        };
        let Some(text) = msg.text() else {
            // Non-text messages (photo, voice, ...) are not supported in
            // v0.2 — ignored silently, never crash the loop.
            tracing::debug!(
                "telegram: ignoring non-text message in chat {}",
                msg.chat.id
            );
            return;
        };
        let chat_id = msg.chat.id;

        // 1. Allowlist, before any other work (fail-closed).
        if !self.allow_from.contains(&chat_id) {
            let mut warned = self.warned_unauthorized.lock().await;
            if warned.insert(chat_id) {
                let _ = self
                    .api
                    .send_message(
                        chat_id,
                        "🔒 Bot ini privat — chat ini tidak diizinkan.",
                        None,
                    )
                    .await;
            }
            return;
        }

        // 2. Groups: only when the bot is @-mentioned.
        if msg.chat.kind == "group" || msg.chat.kind == "supergroup" {
            let username = self.bot_username.lock().await.clone();
            let bot_id = *self.bot_id.lock().await;
            if username.is_empty() || !msg.mentions_bot(&username, bot_id) {
                return; // ignored silently, no reply, no log of content
            }
        }

        // 3. Commands are handled in the router, not the agent.
        if msg.is_command() {
            self.process_command(msg, text).await;
            return;
        }

        // 4. Plain prompt: resolve the chat's session and hand the turn
        // to the gateway. The touch also records the chat kind: only
        // private chats become heartbeat delivery targets (a group
        // mention is recorded as a group — v0.3 F22 fix, re-checker F1).
        if let Err(e) = self
            .index
            .lock()
            .await
            .touch(chat_id, Some(msg.chat.kind.as_str()))
        {
            tracing::warn!("telegram: failed to record activity for chat {chat_id}: {e}");
        }
        let session_id = self.session_for(chat_id).await;
        let inbound = InboundMessage {
            chat_id,
            session_id,
            text: text.to_string(),
        };
        if tx.send(inbound).await.is_err() {
            tracing::warn!("telegram: gateway receiver gone; stopping poll");
        }
    }

    /// The chat's current session, creating one when the chat is new.
    async fn session_for(&self, chat_id: i64) -> String {
        let mut index = self.index.lock().await;
        if let Some(id) = index.current_session(chat_id) {
            return id.to_string();
        }
        let id = format!("purr_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        match index.attach(chat_id, &id) {
            Ok(()) => id,
            Err(e) => {
                tracing::warn!(
                    "telegram: failed to persist chat index ({e}); using ephemeral session"
                );
                id
            }
        }
    }

    /// Resolve a callback tap into a pending approval or ask. The
    /// callback query is answered exactly ONCE: resolved taps get a
    /// plain answer (spinner closes), stale taps get one carrying
    /// "Sudah kedaluwarsa." — the real API allows only one answer per
    /// query, so answering first and again on staleness would lose the
    /// toast.
    async fn process_callback(&self, cb: &CallbackQuery) {
        let Some(data) = cb.data.as_deref() else {
            let _ = self.api.answer_callback_query(&cb.id, None).await;
            return;
        };
        let resolved = if let Some(rest) = data.strip_prefix("ap:") {
            let Some((token, choice)) = rest.split_once(':') else {
                let _ = self.api.answer_callback_query(&cb.id, None).await;
                return;
            };
            let decision = match choice {
                "y" => ApprovalDecision::Approve,
                "n" => ApprovalDecision::Deny,
                "a" => ApprovalDecision::Always,
                _ => {
                    let _ = self.api.answer_callback_query(&cb.id, None).await;
                    return;
                }
            };
            self.pending_approvals.resolve(token, decision).await
        } else if let Some(rest) = data.strip_prefix("ak:") {
            let Some((token, idx)) = rest.split_once(':') else {
                let _ = self.api.answer_callback_query(&cb.id, None).await;
                return;
            };
            let chosen: Option<usize> = idx.parse().ok();
            self.pending_asks.resolve(token, chosen).await
        } else {
            // Unknown callback data: just close the spinner.
            let _ = self.api.answer_callback_query(&cb.id, None).await;
            return;
        };
        if resolved {
            let _ = self.api.answer_callback_query(&cb.id, None).await;
        } else {
            // Already claimed (e.g. timed out): the tap is too late.
            let _ = self
                .api
                .answer_callback_query(&cb.id, Some("Sudah kedaluwarsa."))
                .await;
        }
    }

    /// Bot commands (`/new`, `/resume`, `/stop`, `/help`). Replies are
    /// sent directly — commands never reach the agent loop.
    async fn process_command(&self, msg: &Message, _text: &str) {
        let chat_id = msg.chat.id;
        // A command is activity too: it refreshes the chat's timestamp
        // (and records its kind). A group command is recorded as a
        // group — it never becomes a heartbeat delivery target (v0.3
        // F22 fix, re-checker F1); only private chats do.
        if let Err(e) = self
            .index
            .lock()
            .await
            .touch(chat_id, Some(msg.chat.kind.as_str()))
        {
            tracing::warn!("telegram: failed to record activity for chat {chat_id}: {e}");
        }
        match msg.command().as_deref() {
            Some("/new") => {
                let id = format!("purr_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
                let mut index = self.index.lock().await;
                match index.new_session(chat_id, &id) {
                    Ok(_) => {
                        self.reply_text(
                            chat_id,
                            &format!(
                                "🆕 Sesi baru dimulai ({id}). Sesi sebelumnya diarsipkan — /resume untuk melihatnya."
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        self.reply_text(chat_id, &format!("Gagal memulai sesi baru: {e}"))
                            .await;
                    }
                }
            }
            Some("/resume") => {
                let parts: Vec<&str> = msg.text().unwrap_or("").split_whitespace().collect();
                if parts.len() >= 2 {
                    let wanted = parts[1];
                    let mut index = self.index.lock().await;
                    let known = index.sessions_for_chat(chat_id);
                    if known.iter().any(|s| s == wanted) {
                        match index.attach(chat_id, wanted) {
                            Ok(()) => {
                                self.reply_text(chat_id, &format!("↩️ Kembali ke sesi {wanted}."))
                                    .await;
                            }
                            Err(e) => {
                                self.reply_text(
                                    chat_id,
                                    &format!("Gagal kembali ke sesi {wanted}: {e}"),
                                )
                                .await;
                            }
                        }
                    } else {
                        self.reply_text(
                            chat_id,
                            &format!("Sesi {wanted} tidak ditemukan di chat ini."),
                        )
                        .await;
                    }
                } else {
                    let index = self.index.lock().await;
                    let sessions = index.sessions_for_chat(chat_id);
                    if sessions.is_empty() {
                        self.reply_text(
                            chat_id,
                            "Belum ada sesi di chat ini. Kirim pesan untuk memulai.",
                        )
                        .await;
                    } else {
                        let mut text = String::from("Sesi di chat ini:\n");
                        for (i, s) in sessions.iter().enumerate() {
                            let mark = if i == 0 { " ★ (saat ini)" } else { "" };
                            text.push_str(&format!("• {s}{mark}\n"));
                        }
                        text.push_str("\nBalas /resume <id> untuk kembali ke sesi itu.");
                        self.reply_text(chat_id, &text).await;
                    }
                }
            }
            Some("/stop") => {
                let mut index = self.index.lock().await;
                match index.end_session(chat_id) {
                    Ok(()) => {
                        self.reply_text(
                            chat_id,
                            "⏹ Sesi dihentikan dan diarsipkan. Kirim pesan untuk memulai sesi baru.",
                        )
                        .await;
                    }
                    Err(e) => {
                        self.reply_text(chat_id, &format!("Gagal menghentikan sesi: {e}"))
                            .await;
                    }
                }
            }
            Some("/help") => {
                self.reply_text(
                    chat_id,
                    "🐾 PurrCode AI via Telegram\n\nPerintah:\n/new — mulai sesi baru (sesi lama diarsipkan)\n/resume — daftar sesi chat ini\n/resume <id> — kembali ke sesi lama\n/stop — hentikan sesi saat ini\n/help — pesan ini\n\nKirim apa saja sebagai prompt. Aksi yang butuh persetujuan muncul sebagai kartu Ya / Tidak / Selalu.",
                )
                .await;
            }
            Some(other) => {
                self.reply_text(
                    chat_id,
                    &format!("Perintah {other} tidak dikenal. /help untuk daftar perintah."),
                )
                .await;
            }
            None => {}
        }
    }

    /// Send a plain-text reply in a chat (chunked at 4096 characters).
    async fn reply_text(&self, chat_id: i64, text: &str) {
        for chunk in split_message(text) {
            if self.api.send_message(chat_id, &chunk, None).await.is_err() {
                tracing::warn!("telegram: failed to send reply to chat {chat_id}");
                return;
            }
        }
    }
}

/// Sleep `dur`, or return early (true) when the shutdown watch fires.
async fn wait_or_shutdown(dur: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(dur) => false,
        _ = shutdown.changed() => true,
    }
}

/// Where the chat index lives: `~/.purrcode/telegram/`.
pub fn telegram_state_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    home.join(".purrcode").join("telegram")
}

/// Per-chat view implementing the shared [`Channel`] trait (F16).
pub struct TelegramChatChannel {
    renderer: Arc<TelegramRenderer>,
    approval: Arc<TelegramApprovalRequester>,
    chat_id: i64,
}

impl TelegramChatChannel {
    /// The chat id this channel is bound to.
    pub fn chat_id(&self) -> i64 {
        self.chat_id
    }

    /// The renderer behind this chat channel (for the turn's listener).
    pub fn renderer(&self) -> &Arc<TelegramRenderer> {
        &self.renderer
    }

    /// The approval requester behind this chat channel (for the turn's
    /// orchestrator).
    pub fn approval_requester(&self) -> &Arc<TelegramApprovalRequester> {
        &self.approval
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChatChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            streaming: true,
            edit_message: true,
            inline_buttons: true,
        }
    }

    async fn send_event(&self, event: &AgentEvent) -> PurrResult<()> {
        render_event(&self.renderer, event).await
    }

    async fn request_approval(
        &self,
        _call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> PurrResult<bool> {
        // The pending token (not the loop's call id) is the real
        // correlation id; the loop's ApprovalRequester shares this
        // machinery (ADR-013).
        Ok(self
            .approval
            .request_approval(tool, description, preview)
            .await)
    }
}
