// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering agent events into Telegram messages (v0.2, ADR-013).
//!
//! One [`TelegramRenderer`] per chat turn. Text chunks accumulate into a
//! draft that is sent as a message once it is long enough, then edited
//! in place as it grows (throttled via `ui.stream_edit_ms`), and finalized
//! with Telegram HTML formatting when `ui.parse_mode == "html"`.

use crate::telegram::chunk::split_message;
use crate::telegram::client::TelegramApiClient;
use crate::telegram::format::{escape_html, markdown_to_telegram_html};
use purrcode_api::error::Result;
use purrcode_api::event::AgentEvent;
use purrcode_config::TelegramUiConfig;
use purrcode_core::AgentEventListener;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// Minimum draft length before the first message is sent. Short
/// answers are sent whole at flush time instead of as a placeholder.
const DRAFT_SEND_THRESHOLD: usize = 200;

/// One unit of rendering work, in order.
enum RenderOp {
    Text(String),
    Reasoning(String),
    ToolStarted {
        name: String,
        args: String,
    },
    ToolFinished {
        name: String,
        success: bool,
    },
    /// Flush the draft and ack via the enclosed sender when done.
    Flush(oneshot::Sender<()>),
}

pub struct TelegramRenderer {
    api: Arc<TelegramApiClient>,
    chat_id: i64,
    ui: TelegramUiConfig,
    reply_to_message_id: Option<i64>,
    tx: mpsc::UnboundedSender<RenderOp>,
}

impl TelegramRenderer {
    /// Create a renderer with default formatting settings.
    pub fn new(api: Arc<TelegramApiClient>, chat_id: i64) -> Arc<Self> {
        Self::with_config(api, chat_id, TelegramUiConfig::default(), None)
    }

    /// Create a renderer configured with [`TelegramUiConfig`] and optional reply anchor.
    pub fn with_config(
        api: Arc<TelegramApiClient>,
        chat_id: i64,
        ui: TelegramUiConfig,
        reply_to_message_id: Option<i64>,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let renderer = Arc::new(Self {
            api: api.clone(),
            chat_id,
            ui: ui.clone(),
            reply_to_message_id,
            tx,
        });
        tokio::spawn(render_worker(api, chat_id, ui, reply_to_message_id, rx));
        renderer
    }

    /// Append a text chunk to the draft (synchronous — used by the
    /// loop's listener callbacks).
    pub fn push_text(&self, chunk: &str) {
        let _ = self.tx.send(RenderOp::Text(chunk.to_string()));
    }

    /// Append a reasoning chunk when `ui.show_reasoning` is enabled.
    pub fn push_reasoning(&self, chunk: &str) {
        if self.ui.show_reasoning && !chunk.is_empty() {
            let _ = self.tx.send(RenderOp::Reasoning(chunk.to_string()));
        }
    }

    /// Record a tool call starting (synchronous).
    pub fn tool_started(&self, name: &str) {
        self.tool_started_with_args(name, "");
    }

