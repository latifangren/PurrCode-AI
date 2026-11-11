// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering agent events into Telegram messages (v0.2, ADR-013).
//!
//! One [`TelegramRenderer`] per chat turn. Text chunks accumulate into a
//! draft that is sent as a message once it is long enough, then edited
//! in place as it grows (throttled to ~1 edit/second, only when the
//! content changed — Telegram rejects identical edits). Tool calls get
//! their own short lines (`🔧 read src/auth.rs — ok`) as separate
//! messages, so they never corrupt the answer draft. Long answers are
//! chunked at 4096 characters ([`super::chunk::split_message`]).
//!
//! The loop's [`AgentEventListener`] callbacks are synchronous, so the
//! renderer is an actor: callbacks push [`RenderOp`]s into an unbounded
//! channel and a background task performs the API calls in order. The
//! gateway awaits [`TelegramRenderer::flush`] at the end of a turn so
//! the answer is delivered before the turn is considered done.

use crate::telegram::chunk::split_message;
use crate::telegram::client::TelegramApiClient;
use purrcode_api::error::Result;
use purrcode_api::event::AgentEvent;
use purrcode_core::AgentEventListener;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// Minimum draft length before the first message is sent. Short
/// answers are sent whole at flush time instead of as a placeholder.
const DRAFT_SEND_THRESHOLD: usize = 200;
/// Minimum time between two edits of the same message (Telegram
/// rate-limits edits to roughly one per second).
const EDIT_THROTTLE: Duration = Duration::from_millis(1000);

/// One unit of rendering work, in order.
enum RenderOp {
    Text(String),
    ToolStarted {
        name: String,
    },
    ToolFinished {
        name: String,
        success: bool,
    },
    /// Flush the draft and ack via the enclosed sender when done.
    Flush(oneshot::Sender<()>),
}

/// Renders one chat turn into Telegram messages.
pub struct TelegramRenderer {
    api: Arc<TelegramApiClient>,
    chat_id: i64,
    tx: mpsc::UnboundedSender<RenderOp>,
}

impl TelegramRenderer {
    /// Create a renderer and spawn its worker task. The worker stops
    /// when the last `TelegramRenderer` clone is dropped (the channel
    /// closes) — the gateway should `flush().await` first.
    pub fn new(api: Arc<TelegramApiClient>, chat_id: i64) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let renderer = Arc::new(Self {
            api: api.clone(),
            chat_id,
            tx,
        });
        let worker_api = api;
        tokio::spawn(render_worker(worker_api, chat_id, rx));
        renderer
    }

    /// Append a text chunk to the draft (synchronous — used by the
    /// loop's listener callbacks).
    pub fn push_text(&self, chunk: &str) {
        let _ = self.tx.send(RenderOp::Text(chunk.to_string()));
    }

    /// Record a tool call starting (synchronous).
    pub fn tool_started(&self, name: &str) {
        let _ = self.tx.send(RenderOp::ToolStarted {
            name: name.to_string(),
        });
    }

    /// Record a tool call finishing (synchronous).
    pub fn tool_finished(&self, name: &str, success: bool) {
        let _ = self.tx.send(RenderOp::ToolFinished {
            name: name.to_string(),
            success,
        });
    }

    /// Flush the draft (send/edit the final text) and wait until the
    /// worker has delivered it.
    pub async fn flush(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.tx.send(RenderOp::Flush(ack_tx)).is_err() {
            return;
        }
        let _ = ack_rx.await;
    }

    /// Send one standalone message in this chat (plain text, chunked).
    /// Used for command replies and status lines outside a turn.
    pub async fn send_standalone(&self, text: &str) {
        for chunk in split_message(text) {
            if self
                .api
                .send_message(self.chat_id, &chunk, None)
                .await
                .is_err()
            {
                tracing::warn!("telegram: failed to send message to chat {}", self.chat_id);
                return;
            }
        }
    }
}

