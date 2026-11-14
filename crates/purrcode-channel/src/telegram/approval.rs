// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline approval + ask over Telegram (v0.2, ADR-013).
//!
//! One shared mechanism for both: the gateway sends a message with an
//! inline keyboard, registers a pending entry keyed by a short token,
//! and the poll loop resolves it when the user taps a button (or when
//! the timeout fires). Resolution is single-claim — whoever removes the
//! pending entry first (a tap, or the timeout) decides the outcome, so
//! a tap in the last second cannot race the timeout.
//!
//! `callback_data` is at most 64 bytes (Bot API limit), so only a short
//! token + choice travel in the button; the full context stays in
//! memory. Approval tokens look like `ap:<12hex>:y|n|a`, ask tokens
//! like `ak:<12hex>:<index>`.

use crate::telegram::client::TelegramApiClient;
use crate::telegram::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use purrcode_api::tool::AuditSink;
use purrcode_core::ApprovalRequester;
use purrcode_tools::coordination::ask::{AskOption, AskRequester};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

/// The outcome of an approval card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Deny,
    /// Approve this tool for the rest of the chat session.
    Always,
}

/// A pending-request registry: token → one-shot sender. Generic over
/// the resolved value so approval and ask share the machinery.
pub struct Pending<T> {
    inner: Mutex<HashMap<String, oneshot::Sender<T>>>,
}

impl<T> Pending<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }

    /// Register a request. Returns a fresh unique token (12 hex chars)
    /// and the receiver the waiter awaits on.
    pub async fn register(&self) -> (String, oneshot::Receiver<T>) {
        let mut guard = self.inner.lock().await;
        let token = loop {
            let candidate = uuid::Uuid::new_v4().to_string().replace('-', "");
            let candidate = candidate[..12].to_string();
            if !guard.contains_key(&candidate) {
                break candidate;
            }
        };
        let (tx, rx) = oneshot::channel();
        guard.insert(token.clone(), tx);
        (token, rx)
    }

    /// Resolve a pending request. Returns `false` when no entry exists
    /// for the token (already claimed by the other side, or unknown).
    pub async fn resolve(&self, token: &str, value: T) -> bool {
        let tx = self.inner.lock().await.remove(token);
        match tx {
            Some(tx) => tx.send(value).is_ok(),
            None => false,
        }
    }

    /// Claim (remove) a pending entry WITHOUT resolving it — the
    /// receiver then sees a dropped sender. Used by the timeout path:
    /// after a claim, a late tap finds no entry and is ignored.
    /// Returns `false` when nothing was claimed.
    pub async fn claim(&self, token: &str) -> bool {
        self.inner.lock().await.remove(token).is_some()
    }
}

/// Per-chat-session "always allow" sets: `(chat_id, session_id)` →
/// `{tool, ...}`. An approved `Always` decision adds the tool here, so
/// later calls for the same tool in the same chat session never show a
/// card. Keyed by session as well as chat: a `/new` session starts
/// with a clean slate (the button says "sesi ini" — an Always from a
/// previous session must not silently auto-approve in the new one).
#[derive(Debug, Default)]
pub struct AlwaysSet {
    inner: Mutex<HashMap<(i64, String), HashSet<String>>>,
}

impl AlwaysSet {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }

    pub async fn is_allowed(&self, chat_id: i64, session_id: &str, tool: &str) -> bool {
        self.inner
            .lock()
            .await
            .get(&(chat_id, session_id.to_string()))
            .map(|set| set.contains(tool))
            .unwrap_or(false)
    }

    pub async fn allow(&self, chat_id: i64, session_id: &str, tool: &str) {
        self.inner
            .lock()
            .await
            .entry((chat_id, session_id.to_string()))
            .or_default()
            .insert(tool.to_string());
    }
}

/// Sends approval cards to one Telegram chat and waits for the tap.
/// Implements the loop's [`ApprovalRequester`] (bool), so the gateway
/// wires it exactly like the TUI/CLI requesters.
pub struct TelegramApprovalRequester {
    api: Arc<TelegramApiClient>,
    pending: Arc<Pending<ApprovalDecision>>,
    always: Arc<AlwaysSet>,
    chat_id: i64,
    session_id: String,
    timeout: Duration,
    audit_sink: Option<Arc<dyn AuditSink>>,
}

impl TelegramApprovalRequester {
    pub fn new(
        api: Arc<TelegramApiClient>,
        pending: Arc<Pending<ApprovalDecision>>,
        always: Arc<AlwaysSet>,
        chat_id: i64,
        session_id: impl Into<String>,
        timeout: Duration,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        Self {
            api,
            pending,
            always,
            chat_id,
            session_id: session_id.into(),
            timeout,
            audit_sink,
        }
    }