    /// Record a tool call starting with its arguments preview.
    pub fn tool_started_with_args(&self, name: &str, args: &str) {
        let _ = self.tx.send(RenderOp::ToolStarted {
            name: name.to_string(),
            args: args.to_string(),
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

    /// Send one standalone message in this chat.
    pub async fn send_standalone(&self, text: &str) {
        let _ = self.send_standalone_reported(text).await;
    }

    /// Like `send_standalone`, formatting as Telegram HTML when configured.
    pub async fn send_standalone_reported(&self, text: &str) -> bool {
        let html_mode = self.ui.parse_mode.eq_ignore_ascii_case("html");
        let chunks = if html_mode {
            crate::telegram::format::split_markdown_as_html(text)
        } else {
            split_message(text)
        };
        let pm = if html_mode { Some("HTML") } else { None };
        for (idx, chunk) in chunks.iter().enumerate() {
            let reply_id = if idx == 0 {
                self.reply_to_message_id
            } else {
                None
            };
            if self
                .api
                .send_message_formatted(self.chat_id, chunk, pm, reply_id, None)
                .await
                .is_err()
            {
                tracing::warn!("telegram: failed to send message to chat {}", self.chat_id);
                return false;
            }
        }
        true
    }
}

/// The worker: applies render ops in order against the Bot API.
async fn render_worker(
    api: Arc<TelegramApiClient>,
    chat_id: i64,
    ui: TelegramUiConfig,
    reply_to_message_id: Option<i64>,
    mut rx: mpsc::UnboundedReceiver<RenderOp>,
) {
    let edit_throttle = Duration::from_millis(ui.stream_edit_ms.max(400));
    let mut draft = String::new();
    let mut reasoning_buf = String::new();
    let mut draft_message_id: Option<i64> = None;
    let mut last_edit = Instant::now() - edit_throttle;
    let mut last_sent_len = 0usize;
    let mut sent_heads: Vec<(i64, String)> = Vec::new();
    let mut used_reply_anchor = false;
    let html_mode = ui.parse_mode.eq_ignore_ascii_case("html");
    let verbosity = ui.tool_verbosity.to_ascii_lowercase();

    while let Some(op) = rx.recv().await {
        match op {
            RenderOp::Reasoning(chunk) => {
                reasoning_buf.push_str(&chunk);
            }
            RenderOp::Text(chunk) => {
                if ui.show_reasoning && !reasoning_buf.trim().is_empty() {
                    let thought = std::mem::take(&mut reasoning_buf);
                    let formatted = if html_mode {
                        format!(
                            "💭 <blockquote expandable>{}</blockquote>",
                            escape_html(thought.trim())
                        )
                    } else {
                        format!("💭 {}", thought.trim())
                    };
                    let reply_id = if !used_reply_anchor {
                        used_reply_anchor = true;
                        reply_to_message_id
                    } else {
                        None
                    };
                    let _ = api
                        .send_message_formatted(
                            chat_id,
                            &formatted,
                            if html_mode { Some("HTML") } else { None },
                            reply_id,
                            None,
                        )
                        .await;
                }
                draft.push_str(&chunk);
                let reply_id = if !used_reply_anchor && draft_message_id.is_none() {
                    reply_to_message_id
                } else {
                    None
                };
                if maybe_send_or_edit(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_edit,
                    edit_throttle,
                    &mut last_sent_len,
                    &mut sent_heads,
                    reply_id,
                )
                .await
                {
                    used_reply_anchor = true;
                }
            }
            RenderOp::ToolStarted { name, args } => {
                flush_draft(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_sent_len,
                    &mut sent_heads,
                    html_mode,
                    if !used_reply_anchor {
                        used_reply_anchor = true;
                        reply_to_message_id
                    } else {
                        None
                    },
                )
                .await;
                if verbosity == "minimal" {
                    let _ = api.send_chat_action(chat_id, "typing").await;
                    continue;
                }
                let line = if verbosity == "verbose" && !args.trim().is_empty() {
                    let short_args: String = args.trim().chars().take(120).collect();
                    if html_mode {
                        format!(
                            "🔧 <code>{}</code> <i>{}</i> — …",
                            escape_html(&name),
                            escape_html(&short_args)
                        )
                    } else {
                        format!("🔧 {name} ({short_args}) — …")
                    }
                } else if html_mode {
                    format!("🔧 <code>{}</code> — …", escape_html(&name))
                } else {
                    format!("🔧 {name} — …")
                };
                let pm = if html_mode { Some("HTML") } else { None };
                if api
                    .send_message_formatted(chat_id, &line, pm, None, None)
                    .await
                    .is_err()
                {
                    tracing::warn!("telegram: failed to send tool line for '{name}'");
                }
            }
            RenderOp::ToolFinished { name, success } => {
                if verbosity == "minimal" {
                    continue;
                }
                let status = if success { "ok" } else { "failed" };
                let line = if html_mode {
                    format!(
                        "{} <code>{}</code> — {status}",
                        if success { "✅" } else { "❌" },
                        escape_html(&name)
                    )
                } else {
                    format!("🔧 {name} — {status}")
                };
                let pm = if html_mode { Some("HTML") } else { None };
                if api
                    .send_message_formatted(chat_id, &line, pm, None, None)
                    .await
                    .is_err()
                {
                    tracing::warn!("telegram: failed to send tool result line for '{name}'");
                }
            }
            RenderOp::Flush(ack) => {
                if ui.show_reasoning && !reasoning_buf.trim().is_empty() {
                    let thought = std::mem::take(&mut reasoning_buf);
                    let formatted = if html_mode {
                        format!(
                            "💭 <blockquote expandable>{}</blockquote>",
                            escape_html(thought.trim())
                        )
                    } else {
                        format!("💭 {}", thought.trim())
                    };
                    let reply_id = if !used_reply_anchor {
                        used_reply_anchor = true;
                        reply_to_message_id
                    } else {
                        None
                    };
                    let _ = api
                        .send_message_formatted(
                            chat_id,
                            &formatted,
                            if html_mode { Some("HTML") } else { None },
                            reply_id,
                            None,
                        )
                        .await;
                }
                flush_draft(
                    &api,
                    chat_id,
                    &mut draft,
                    &mut draft_message_id,
                    &mut last_sent_len,
                    &mut sent_heads,
                    html_mode,
                    if !used_reply_anchor {
                        used_reply_anchor = true;
                        reply_to_message_id
                    } else {
                        None
                    },
                )
                .await;
                let _ = ack.send(());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn maybe_send_or_edit(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &mut String,
    draft_message_id: &mut Option<i64>,
    last_edit: &mut Instant,
    edit_throttle: Duration,
    last_sent_len: &mut usize,
    sent_heads: &mut Vec<(i64, String)>,
    reply_to_message_id: Option<i64>,
) -> bool {
    let mut sent_new = false;
    if draft_message_id.is_none() {
        if draft.chars().count() >= DRAFT_SEND_THRESHOLD {
            sent_new = send_draft_head(
                api,
                chat_id,
                draft,
                draft_message_id,
                last_sent_len,
                reply_to_message_id,
            )
            .await;
        }
        return sent_new;
    }
    if last_edit.elapsed() < edit_throttle {
        return false;
    }
    if draft.chars().count() == *last_sent_len {
        return false;
    }
    let Some(mid) = *draft_message_id else {
        return false;
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
        if let Some(mid) = *draft_message_id {
            sent_heads.push((mid, draft_head_text(draft)));
        }
        let rest = draft[draft_head_byte_len(draft)..].to_string();
        *draft = rest;
        *draft_message_id = None;
        *last_sent_len = 0;
    }
    false
}

async fn send_draft_head(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &str,
    draft_message_id: &mut Option<i64>,
    last_sent_len: &mut usize,
    reply_to_message_id: Option<i64>,
) -> bool {
    let head = draft_head_text(draft);
    match api
        .send_message_formatted(chat_id, &head, None, reply_to_message_id, None)
        .await
    {
        Ok(msg) => {
            *draft_message_id = Some(msg.message_id);
            *last_sent_len = head.chars().count();
            true
        }
        Err(e) => {
            tracing::warn!("telegram: failed to send draft message: {e}");
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn flush_draft(
    api: &Arc<TelegramApiClient>,
    chat_id: i64,
    draft: &mut String,
    draft_message_id: &mut Option<i64>,
    last_sent_len: &mut usize,
    sent_heads: &mut Vec<(i64, String)>,
    html_mode: bool,
    reply_to_message_id: Option<i64>,
) {
    if draft.is_empty() && sent_heads.is_empty() {
        return;
    }
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
    let pm = if html_mode { Some("HTML") } else { None };
    let format_chunk = |raw: &str| -> String {
        if html_mode {
            let h = markdown_to_telegram_html(raw);
            if h.chars().count() <= crate::telegram::chunk::MAX_MESSAGE_CHARS {
                h
            } else {
                raw.to_string()
            }
        } else {
            raw.to_string()
        }
    };

    for (k, (mid, text)) in sent_heads.iter().enumerate() {
        let base = if total > 1 {
            format!("{text}\n({}/{})", k + 1, total)
        } else {
            text.clone()
        };
        if total > 1 || html_mode {
            let formatted = format_chunk(&base);
            let _ = api
                .edit_message_text_formatted(chat_id, *mid, &formatted, pm, None)
                .await;
        }
    }
    if let Some((mid, head)) = &current_head {
        if total > 1 || head.chars().count() != *last_sent_len || html_mode {
            let target = if total > 1 {
                format!("{head}\n({}/{})", sent_heads.len() + 1, total)
            } else {
                head.clone()
            };
            let formatted = format_chunk(&target);
            let _ = api
                .edit_message_text_formatted(chat_id, *mid, &formatted, pm, None)
                .await;
        }
    }
    if !rest.is_empty() {
        let start = sent_heads.len() + usize::from(current_head.is_some()) + 1;
        for (idx, chunk) in crate::telegram::chunk::split_message_numbered(&rest, start, total)
            .into_iter()
            .enumerate()
        {
            let reply_id = if idx == 0 && sent_heads.is_empty() && current_head.is_none() {
                reply_to_message_id
            } else {
                None
            };
            let formatted = format_chunk(&chunk);
            if api
                .send_message_formatted(chat_id, &formatted, pm, reply_id, None)
                .await
                .is_err()
            {
                break;
            }
        }
    }
    *draft = String::new();
    *draft_message_id = None;
    *last_sent_len = 0;
    sent_heads.clear();
}

/// The first chunk's worth of the draft.
fn draft_head_text(draft: &str) -> String {
    let len = crate::telegram::chunk::first_chunk_content_len(draft);
    draft.chars().take(len).collect()
}

/// Byte length of [`draft_head_text`] (for remainder slicing).
fn draft_head_byte_len(draft: &str) -> usize {
    draft_head_text(draft).len()
}

/// The loop-facing listener: forwards callbacks into a [`TelegramRenderer`].
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
    fn on_text_chunk(&self, chunk: &str, is_reasoning: bool) {
        if is_reasoning {
            self.renderer.push_reasoning(chunk);
        } else {
            self.renderer.push_text(chunk);
        }
    }

    fn on_tool_call_started(&self, name: &str, args: &str) {
        self.renderer.tool_started_with_args(name, args);
    }

    fn on_tool_call_finished(&self, name: &str, _output: &str, success: bool) {
        self.renderer.tool_finished(name, success);
    }
}

/// Render an [`AgentEvent`] into a chat.
pub async fn render_event(renderer: &Arc<TelegramRenderer>, event: &AgentEvent) -> Result<()> {
    match event {
        AgentEvent::TextChunk { text } => {
            renderer.push_text(text);
        }
        AgentEvent::ReasoningChunk { thinking } => {
            renderer.push_reasoning(thinking);
        }
        AgentEvent::ToolCallStarted {
            tool, arguments, ..
        } => {
            renderer.tool_started_with_args(tool, &arguments.to_string());
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
