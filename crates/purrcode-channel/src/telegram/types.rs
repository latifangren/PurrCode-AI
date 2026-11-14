// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal Telegram Bot API types (v0.2, ADR-013).
//!
//! Only the surface the gateway needs: updates (`message` /
//! `callback_query`), sending/editing messages, inline keyboards, and
//! `getMe`. Everything is plain serde — no Telegram SDK.

use serde::{Deserialize, Serialize};

/// Envelope of every Bot API response.
#[derive(Debug, Clone, Deserialize)]
pub struct BotApiResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub error_code: Option<i32>,
    pub description: Option<String>,
    pub parameters: Option<ResponseParameters>,
}

/// Extra error parameters (e.g. `retry_after` on 429).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResponseParameters {
    #[serde(default)]
    pub retry_after: Option<u64>,
    #[serde(default)]
    pub migrate_to_chat_id: Option<i64>,
}

/// One update from `getUpdates`.
#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

/// A message (incoming or sent).
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub entities: Option<Vec<MessageEntity>>,
    #[serde(default)]
    pub caption_entities: Option<Vec<MessageEntity>>,
}

/// A chat (DM or group). The session key is the chat id (ADR-013).
#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

/// A Telegram user (or bot).
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
}

/// One entity inside message text (mention, bot_command, ...).
#[derive(Debug, Clone, Deserialize)]
pub struct MessageEntity {
    #[serde(rename = "type")]
    pub kind: String,
    pub offset: usize,
    pub length: usize,
    #[serde(default)]
    pub user: Option<User>,
}

/// A tap on an inline keyboard button.
#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

/// The bot itself (`getMe`).
#[derive(Debug, Clone, Deserialize)]
pub struct BotInfo {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    pub username: String,
    #[serde(default)]
    pub first_name: Option<String>,
}

/// Inline keyboard markup for `sendMessage` / `editMessageText`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

/// One inline button. `callback_data` must stay under 64 bytes (Bot API
/// limit) — the gateway only ever sends short tokens like `ap:<8 hex>`.
#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

impl InlineKeyboardMarkup {
    pub fn single_row(buttons: Vec<InlineKeyboardButton>) -> Self {
        Self {
            inline_keyboard: vec![buttons],
        }
    }
}

/// Slice a string by UTF-16 code-unit offsets (Telegram entity offsets).
/// Returns `None` when the range is out of bounds or splits a surrogate
/// pair. Used by `Message::mentions_bot`.
fn utf16_slice(text: &str, offset: usize, length: usize) -> Option<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = offset.checked_add(length)?;
    if end > units.len() {
        return None;
    }
    String::from_utf16(&units[offset..end]).ok()
}

