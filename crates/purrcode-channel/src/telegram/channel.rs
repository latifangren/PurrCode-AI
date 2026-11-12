// SPDX-License-Identifier: GPL-3.0-or-later

use super::client::TgClient;
use super::types::InlineKeyboardMarkup;
use async_trait::async_trait;
use purrcode_api::channel::{Channel, ChannelCapabilities};
use purrcode_api::error::Result;
use purrcode_api::event::AgentEvent;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex, RwLock};

#[async_trait]
pub trait TelegramMessageHandler: Send + Sync {
    async fn handle_message(
        &self,
        chat_id: i64,
        user_id: i64,
        username: Option<&str>,
        text: &str,
    ) -> Result<String>;
    async fn handle_command(&self, chat_id: i64, command: &str, args: &str) -> Result<String>;
}

#[derive(Clone)]
pub struct TelegramChannel {
    client: Arc<TgClient>,
    allowed_users: Vec<String>,
    active_chat_id: Arc<AtomicI64>,
    pending_approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    #[allow(dead_code)]
    active_stream_msg_id: Arc<Mutex<Option<i64>>>,
    #[allow(dead_code)]
    stream_buffer: Arc<RwLock<String>>,
}

impl TelegramChannel {
    pub fn new(
        token: impl Into<String>,
        allowed_users: Vec<String>,
        base_url: Option<String>,
    ) -> Self {
        let client = Arc::new(TgClient::new(token, base_url));
        if allowed_users.is_empty() {
            tracing::warn!(
                "SECURITY WARNING: Telegram bot has no 'allowed_users' configured! \
                Anyone on Telegram who messages the bot can trigger actions. \
                Please set 'telegram.allowed_users' in your configuration."
            );
        }
        Self {
            client,
            allowed_users,
            active_chat_id: Arc::new(AtomicI64::new(0)),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            active_stream_msg_id: Arc::new(Mutex::new(None)),
            stream_buffer: Arc::new(RwLock::new(String::new())),
        }
    }

    pub fn set_active_chat_id(&self, chat_id: i64) {
        self.active_chat_id.store(chat_id, Ordering::SeqCst);
    }

    pub fn get_active_chat_id(&self) -> i64 {
        self.active_chat_id.load(Ordering::SeqCst)
    }

    pub fn is_user_allowed(&self, user_id: i64, username: Option<&str>) -> bool {
        if self.allowed_users.is_empty() {
            return true;
        }
        let uid_str = user_id.to_string();
        for allowed in &self.allowed_users {
            let clean = allowed.trim().trim_start_matches('@');
            if clean == uid_str {
                return true;
            }
            if let Some(uname) = username {
                if clean.eq_ignore_ascii_case(uname.trim_start_matches('@')) {
                    return true;
                }
            }
        }
        false
    }

    pub async fn send_chat_message(&self, chat_id: i64, text: &str) -> Result<()> {
        let _ = self.client.send_message(chat_id, text, None).await?;
        Ok(())
    }

    pub async fn resolve_approval(&self, call_id: &str, approved: bool) -> bool {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(tx) = pending.remove(call_id) {
            tx.send(approved).is_ok()
        } else {
            false
        }
    }

