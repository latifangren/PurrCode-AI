// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{InlineKeyboardMarkup, TgMessage, TgResponse, TgUpdate, TgUser};
use purrcode_api::error::{PurrError, Result};
use reqwest::Client;

pub struct TgClient {
    client: Client,
    token: String,
    base_url: String,
}

impl TgClient {
    pub fn new(token: impl Into<String>, base_url: Option<String>) -> Self {
        let base = base_url.unwrap_or_else(|| "https://api.telegram.org".to_string());
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(45))
                .build()
                .unwrap_or_default(),
            token: token.into(),
            base_url: base,
        }
    }

    fn endpoint(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url, self.token, method)
    }

    pub async fn get_me(&self) -> Result<TgUser> {
        let url = self.endpoint("getMe");
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("getMe failed: {}", e),
            })?;

        let parsed: TgResponse<TgUser> = resp.json().await.map_err(|e| PurrError::Channel {
            channel: "telegram".to_string(),
            message: format!("Failed to parse getMe response: {}", e),
        })?;

        if !parsed.ok {
            return Err(PurrError::Channel {
                channel: "telegram".to_string(),
                message: parsed
                    .description
                    .unwrap_or_else(|| "getMe returned ok=false".to_string()),
            });
        }

        parsed.result.ok_or_else(|| PurrError::Channel {
            channel: "telegram".to_string(),
            message: "Missing result in getMe response".to_string(),
        })
    }

    pub async fn get_updates(&self, offset: Option<i64>, timeout: u32) -> Result<Vec<TgUpdate>> {
        let url = self.endpoint("getUpdates");
        let mut query = vec![("timeout", timeout.to_string())];
        if let Some(off) = offset {
            query.push(("offset", off.to_string()));
        }

        let resp = self
            .client
            .get(&url)
            .query(&query)
            .send()
            .await
            .map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("getUpdates request failed: {}", e),
            })?;

        let parsed: TgResponse<Vec<TgUpdate>> =
            resp.json().await.map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("Failed to parse getUpdates response: {}", e),
            })?;

        if !parsed.ok {
            return Err(PurrError::Channel {
                channel: "telegram".to_string(),
                message: parsed
                    .description
                    .unwrap_or_else(|| "getUpdates returned ok=false".to_string()),
            });
        }

        Ok(parsed.result.unwrap_or_default())
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<&InlineKeyboardMarkup>,
    ) -> Result<TgMessage> {
        let chunks = split_message(text, 4000);
        let mut last_msg = None;

        for (idx, chunk) in chunks.iter().enumerate() {
            let is_last = idx == chunks.len() - 1;
            let markup = if is_last { reply_markup } else { None };

            let url = self.endpoint("sendMessage");
            let mut payload = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
            });

            if let Some(m) = markup {
                payload["reply_markup"] = serde_json::to_value(m).unwrap_or_default();
            }

            let resp = self
                .client
                .post(&url)
                .json(&payload)
                .send()
                .await
                .map_err(|e| PurrError::Channel {
                    channel: "telegram".to_string(),
                    message: format!("sendMessage request failed: {}", e),
                })?;

            let parsed: TgResponse<TgMessage> =
                resp.json().await.map_err(|e| PurrError::Channel {
                    channel: "telegram".to_string(),
                    message: format!("Failed to parse sendMessage response: {}", e),
                })?;

            if !parsed.ok {
                return Err(PurrError::Channel {
                    channel: "telegram".to_string(),
                    message: parsed
                        .description
                        .unwrap_or_else(|| "sendMessage returned ok=false".to_string()),
                });
            }

            last_msg = parsed.result;
        }

        last_msg.ok_or_else(|| PurrError::Channel {
            channel: "telegram".to_string(),
            message: "No message sent".to_string(),
        })
    }

    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        reply_markup: Option<&InlineKeyboardMarkup>,
    ) -> Result<()> {
        let url = self.endpoint("editMessageText");
        let truncated = if text.len() > 4000 {
            &text[..4000]
        } else {
            text
        };

        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": truncated,
        });

        if let Some(m) = reply_markup {
            payload["reply_markup"] = serde_json::to_value(m).unwrap_or_default();
        }

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("editMessageText request failed: {}", e),
            })?;

        let parsed: TgResponse<serde_json::Value> =
            resp.json().await.map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("Failed to parse editMessageText response: {}", e),
            })?;

        if !parsed.ok {
            // Note: Telegram returns an error if message content is unmodified; ignore that specific error
            if let Some(desc) = &parsed.description {
                if desc.contains("message is not modified") {
                    return Ok(());
                }
            }
            return Err(PurrError::Channel {
                channel: "telegram".to_string(),
                message: parsed
                    .description
                    .unwrap_or_else(|| "editMessageText returned ok=false".to_string()),
            });
        }

        Ok(())
    }

    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let url = self.endpoint("answerCallbackQuery");
        let mut payload = serde_json::json!({
            "callback_query_id": callback_query_id,
        });
        if let Some(t) = text {
            payload["text"] = serde_json::Value::String(t.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!("answerCallbackQuery request failed: {}", e),
            })?;

        let parsed: TgResponse<bool> = resp.json().await.map_err(|e| PurrError::Channel {
            channel: "telegram".to_string(),
            message: format!("Failed to parse answerCallbackQuery response: {}", e),
        })?;

        if !parsed.ok {
            return Err(PurrError::Channel {
                channel: "telegram".to_string(),
                message: parsed
                    .description
                    .unwrap_or_else(|| "answerCallbackQuery returned ok=false".to_string()),
            });
        }

        Ok(())
    }
}

pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::with_capacity(max_len);

    for line in text.lines() {
        if current.len() + line.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current);
                current = String::with_capacity(max_len);
            }
            if line.len() > max_len {
                // If single line exceeds limit, slice by characters
                let chars: Vec<char> = line.chars().collect();
                for sub in chars.chunks(max_len) {
                    chunks.push(sub.iter().collect());
                }
                continue;
            }
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message() {
        let small = "Hello world!";
        assert_eq!(split_message(small, 20), vec!["Hello world!"]);

        let multi = "Line 1\nLine 2\nLine 3\nLine 4";
        let chunks = split_message(multi, 15);
        assert!(chunks.len() >= 2);
        for chunk in chunks {
            assert!(chunk.len() <= 15);
        }
    }
}