/// The worker: applies render ops in order against the Bot API.
async fn render_worker(
    api: Arc<TelegramApiClient>,
    chat_id: i64,
    mut rx: mpsc::UnboundedReceiver<RenderOp>,
) {
    let mut draft = String::new();
    let mut draft_message_id: Option<i64> = None;
    let mut last_edit = Instant::now() - EDIT_THROTTLE;
    let mut last_sent_len = 0usize;
    // Head chunks already sent mid-stream (finalized when the draft
    // outgrew one message). They gain their `(k/n)` marker at flush,
    // once the total is knowable.
    let mut sent_heads: Vec<(i64, String)> = Vec::new();

    while let Some(op) = rx.recv().await {
        match op {
            RenderOp::Text(chunk) => {
                draft.push_str(&chunk);
                maybe_send_or_edit(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_edit,
                    &mut last_sent_len,
                    &mut sent_heads,
                )
                .await;
            }
            RenderOp::ToolStarted { name } => {
                // Flush the current draft first so the tool line lands
                // AFTER the text written so far.
                flush_draft(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_sent_len,
                    &mut sent_heads,
                )
                .await;
                let line = format!("🔧 {name} — …");
                if api.send_message(chat_id, &line, None).await.is_err() {
                    tracing::warn!("telegram: failed to send tool line for '{name}'");
                }
            }
            RenderOp::ToolFinished { name, success } => {
                let line = format!("🔧 {name} — {}", if success { "ok" } else { "failed" });
                if api.send_message(chat_id, &line, None).await.is_err() {
                    tracing::warn!("telegram: failed to send tool result line for '{name}'");
                }
            }
            RenderOp::Flush(ack) => {
                flush_draft(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_sent_len,
                    &mut sent_heads,
                )
                .await;
                let _ = ack.send(());
            }
        }
    }
}

/// Send the draft as a first message once it is long enough, or edit
/// the existing draft message when it grew (throttled, only on change).
/// When the draft exceeds one message, the first chunk is finalized and
/// the remainder starts a new draft.
async fn maybe_send_or_edit(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &mut String,
    draft_message_id: &mut Option<i64>,
    last_edit: &mut Instant,
    last_sent_len: &mut usize,
    sent_heads: &mut Vec<(i64, String)>,
) {
    if draft_message_id.is_none() {
        if draft.chars().count() >= DRAFT_SEND_THRESHOLD {
            send_draft_head(api, chat_id, draft, draft_message_id, last_sent_len).await;
        }
        return;
    }
    if last_edit.elapsed() < EDIT_THROTTLE {
        return;
    }
    if draft.chars().count() == *last_sent_len {
        return; // nothing new to show
    }
    let Some(mid) = *draft_message_id else {
        return; // guarded above by the is_none() check
    };
    let head = draft_head_text(draft);
    if api
        .edit_message_text(chat_id, mid, &head, None)
        .await
        .is_ok()
    {
        *last_edit = Instant::now();
        *last_sent_len = head.chars().count();
    }
    if draft.chars().count() > crate::telegram::chunk::MAX_MESSAGE_CHARS {
        // The draft outgrew one message: finalize this chunk and start a
        // fresh draft with the remainder. The finalized head is recorded
        // so flush can give it its `(k/n)` marker once the total is known.
        if let Some(mid) = *draft_message_id {
            sent_heads.push((mid, draft_head_text(draft)));
        }
        let rest = draft[draft_head_byte_len(draft)..].to_string();
        *draft = rest;
        *draft_message_id = None;
        *last_sent_len = 0;
    }
}

/// Send the draft head as a new message (used for the first send and
/// for the remainder after a chunk boundary).
async fn send_draft_head(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &str,
    draft_message_id: &mut Option<i64>,
    last_sent_len: &mut usize,
) {
    let head = draft_head_text(draft);
    match api.send_message(chat_id, &head, None).await {
        Ok(msg) => {
            *draft_message_id = Some(msg.message_id);
            *last_sent_len = head.chars().count();
        }
        Err(e) => {
            tracing::warn!("telegram: failed to send draft message: {e}");
        }
    }
}