    fn audit(&self, tool: &str, decision: &str, detail: &str) {
        if let Some(sink) = &self.audit_sink {
            // A dedicated event: the loop ALSO logs its generic
            // `tool_approval` entry (approved/denied by bool). This one
            // records the SOURCE — operator vs timeout vs always — which
            // the bool cannot carry (ADR-013).
            sink.record_event(
                &self.session_id,
                "telegram",
                "telegram_approval",
                Some(tool),
                decision,
                detail,
            );
        }
    }
}

#[async_trait::async_trait]
impl ApprovalRequester for TelegramApprovalRequester {
    async fn request_approval(
        &self,
        _call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool {
        // `Always` from an earlier card in this chat session: no card,
        // no wait.
        if self
            .always
            .is_allowed(self.chat_id, &self.session_id, tool)
            .await
        {
            self.audit(
                tool,
                "approved_always",
                "tool allowed for this chat session",
            );
            return true;
        }

        let (token, rx) = self.pending.register().await;
        let mut card = format!("⚠️ Approval Required\nTool: {tool}\n{description}");
        if let Some(diff) = preview {
            // Plain text (no parse_mode): no ``` fences — they would
            // show as literal backticks in the Telegram app.
            let shown: String = diff.chars().take(400).collect();
            card.push_str(&format!("\n\nPreview:\n{shown}"));
        }
        card.push_str(&format!(
            "\n\nTap below. Expires in {}s — no tap denies automatically.",
            self.timeout.as_secs()
        ));
        let keyboard = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton {
                        text: "✅ Ya".to_string(),
                        callback_data: format!("ap:{token}:y"),
                    },
                    InlineKeyboardButton {
                        text: "❌ Tidak".to_string(),
                        callback_data: format!("ap:{token}:n"),
                    },
                ],
                vec![InlineKeyboardButton {
                    text: "✦ Selalu (sesi ini)".to_string(),
                    callback_data: format!("ap:{token}:a"),
                }],
            ],
        };
        let card_msg = match self
            .api
            .send_message(self.chat_id, &card, Some(keyboard))
            .await
        {
            Ok(msg) => Some(msg.message_id),
            Err(e) => {
                tracing::warn!("telegram: failed to send approval card for '{tool}': {e}");
                // Fail-closed: if the card never reached the user, deny.
                self.pending.claim(&token).await;
                self.audit(
                    tool,
                    "denied_unreachable",
                    "approval card could not be delivered",
                );
                return false;
            }
        };

        let decision = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(d)) => d,
            Ok(Err(_)) => {
                // Sender dropped without resolving (channel shutting
                // down): deny, fail-closed.
                self.audit(tool, "denied_unreachable", "approval channel closed");
                return false;
            }
            Err(_) => {
                // Timeout: claim the entry (single-claim — a tap landing
                // in the same instant loses to the claim and is ignored),
                // then deny.
                self.pending.claim(&token).await;
                if let Some(mid) = card_msg {
                    let _ = self
                        .api
                        .edit_message_text(
                            self.chat_id,
                            mid,
                            "⏱ Approval expired — denied automatically (timeout).",
                            None,
                        )
                        .await;
                }
                self.audit(tool, "denied_timeout", "no tap within the timeout");
                return false;
            }
        };

        // Best-effort card update so the chat shows what happened.
        if let Some(mid) = card_msg {
            let resolved = match decision {
                ApprovalDecision::Approve => "✅ Approved.",
                ApprovalDecision::Deny => "❌ Denied.",
                ApprovalDecision::Always => "✦ Always allowed for this session.",
            };
            let _ = self
                .api
                .edit_message_text(self.chat_id, mid, resolved, None)
                .await;
        }
        match decision {
            ApprovalDecision::Approve => {
                self.audit(tool, "approved_by_user", "tapped Ya");
                true
            }
            ApprovalDecision::Deny => {
                self.audit(tool, "denied_by_user", "tapped Tidak");
                false
            }
            ApprovalDecision::Always => {
                self.always
                    .allow(self.chat_id, &self.session_id, tool)
                    .await;
                self.audit(tool, "approved_always", "tapped Selalu (sesi ini)");
                true
            }
        }
    }
}

/// The always-set key for skill drafts. A dedicated key (not
/// `manage_skill`) so that "Selalu" on a skill-draft card
/// auto-approves future drafts only — never ordinary `manage_skill`
/// tool calls in the same session (v0.3 F20, ADR-014 item 7).
pub const SKILL_DRAFT_TOOL: &str = "skill_draft";