impl Message {
    /// The text the user sent (message text, falling back to caption).
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref().or(self.caption.as_deref())
    }

    /// Entities attached to the text (or to the caption).
    fn entities(&self) -> &[MessageEntity] {
        if self.text.is_some() {
            self.entities.as_deref().unwrap_or(&[])
        } else {
            self.caption_entities.as_deref().unwrap_or(&[])
        }
    }

    /// True when the message mentions the bot by @username (entity
    /// `mention`), mentions the bot user directly (`text_mention`), or
    /// is a group command addressed to the bot (`bot_command` entity
    /// `/cmd@username`). Group gating (ADR-013): a group message is
    /// only processed when it mentions the bot — and a command like
    /// `/new@PurrCodeBot` IS a mention, or group commands would be
    /// silently ignored (the gate runs before the command router).
    ///
    /// NOTE: Telegram entity offsets are UTF-16 code units, NOT bytes —
    /// slicing the Rust string by `offset` directly would be wrong (and
    /// could panic) as soon as an emoji precedes the mention. The entity
    /// range is converted through UTF-16 first.
    pub fn mentions_bot(&self, bot_username: &str, bot_id: i64) -> bool {
        let Some(text) = self.text() else {
            return false;
        };
        let want = format!("@{bot_username}").to_lowercase();
        for ent in self.entities() {
            if ent.kind == "mention" {
                if let Some(mentioned) = utf16_slice(text, ent.offset, ent.length) {
                    if mentioned.to_lowercase() == want {
                        return true;
                    }
                }
            } else if ent.kind == "text_mention" {
                if let Some(user) = &ent.user {
                    if user.id == bot_id {
                        return true;
                    }
                }
            } else if ent.kind == "bot_command" {
                // "/cmd@username": the entity text carries the @suffix.
                if let Some(cmd) = utf16_slice(text, ent.offset, ent.length) {
                    if let Some((_, suffix)) = cmd.split_once('@') {
                        if format!("@{suffix}").to_lowercase() == want {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// True when the text is a bot command (`/new`, `/resume`, ...).
    pub fn is_command(&self) -> bool {
        self.text()
            .map(|t| t.trim_start().starts_with('/'))
            .unwrap_or(false)
    }

    /// The command word without arguments, e.g. `/new` from `/new foo`.
    /// Returns `None` when the text is not a command.
    pub fn command(&self) -> Option<String> {
        let text = self.text()?.trim_start();
        if !text.starts_with('/') {
            return None;
        }
        let word = text[1..].split_whitespace().next().unwrap_or("");
        // Telegram appends the bot username in groups: `/new@botname`.
        let word = word.split('@').next().unwrap_or(word);
        if word.is_empty() {
            None
        } else {
            Some(format!("/{word}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_with(text: &str, entities: Vec<MessageEntity>) -> Message {
        Message {
            message_id: 1,
            chat: Chat {
                id: 100,
                kind: "private".to_string(),
                title: None,
                username: None,
            },
            from: Some(User {
                id: 7,
                is_bot: false,
                username: Some("alice".to_string()),
                first_name: Some("Alice".to_string()),
            }),
            text: Some(text.to_string()),
            caption: None,
            entities: Some(entities),
            caption_entities: None,
        }
    }

    #[test]
    fn test_mentions_bot_via_mention_entity() {
        let msg = message_with(
            "hello @PurrBot how are you",
            vec![MessageEntity {
                kind: "mention".to_string(),
                offset: 6,
                length: 8,
                user: None,
            }],
        );
        assert!(msg.mentions_bot("PurrBot", 999));
        assert!(!msg.mentions_bot("OtherBot", 999));
    }

    #[test]
    fn test_mentions_bot_with_emoji_before_mention() {
        // "🐾 hi @PurrBot": the paw emoji is ONE Rust char but TWO
        // UTF-16 code units (surrogate pair), so the mention entity
        // starts at UTF-16 offset 6 (2 + 1 + 1 + 1 + 1), NOT byte offset
        // 6 (the emoji alone is 4 bytes) and NOT char offset 5. A
        // byte-offset slice would be wrong / could panic here.
        let msg = message_with(
            "🐾 hi @PurrBot",
            vec![MessageEntity {
                kind: "mention".to_string(),
                offset: 6,
                length: 8,
                user: None,
            }],
        );
        assert!(msg.mentions_bot("PurrBot", 999));
    }

    #[test]
    fn test_mentions_bot_via_text_mention() {
        let msg = message_with(
            "hello",
            vec![MessageEntity {
                kind: "text_mention".to_string(),
                offset: 0,
                length: 5,
                user: Some(User {
                    id: 999,
                    is_bot: true,
                    username: Some("PurrBot".to_string()),
                    first_name: Some("Purr".to_string()),
                }),
            }],
        );
        assert!(msg.mentions_bot("PurrBot", 999));
        assert!(!msg.mentions_bot("PurrBot", 123));
    }

    #[test]
    fn test_mentions_bot_via_bot_command_entity() {
        // "/new@PurrBot": a group command addressed to the bot counts
        // as a mention — otherwise the group gate (which runs before
        // the command router) would swallow it silently.
        let msg = message_with(
            "/new@PurrBot",
            vec![MessageEntity {
                kind: "bot_command".to_string(),
                offset: 0,
                length: 12,
                user: None,
            }],
        );
        assert!(msg.mentions_bot("PurrBot", 999));
        assert!(!msg.mentions_bot("OtherBot", 999));
        // A bare command with no @suffix is NOT a mention (in a group
        // it is addressed to whichever bot the user picked — the gate
        // cannot attribute it to us).
        let msg = message_with(
            "/new",
            vec![MessageEntity {
                kind: "bot_command".to_string(),
                offset: 0,
                length: 4,
                user: None,
            }],
        );
        assert!(!msg.mentions_bot("PurrBot", 999));
    }

    #[test]
    fn test_command_parsing() {
        let msg = message_with("/new", vec![]);
        assert!(msg.is_command());
        assert_eq!(msg.command().as_deref(), Some("/new"));

        let msg = message_with("/resume@PurrBot extra args", vec![]);
        assert_eq!(msg.command().as_deref(), Some("/resume"));

        let msg = message_with("just text", vec![]);
        assert!(!msg.is_command());
        assert_eq!(msg.command(), None);
    }

    #[test]
    fn test_bot_api_response_parses() {
        let raw = r#"{"ok":true,"result":[{"update_id":42,"message":{"message_id":1,"chat":{"id":100,"type":"private"},"text":"hi"}}]}"#;
        let resp: BotApiResponse<Vec<Update>> = serde_json::from_str(raw).expect("updates parse");
        assert!(resp.ok);
        let updates = resp.result.expect("result present");
        assert_eq!(updates[0].update_id, 42);
        assert_eq!(
            updates[0].message.as_ref().unwrap().text.as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn test_error_response_parses_retry_after() {
        let raw = r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 3","parameters":{"retry_after":3}}"#;
        let resp: BotApiResponse<Vec<Update>> = serde_json::from_str(raw).expect("error parses");
        assert!(!resp.ok);
        assert_eq!(resp.error_code, Some(429));
        assert_eq!(
            resp.parameters.as_ref().and_then(|p| p.retry_after),
            Some(3)
        );
    }
}