/// Flush the draft: finalize every part of the current text unit.
/// Parts already sent mid-stream (head chunks finalized when the draft
/// outgrew one message) are edited to gain their `(k/n)` marker — they
/// were sent before the total was knowable. The current head is edited
/// (or the whole draft sent) the same way, and the remainder is sent
/// chunked with offset numbering, so the whole answer reads
/// `(1/n)`…`(n/n)`. Leaves the renderer empty.
async fn flush_draft(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &mut String,
    draft_message_id: &mut Option<i64>,
    last_sent_len: &mut usize,
    sent_heads: &mut Vec<(i64, String)>,
) {
    if draft.is_empty() && sent_heads.is_empty() {
        return;
    }
    // The current head (if a draft message exists) plus the remainder
    // after it. With no draft message yet, the whole draft is the
    // remainder.
    let mut current_head: Option<(i64, String)> = None;
    let mut rest = String::new();
    if !draft.is_empty() {
        if let Some(mid) = *draft_message_id {
            current_head = Some((mid, draft_head_text(draft)));
            if draft.chars().count() > crate::telegram::chunk::MAX_MESSAGE_CHARS {
                rest = draft[draft_head_byte_len(draft)..].to_string();
            }
        } else {
            rest = std::mem::take(draft);
        }
    }
    let rest_count = if rest.is_empty() {
        0
    } else {
        crate::telegram::chunk::split_count(&rest)
    };
    let total = sent_heads.len() + usize::from(current_head.is_some()) + rest_count;
    // Finalized heads gain their marker now that the total is known.
    for (k, (mid, text)) in sent_heads.iter().enumerate() {
        if total > 1 {
            let _ = api
                .edit_message_text(
                    chat_id,
                    *mid,
                    &format!("{text}\n({}/{})", k + 1, total),
                    None,
                )
                .await;
        }
    }
    // The current head: edit to the full head (+ marker when multi-part).
    if let Some((mid, head)) = &current_head {
        if total > 1 || head.chars().count() != *last_sent_len {
            let target = if total > 1 {
                format!("{head}\n({}/{})", sent_heads.len() + 1, total)
            } else {
                head.clone()
            };
            let _ = api.edit_message_text(chat_id, *mid, &target, None).await;
        }
    }
    // The remainder, numbered after the parts already sent.
    if !rest.is_empty() {
        let start = sent_heads.len() + usize::from(current_head.is_some()) + 1;
        for chunk in crate::telegram::chunk::split_message_numbered(&rest, start, total) {
            if api.send_message(chat_id, &chunk, None).await.is_err() {
                break;
            }
        }
    }
    *draft = String::new();
    *draft_message_id = None;
    *last_sent_len = 0;
    sent_heads.clear();
}

/// The first chunk's worth of the draft, as plain text (v0.2 sends
/// without `parse_mode`, so no fence/marker decoration is needed on a
/// live draft — those exist only on final sends via `split_message`).
/// Slicing uses the chunk's content length, so no character is lost.
fn draft_head_text(draft: &str) -> String {
    let len = crate::telegram::chunk::first_chunk_content_len(draft);
    draft.chars().take(len).collect()
}

/// Byte length of [`draft_head_text`] (for remainder slicing).
fn draft_head_byte_len(draft: &str) -> usize {
    draft_head_text(draft).len()
}

/// The loop-facing listener: forwards the three callbacks into a
/// [`TelegramRenderer`].
pub struct TelegramEventListener {
    renderer: Arc<TelegramRenderer>,
}

impl TelegramEventListener {
    pub fn new(renderer: Arc<TelegramRenderer>) -> Self {
        Self { renderer }
    }

    pub fn renderer(&self) -> &Arc<TelegramRenderer> {
        &self.renderer
    }
}

impl AgentEventListener for TelegramEventListener {
    fn on_text_chunk(&self, chunk: &str, _is_reasoning: bool) {
        if _is_reasoning {
            return; // reasoning deltas are not streamed to the chat
        }
        self.renderer.push_text(chunk);
    }

    fn on_tool_call_started(&self, name: &str, _args: &str) {
        self.renderer.tool_started(name);
    }

    fn on_tool_call_finished(&self, name: &str, _output: &str, success: bool) {
        self.renderer.tool_finished(name, success);
    }
}

/// Render an [`AgentEvent`] into a chat (the `Channel::send_event`
/// path). Text chunks go through the draft; tool events become lines;
/// `TurnFinished` flushes. Events without a chat rendering (turn
/// start, approvals — those have their own cards) are ignored.
pub async fn render_event(renderer: &Arc<TelegramRenderer>, event: &AgentEvent) -> Result<()> {
    match event {
        AgentEvent::TextChunk { text } => {
            renderer.push_text(text);
        }
        AgentEvent::ToolCallStarted { tool, .. } => {
            renderer.tool_started(tool);
        }
        AgentEvent::ToolCallFinished { tool, success, .. } => {
            renderer.tool_finished(tool, *success);
        }
        AgentEvent::TurnFinished { .. } | AgentEvent::Error { .. } => {
            renderer.flush().await;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draft_head_is_the_first_chunk_content() {
        let long = "z".repeat(5000);
        let head = draft_head_text(&long);
        assert!(!head.contains("(1/2)"));
        // Capped at the chunk budget (4096 - 14 reserve) so a final
        // `(i/n)` marker still fits under the Telegram limit.
        assert_eq!(
            head.chars().count(),
            crate::telegram::chunk::MAX_MESSAGE_CHARS - 14
        );
        // Slicing at the head's byte length loses nothing.
        let rest = &long[draft_head_byte_len(&long)..];
        assert_eq!(head.len() + rest.len(), long.len());
        assert_eq!(rest.chars().count(), 5000 - 4082);
    }

    #[test]
    fn test_draft_head_short_is_whole() {
        assert_eq!(draft_head_text("hello"), "hello");
    }
}