/// Asks one Telegram chat to approve saving a staged skill draft
/// (v0.3 F20). Mirrors [`TelegramApprovalRequester`] exactly —
/// same pending machinery, same 120s timeout → deny, same
/// single-claim resolution, same card-update best effort — but with
/// the `sk:` callback prefix and the `skill_draft` always-key, so a
/// draft approval never shares state with a tool approval. The audit
/// event is `telegram_skill_approval` (same decision vocabulary as
/// `telegram_approval`).
pub struct SkillDraftRequester {
    api: Arc<TelegramApiClient>,
    pending: Arc<Pending<ApprovalDecision>>,
    always: Arc<AlwaysSet>,
    chat_id: i64,
    session_id: String,
    timeout: Duration,
    audit_sink: Option<Arc<dyn AuditSink>>,
}

impl SkillDraftRequester {
    pub fn new(
        api: Arc<TelegramApiClient>,
        pending: Arc<Pending<ApprovalDecision>>,
        always: Arc<AlwaysSet>,
        chat_id: i64,
        session_id: impl Into<String>,
        timeout: Duration,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        Self {
            api,
            pending,
            always,
            chat_id,
            session_id: session_id.into(),
            timeout,
            audit_sink,
        }
    }

    fn audit(&self, decision: &str, detail: &str) {
        if let Some(sink) = &self.audit_sink {
            sink.record_event(
                &self.session_id,
                "telegram",
                "telegram_skill_approval",
                Some(SKILL_DRAFT_TOOL),
                decision,
                detail,
            );
        }
    }

    /// Ask the user to approve saving the staged draft `name`
    /// (`description` is the draft's one-line description, `preview`
    /// the SKILL.md body). Returns true on Ya or Selalu, false on
    /// Tidak, timeout, or an undeliverable card (fail-closed).
    pub async fn request_approval(
        &self,
        name: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool {
        if self
            .always
            .is_allowed(self.chat_id, &self.session_id, SKILL_DRAFT_TOOL)
            .await
        {
            self.audit(
                "approved_always",
                "skill drafts allowed for this chat session",
            );
            return true;
        }

        let (token, rx) = self.pending.register().await;
        let mut card = format!("💡 Skill draft\nName: {name}\n{description}");
        if let Some(diff) = preview {
            let shown: String = diff.chars().take(400).collect();
            card.push_str(&format!("\n\nPreview:\n{shown}"));
        }
        card.push_str(&format!(
            "\n\nTap below to save it as a live skill. Expires in {}s — no tap denies automatically.",
            self.timeout.as_secs()
        ));
        let keyboard = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton {
                        text: "✅ Ya".to_string(),
                        callback_data: format!("sk:{token}:y"),
                    },
                    InlineKeyboardButton {
                        text: "❌ Tidak".to_string(),
                        callback_data: format!("sk:{token}:n"),
                    },
                ],
                vec![InlineKeyboardButton {
                    text: "✦ Selalu (sesi ini)".to_string(),
                    callback_data: format!("sk:{token}:a"),
                }],
            ],
        };
        let card_msg = match self
            .api
            .send_message(self.chat_id, &card, Some(keyboard))
            .await
        {
            Ok(msg) => Some(msg.message_id),
            Err(e) => {
                tracing::warn!("telegram: failed to send skill draft card for '{name}': {e}");
                self.pending.claim(&token).await;
                self.audit(
                    "denied_unreachable",
                    "skill draft card could not be delivered",
                );
                return false;
            }
        };

        let decision = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(d)) => d,
            Ok(Err(_)) => {
                self.audit("denied_unreachable", "approval channel closed");
                return false;
            }
            Err(_) => {
                self.pending.claim(&token).await;
                if let Some(mid) = card_msg {
                    let _ = self
                        .api
                        .edit_message_text(
                            self.chat_id,
                            mid,
                            "⏱ Skill draft expired — denied automatically (timeout).",
                            None,
                        )
                        .await;
                }
                self.audit("denied_timeout", "no tap within the timeout");
                return false;
            }
        };

        if let Some(mid) = card_msg {
            let resolved = match decision {
                ApprovalDecision::Approve => "✅ Skill draft approved — saving…",
                ApprovalDecision::Deny => "❌ Skill draft discarded.",
                ApprovalDecision::Always => "✦ Skill drafts auto-saved for this session.",
            };
            let _ = self
                .api
                .edit_message_text(self.chat_id, mid, resolved, None)
                .await;
        }
        match decision {
            ApprovalDecision::Approve => {
                self.audit("approved_by_user", "tapped Ya");
                true
            }
            ApprovalDecision::Deny => {
                self.audit("denied_by_user", "tapped Tidak");
                false
            }
            ApprovalDecision::Always => {
                self.always
                    .allow(self.chat_id, &self.session_id, SKILL_DRAFT_TOOL)
                    .await;
                self.audit("approved_always", "tapped Selalu (sesi ini)");
                true
            }
        }
    }
}

