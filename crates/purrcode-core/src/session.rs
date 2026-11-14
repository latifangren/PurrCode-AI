// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{ChatMessage, MessageRole};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub version: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub model: Option<String>,
    pub preset: String,
    pub model_preset: Option<String>,
    pub turn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurnEntry {
    pub version: String,
    pub session_id: String,
    pub timestamp: String,
    pub message: ChatMessage,
}

pub struct SessionStore {
    root_dir: PathBuf,
}

impl SessionStore {
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root_dir.join("sessions")
    }

    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let jsonl_path = self.session_jsonl_path(session_id);
        if jsonl_path.exists() {
            std::fs::remove_file(&jsonl_path).map_err(PurrError::Io)?;
        }
        let dir_path = self.session_dir(session_id);
        if dir_path.exists() && dir_path.is_dir() {
            std::fs::remove_dir_all(&dir_path).map_err(PurrError::Io)?;
        }
        Ok(())
    }

    pub fn session_jsonl_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{}.jsonl", session_id))
    }

    /// Create an empty session file so a fresh session shows up in
    /// `list_sessions` before its first message is appended (v0.4 B1:
    /// the WebUI "new session" button must list the session at once).
    /// Idempotent: an existing file is left untouched.
    pub fn create_session(&self, session_id: &str) -> Result<()> {
        let dir = self.sessions_dir();
        std::fs::create_dir_all(&dir).map_err(PurrError::Io)?;
        let path = self.session_jsonl_path(session_id);
        if !path.exists() {
            std::fs::write(&path, "").map_err(PurrError::Io)?;
        }
        Ok(())
    }

    /// True when the session has a JSONL file on disk.
    pub fn session_exists(&self, session_id: &str) -> bool {
        self.session_jsonl_path(session_id).exists()
    }

    pub fn append_message(&self, session_id: &str, message: &ChatMessage) -> Result<()> {
        let dir = self.sessions_dir();
        std::fs::create_dir_all(&dir).map_err(PurrError::Io)?;
        let path = self.session_jsonl_path(session_id);

        let entry = SessionTurnEntry {
            version: "session/v1".to_string(),
            session_id: session_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            message: message.clone(),
        };

        let line = serde_json::to_string(&entry)? + "\n";
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(PurrError::Io)?;
        file.write_all(line.as_bytes()).map_err(PurrError::Io)?;
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let path = self.session_jsonl_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path).map_err(PurrError::Io)?;
        let mut messages = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SessionTurnEntry>(trimmed) {
                messages.push(entry.message);
            }
        }
        Ok(messages)
    }

    pub fn fork_session(&self, source_id: &str, new_id: &str) -> Result<()> {
        let src = self.session_jsonl_path(source_id);
        if !src.exists() {
            return Err(PurrError::Session(format!(
                "Source session '{}' does not exist",
                source_id
            )));
        }
        let dst = self.session_jsonl_path(new_id);
        std::fs::copy(src, dst).map_err(PurrError::Io)?;
        Ok(())
    }

    pub fn save_preset_snapshots(
        &self,
        session_id: &str,
        preset_json: &serde_json::Value,
        model_preset_json: &serde_json::Value,
    ) -> Result<()> {
        let sdir = self.session_dir(session_id);
        std::fs::create_dir_all(&sdir).map_err(PurrError::Io)?;
        std::fs::write(
            sdir.join("preset.json"),
            serde_json::to_string_pretty(preset_json)?.as_bytes(),
        )
        .map_err(PurrError::Io)?;
        std::fs::write(
            sdir.join("model-preset.json"),
            serde_json::to_string_pretty(model_preset_json)?.as_bytes(),
        )
        .map_err(PurrError::Io)?;
        Ok(())
    }

    /// Non-destructive compaction: preserves raw JSONL on disk while returning a compacted
    /// in-memory message vector keeping system prompt + summary + most recent `keep_last` messages.
    pub fn compact_messages(
        messages: &[ChatMessage],
        focus_note: Option<&str>,
        keep_last: usize,
    ) -> Vec<ChatMessage> {
        let mut compacted = Vec::new();
        let mut start_idx = 0;

        if let Some(first) = messages.first() {
            if first.role == MessageRole::System {
                compacted.push(first.clone());
                start_idx = 1;
            }
        }

        if messages.len().saturating_sub(start_idx) <= keep_last {
            return messages.to_vec();
        }

        let tail_start = messages.len().saturating_sub(keep_last).max(start_idx);
        let summarized_slice = &messages[start_idx..tail_start];
        let mut summary_lines = Vec::new();
        if let Some(focus) = focus_note {
            summary_lines.push(format!("[Compaction Focus: {}]", focus));
        }
        for msg in summarized_slice {
            let role_label = match msg.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            let snippet: String = msg.content.chars().take(160).collect();
            summary_lines.push(format!("- [{}]: {}", role_label, snippet));
        }

        compacted.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "### Compacted Session Summary ({} earlier turns preserved in session JSONL)\n{}",
                summarized_slice.len(),
                summary_lines.join("\n")
            ),
            tool_calls: None,
            tool_call_id: None,
        });

        compacted.extend_from_slice(&messages[tail_start..]);
        compacted
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.sessions_dir()) {
            for entry in entries.flatten() {
                let path: PathBuf = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        ids.push(stem.to_string());
                    }
                }
            }
        }
        ids.sort();
        ids
    }

    pub fn export_session(&self, session_id: &str, format: &str) -> Result<String> {
        let msgs = self.load_messages(session_id)?;
        if format.eq_ignore_ascii_case("html") {
            let mut html = String::from(
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>PurrCode Session</title></head><body>\n",
            );
            html.push_str(&format!("<h1>Session: {}</h1>\n", session_id));
            for m in msgs {
                html.push_str(&format!(
                    "<div class=\"msg\"><strong>{:?}:</strong><pre>{}</pre></div>\n",
                    m.role,
                    m.content.replace('<', "&lt;").replace('>', "&gt;")
                ));
            }
            html.push_str("</body></html>");
            Ok(html)
        } else {
            let path: &Path = &self.session_jsonl_path(session_id);
            std::fs::read_to_string(path).map_err(PurrError::Io)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_append_fork_and_compact() {
        let temp_dir = std::env::temp_dir().join(format!("purr_sess_{}", uuid::Uuid::new_v4()));
        let store = SessionStore::new(&temp_dir);

        let m1 = ChatMessage {
            role: MessageRole::User,
            content: "Hello 1".to_string(),
            tool_calls: None,
            tool_call_id: None,
        };
        let m2 = ChatMessage {
            role: MessageRole::Assistant,
            content: "Reply 1".to_string(),
            tool_calls: None,
            tool_call_id: None,
        };
        store.append_message("purr_test1", &m1).unwrap();
        store.append_message("purr_test1", &m2).unwrap();

        let loaded = store.load_messages("purr_test1").unwrap();
        assert_eq!(loaded.len(), 2);

        store.fork_session("purr_test1", "purr_fork1").unwrap();
        let forked = store.load_messages("purr_fork1").unwrap();
        assert_eq!(forked.len(), 2);

        let compacted = SessionStore::compact_messages(&loaded, Some("keep auth"), 1);
        assert_eq!(compacted.len(), 2);
        assert!(compacted[0].content.contains("Compacted Session Summary"));

        assert!(store.session_exists("purr_test1"));
        store.delete_session("purr_test1").unwrap();
        assert!(!store.session_exists("purr_test1"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