    pub async fn run_polling_loop(
        self: Arc<Self>,
        handler: Arc<dyn TelegramMessageHandler>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut offset: Option<i64> = None;
        tracing::info!("Starting Telegram Bot long-polling loop...");

        loop {
            tokio::select! {
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        tracing::info!("Telegram polling loop received shutdown signal");
                        break;
                    }
                }
                res = self.client.get_updates(offset, 20) => {
                    match res {
                        Ok(updates) => {
                            for update in updates {
                                offset = Some(update.update_id + 1);

                                // 1. Handle Callback Query (Approval Buttons)
                                if let Some(cb) = update.callback_query {
                                    let user_id = cb.from.id;
                                    let username = cb.from.username.as_deref();
                                    if !self.is_user_allowed(user_id, username) {
                                        let _ = self.client.answer_callback_query(&cb.id, Some("Unauthorized user")).await;
                                        continue;
                                    }

                                    if let Some(data) = cb.data {
                                        if let Some(call_id) = data.strip_prefix("allow:") {
                                            self.resolve_approval(call_id, true).await;
                                            let _ = self.client.answer_callback_query(&cb.id, Some("Action Approved ✅")).await;
                                            if let Some(msg) = cb.message {
                                                let updated_text = format!("{}\n\n[Status: Approved by @{}]", msg.text.unwrap_or_default(), username.unwrap_or("user"));
                                                let _ = self.client.edit_message_text(msg.chat.id, msg.message_id, &updated_text, None).await;
                                            }
                                        } else if let Some(call_id) = data.strip_prefix("deny:") {
                                            self.resolve_approval(call_id, false).await;
                                            let _ = self.client.answer_callback_query(&cb.id, Some("Action Denied ❌")).await;
                                            if let Some(msg) = cb.message {
                                                let updated_text = format!("{}\n\n[Status: Denied by @{}]", msg.text.unwrap_or_default(), username.unwrap_or("user"));
                                                let _ = self.client.edit_message_text(msg.chat.id, msg.message_id, &updated_text, None).await;
                                            }
                                        }
                                    }
                                    continue;
                                }

                                // 2. Handle Incoming Chat Message
                                if let Some(msg) = update.message {
                                    let chat_id = msg.chat.id;
                                    self.set_active_chat_id(chat_id);

                                    let user = match msg.from {
                                        Some(u) => u,
                                        None => continue,
                                    };

                                    if !self.is_user_allowed(user.id, user.username.as_deref()) {
                                        let _ = self.client.send_message(
                                            chat_id,
                                            "⚠️ Unauthorized: Your Telegram account is not permitted to access this PurrCode AI gateway.",
                                            None,
                                        ).await;
                                        continue;
                                    }

                                    let text = match msg.text {
                                        Some(t) => t,
                                        None => continue,
                                    };

                                    let trimmed = text.trim();
                                    if trimmed.starts_with('/') {
                                        let mut parts = trimmed.splitn(2, ' ');
                                        let cmd = parts.next().unwrap_or("").to_lowercase();
                                        let args = parts.next().unwrap_or("").trim();

                                        let reply = match handler.handle_command(chat_id, &cmd, args).await {
                                            Ok(rep) => rep,
                                            Err(e) => format!("Command error: {}", e),
                                        };
                                        let _ = self.client.send_message(chat_id, &reply, None).await;
                                    } else {
                                        // Send initial acknowledgement
                                        let _ = self.client.send_message(chat_id, "🐾 Processing request...", None).await;

                                        let this_channel = self.clone();
                                        let this_handler = handler.clone();
                                        let prompt_text = trimmed.to_string();
                                        let user_name = user.username.clone();
                                        tokio::spawn(async move {
                                            match this_handler.handle_message(chat_id, user.id, user_name.as_deref(), &prompt_text).await {
                                                Ok(reply) => {
                                                    let _ = this_channel.client.send_message(chat_id, &reply, None).await;
                                                }
                                                Err(e) => {
                                                    let _ = this_channel.client.send_message(chat_id, &format!("❌ Agent error: {}", e), None).await;
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Telegram polling error: {}", e);
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
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

    async fn send_event(&self, event: &AgentEvent) -> Result<()> {
        let chat_id = self.get_active_chat_id();
        if chat_id == 0 {
            return Ok(());
        }

        match event {
            AgentEvent::ToolCallStarted {
                tool, arguments, ..
            } => {
                let args_str = serde_json::to_string_pretty(arguments).unwrap_or_default();
                let truncated = if args_str.len() > 500 {
                    format!("{}...", &args_str[..500])
                } else {
                    args_str
                };
                let msg = format!(
                    "🔧 *Executing tool:* `{}`\n```json\n{}\n```",
                    tool, truncated
                );
                let _ = self.client.send_message(chat_id, &msg, None).await;
            }
            AgentEvent::ToolCallFinished {
                tool,
                result,
                success,
                ..
            } => {
                let icon = if *success { "✅" } else { "❌" };
                let res_str = serde_json::to_string_pretty(result).unwrap_or_default();
                let truncated = if res_str.len() > 300 {
                    format!("{}...", &res_str[..300])
                } else {
                    res_str
                };
                let msg = format!(
                    "{} *Tool `{}` finished:*\n```\n{}\n```",
                    icon, tool, truncated
                );
                let _ = self.client.send_message(chat_id, &msg, None).await;
            }
            AgentEvent::SubagentSpawned {
                subagent_id,
                role,
                brief,
                ..
            } => {
                let msg = format!(
                    "🐾 *Subagent Launched:* `{} ({})`\n_{}_",
                    subagent_id, role, brief
                );
                let _ = self.client.send_message(chat_id, &msg, None).await;
            }
            AgentEvent::SubagentStatusChanged {
                subagent_id,
                status,
                summary,
            } => {
                let sum_str = summary.as_deref().unwrap_or("");
                let msg = format!("🐾 *Subagent {:?}:* `{}` {}", status, subagent_id, sum_str);
                let _ = self.client.send_message(chat_id, &msg, None).await;
            }
            AgentEvent::Error { code, message } => {
                let msg = format!("⚠️ *Error ({}):* {}", code, message);
                let _ = self.client.send_message(chat_id, &msg, None).await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn request_approval(
        &self,
        call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> Result<bool> {
        let chat_id = self.get_active_chat_id();
        if chat_id == 0 {
            // Fail-closed if no chat is active
            return Ok(false);
        }

        let mut text = format!(
            "🛡️ *Approval Required*\n\n*Tool:* `{}`\n*Action:* {}\n",
            tool, description
        );

        if let Some(prev) = preview {
            let trunc_prev = if prev.len() > 800 {
                format!("{}...", &prev[..800])
            } else {
                prev.to_string()
            };
            text.push_str(&format!("\n*Preview:*\n```\n{}\n```\n", trunc_prev));
        }

        let keyboard = InlineKeyboardMarkup::approval_buttons(call_id);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_approvals.lock().await;
            pending.insert(call_id.to_string(), tx);
        }

        if let Err(e) = self
            .client
            .send_message(chat_id, &text, Some(&keyboard))
            .await
        {
            let mut pending = self.pending_approvals.lock().await;
            pending.remove(call_id);
            return Err(e);
        }

        // Wait with a 180s fail-closed timeout
        match tokio::time::timeout(Duration::from_secs(180), rx).await {
            Ok(Ok(decision)) => Ok(decision),
            _ => {
                // Timeout or channel closed: clean up and default to DENY
                let mut pending = self.pending_approvals.lock().await;
                pending.remove(call_id);
                let _ = self
                    .client
                    .send_message(
                        chat_id,
                        &format!(
                            "⏱️ Approval for `{}` timed out after 180s (defaulted to DENY).",
                            tool
                        ),
                        None,
                    )
                    .await;
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_user_allowed() {
        let channel = TelegramChannel::new(
            "fake_token",
            vec!["123456".to_string(), "@alice".to_string()],
            None,
        );
        assert!(channel.is_user_allowed(123456, None));
        assert!(channel.is_user_allowed(999999, Some("alice")));
        assert!(channel.is_user_allowed(999999, Some("@alice")));
        assert!(!channel.is_user_allowed(999999, Some("bob")));
        assert!(!channel.is_user_allowed(888888, None));
    }

    #[tokio::test]
    async fn test_telegram_resolve_approval() {
        let channel = TelegramChannel::new("fake_token", vec![], None);
        channel.set_active_chat_id(1001);
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut pending = channel.pending_approvals.lock().await;
            pending.insert("call-123".to_string(), tx);
        }

        assert!(channel.resolve_approval("call-123", true).await);
        let res = rx.await.unwrap();
        assert!(res);

        // Resolving an unknown call should return false
        assert!(!channel.resolve_approval("call-999", true).await);
    }
}