/// Sends an `ask` picker to one Telegram chat and waits for the tap.
/// Implements the tools crate's [`AskRequester`]; a timeout (or a
/// dropped channel) answers `None`, which the `ask` tool surfaces as
/// "no answer".
pub struct TelegramAskRequester {
    api: Arc<TelegramApiClient>,
    pending: Arc<Pending<Option<usize>>>,
    chat_id: i64,
    timeout: Duration,
}

impl TelegramAskRequester {
    pub fn new(
        api: Arc<TelegramApiClient>,
        pending: Arc<Pending<Option<usize>>>,
        chat_id: i64,
        timeout: Duration,
    ) -> Self {
        Self {
            api,
            pending,
            chat_id,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl AskRequester for TelegramAskRequester {
    async fn request_decision(&self, question: &str, options: &[AskOption]) -> Option<String> {
        if options.is_empty() {
            return None;
        }
        let (token, rx) = self.pending.register().await;
        let mut buttons: Vec<InlineKeyboardButton> = Vec::new();
        for (i, opt) in options.iter().enumerate() {
            // Button label: the option text, truncated so callback_data
            // (token + index) stays far under the 64-byte limit.
            let label: String = opt.label.chars().take(30).collect();
            buttons.push(InlineKeyboardButton {
                text: label,
                callback_data: format!("ak:{token}:{i}"),
            });
        }
        // Two buttons per row for compactness.
        let rows: Vec<Vec<InlineKeyboardButton>> = buttons.chunks(2).map(|c| c.to_vec()).collect();
        let keyboard = InlineKeyboardMarkup {
            inline_keyboard: rows,
        };
        let card = format!("❓ {question}\n\nTap an option below.");
        let card_msg = match self
            .api
            .send_message(self.chat_id, &card, Some(keyboard))
            .await
        {
            Ok(msg) => Some(msg.message_id),
            Err(e) => {
                tracing::warn!("telegram: failed to send ask card: {e}");
                self.pending.claim(&token).await;
                return None;
            }
        };
        let chosen = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(idx)) => idx,
            Ok(Err(_)) => {
                tracing::warn!("telegram: ask channel closed");
                return None;
            }
            Err(_) => {
                self.pending.claim(&token).await;
                if let Some(mid) = card_msg {
                    let _ = self
                        .api
                        .edit_message_text(self.chat_id, mid, "⏱ No answer (timeout).", None)
                        .await;
                }
                return None;
            }
        };
        if let Some(mid) = card_msg {
            let resolved = match chosen {
                Some(i) => options
                    .get(i)
                    .map(|o| format!("✅ Chosen: {}", o.label))
                    .unwrap_or_else(|| "✅ Chosen.".to_string()),
                None => "❌ No option chosen.".to_string(),
            };
            let _ = self
                .api
                .edit_message_text(self.chat_id, mid, &resolved, None)
                .await;
        }
        // The ask tool expects the selected option's `id` (it looks the
        // label up itself, like the TUI requester does).
        chosen.and_then(|i| options.get(i).map(|o| o.id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pending_register_resolve_is_single_claim() {
        let pending: Arc<Pending<ApprovalDecision>> = Pending::new();
        let (token, rx) = pending.register().await;
        assert_eq!(token.len(), 12);
        assert!(pending.resolve(&token, ApprovalDecision::Approve).await);
        // Second resolve finds nothing — the entry is gone.
        assert!(!pending.resolve(&token, ApprovalDecision::Deny).await);
        assert_eq!(rx.await.unwrap(), ApprovalDecision::Approve);
    }

    #[tokio::test]
    async fn test_pending_claim_drops_receiver() {
        let pending: Arc<Pending<ApprovalDecision>> = Pending::new();
        let (token, rx) = pending.register().await;
        assert!(pending.claim(&token).await);
        assert!(!pending.claim(&token).await);
        assert!(rx.await.is_err(), "dropped sender must end the wait");
    }

    #[tokio::test]
    async fn test_pending_tokens_are_unique() {
        let pending: Arc<Pending<ApprovalDecision>> = Pending::new();
        let mut tokens = HashSet::new();
        for _ in 0..200 {
            let (token, _rx) = pending.register().await;
            assert!(tokens.insert(token), "duplicate token generated");
        }
    }

    #[tokio::test]
    async fn test_always_set_is_per_chat_session() {
        let always = AlwaysSet::new();
        always.allow(100, "sesi-a", "write").await;
        assert!(always.is_allowed(100, "sesi-a", "write").await);
        // A new session in the same chat starts clean.
        assert!(!always.is_allowed(100, "sesi-b", "write").await);
        assert!(!always.is_allowed(200, "sesi-a", "write").await);
        assert!(!always.is_allowed(100, "sesi-a", "bash").await);
    }
}
