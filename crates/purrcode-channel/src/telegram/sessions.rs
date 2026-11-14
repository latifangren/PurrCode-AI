// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-chat session index (v0.2, ADR-013).
//!
//! Maps Telegram `chat_id` → the PurrCode session that chat is talking
//! in. Stored as one small JSON file under `~/.purrcode/telegram/` and
//! written atomically (temp file + rename) so a crash mid-write never
//! leaves a half-written index. The session transcripts themselves live
//! in `SessionStore` (`~/.purrcode/sessions/<id>/`); this file is only
//! the index. `/new` archives the current session into history instead
//! of deleting it — `/resume` can bring it back.

use purrcode_api::error::{PurrError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Sessions a single chat has used. `current` is the session new
/// messages go to; `history` holds archived ones, oldest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatSessions {
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub history: Vec<String>,
    /// Unix seconds of the last accepted message/command from this
    /// chat. Drives heartbeat delivery routing ("last active chat").
    /// Absent for chats last seen before this field existed (v0.3 F22).
    #[serde(default)]
    pub last_active_at: Option<i64>,
    /// Telegram chat type as reported by the Bot API (`private`,
    /// `group`, `supergroup`, `channel`). Recorded at touch time
    /// (v0.3 F22 fix): only private chats are heartbeat delivery
    /// targets — a group mention must never make a group the "home
    /// chat", or a heartbeat summary (which may quote the user's
    /// private checks) would leak to the group's members. Absent for
    /// chats last seen before this field existed; those chats are NOT
    /// eligible for delivery routing (fail-closed — the
    /// `allow_from[0]` fallback covers the normal single-user case).
    #[serde(default)]
    pub kind: Option<String>,
}

/// The `chat_id → ChatSessions` index.
#[derive(Debug)]
pub struct ChatSessionIndex {
    path: PathBuf,
    map: HashMap<i64, ChatSessions>,
}

impl ChatSessionIndex {
    /// File name inside the telegram state directory.
    pub const FILE_NAME: &'static str = "chats.json";

    /// Load the index from `<dir>/chats.json` (missing or corrupt file =
    /// an empty index; a corrupt file is never silently overwritten until
    /// the next `attach`/`new_session` save).
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(Self::FILE_NAME);
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self { path, map }
    }

    /// The session id a chat is currently talking in, if any.
    pub fn current_session(&self, chat_id: i64) -> Option<&str> {
        self.map.get(&chat_id).and_then(|s| s.current.as_deref())
    }

    /// All sessions a chat has used, most recent first (current first,
    /// then history newest-first). Used by `/resume`.
    pub fn sessions_for_chat(&self, chat_id: i64) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(entry) = self.map.get(&chat_id) {
            if let Some(current) = &entry.current {
                out.push(current.clone());
            }
            for old in entry.history.iter().rev() {
                out.push(old.clone());
            }
        }
        out
    }

    /// Point a chat at a session (creating the entry when needed) and
    /// persist the index atomically.
    pub fn attach(&mut self, chat_id: i64, session_id: &str) -> Result<()> {
        let entry = self.map.entry(chat_id).or_default();
        if entry.current.as_deref() == Some(session_id) {
            return Ok(());
        }
        if let Some(old) = entry.current.replace(session_id.to_string()) {
            entry.history.push(old);
        }
        self.save()
    }

    /// Start a fresh session for a chat: the current session (if any) is
    /// archived into history, never deleted. Returns the new session id.
    pub fn new_session(&mut self, chat_id: i64, new_session_id: &str) -> Result<String> {
        let entry = self.map.entry(chat_id).or_default();
        if let Some(old) = entry.current.take() {
            entry.history.push(old);
        }
        entry.current = Some(new_session_id.to_string());
        self.save()?;
        Ok(new_session_id.to_string())
    }

    /// Record activity from a chat (an accepted message or command)
    /// and persist the index atomically. One small write per accepted
    /// message — human-paced, and the file is tiny. `kind` is the
    /// Telegram chat type (`private`, `group`, ...); a `None` touch
    /// refreshes the timestamp without overwriting a recorded kind.
    pub fn touch(&mut self, chat_id: i64, kind: Option<&str>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let entry = self.map.entry(chat_id).or_default();
        entry.last_active_at = Some(now);
        if let Some(kind) = kind {
            entry.kind = Some(kind.to_string());
        }
        self.save()
    }

    /// The most recently active PRIVATE chat, if any. Groups,
    /// supergroups and channels are never delivery targets, and chats
    /// whose kind was never recorded are not eligible either
    /// (fail-closed). The caller falls back to the first `allow_from`
    /// entry then.
    pub fn last_active_private_chat(&self) -> Option<i64> {
        self.map
            .iter()
            .filter(|(_, s)| s.kind.as_deref() == Some("private"))
            .filter_map(|(id, s)| s.last_active_at.map(|t| (*id, t)))
            .max_by_key(|(_, t)| *t)
            .map(|(id, _)| id)
    }

    /// End a chat's current session (`/stop`): the current session is
    /// archived into history and the chat has no current session until
    /// the next message starts a fresh one.
    pub fn end_session(&mut self, chat_id: i64) -> Result<()> {
        if let Some(entry) = self.map.get_mut(&chat_id) {
            if let Some(old) = entry.current.take() {
                entry.history.push(old);
            }
        }
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PurrError::Channel {
                channel: "telegram".to_string(),
                message: format!(
                    "failed to create telegram state dir {}: {e}",
                    parent.display()
                ),
            })?;
        }
        let raw = serde_json::to_string_pretty(&self.map).map_err(|e| PurrError::Channel {
            channel: "telegram".to_string(),
            message: format!("failed to serialize telegram chat index: {e}"),
        })?;
        // Atomic write: temp file in the same directory, then rename.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, raw).map_err(|e| PurrError::Channel {
            channel: "telegram".to_string(),
            message: format!("failed to write telegram chat index: {e}"),
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|e| PurrError::Channel {
            channel: "telegram".to_string(),
            message: format!("failed to finalize telegram chat index: {e}"),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("purr_tg_idx_{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn test_roundtrip_and_new_session_archives() {
        let dir = tmp_dir();
        let mut idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.current_session(100), None);

        idx.attach(100, "purr_aaa111").unwrap();
        assert_eq!(idx.current_session(100), Some("purr_aaa111"));

        // Reload from disk: the index survives.
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.current_session(100), Some("purr_aaa111"));

        // /new archives the old session instead of deleting it.
        let mut idx = idx;
        idx.new_session(100, "purr_bbb222").unwrap();
        assert_eq!(idx.current_session(100), Some("purr_bbb222"));
        assert_eq!(
            idx.sessions_for_chat(100),
            vec!["purr_bbb222".to_string(), "purr_aaa111".to_string()]
        );

        // Reload again: history survives too.
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(
            idx.sessions_for_chat(100),
            vec!["purr_bbb222".to_string(), "purr_aaa111".to_string()]
        );

        // No temp file left behind after atomic saves.
        assert!(!dir.join("chats.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_touch_and_last_active_private_chat() {
        let dir = tmp_dir();
        let mut idx = ChatSessionIndex::load(&dir);
        // No timestamp anywhere yet: no last-active chat.
        assert_eq!(idx.last_active_private_chat(), None);
        idx.attach(100, "purr_aaa111").unwrap();
        assert_eq!(idx.last_active_private_chat(), None);

        idx.touch(100, Some("private")).unwrap();
        assert_eq!(idx.last_active_private_chat(), Some(100));
        // A later touch in another PRIVATE chat wins.
        std::thread::sleep(std::time::Duration::from_secs(1));
        idx.touch(200, Some("private")).unwrap();
        assert_eq!(idx.last_active_private_chat(), Some(200));

        // A NEWER touch in a GROUP does not win: groups are never
        // delivery targets (v0.3 F22 fix, re-checker F1).
        std::thread::sleep(std::time::Duration::from_secs(1));
        idx.touch(-100, Some("group")).unwrap();
        assert_eq!(idx.last_active_private_chat(), Some(200));
        // A touch without a kind refreshes the timestamp but never
        // promotes the chat to a delivery target.
        std::thread::sleep(std::time::Duration::from_secs(1));
        idx.touch(-100, None).unwrap();
        assert_eq!(idx.last_active_private_chat(), Some(200));

        // The timestamp + kind survive a reload (and a file written
        // before these fields existed loads as None, never as an
        // error — and a kind-less entry is not eligible).
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.last_active_private_chat(), Some(200));
        std::fs::write(
            dir.join("chats.json"),
            r#"{"300": {"current": "purr_ccc333", "history": []}}"#,
        )
        .unwrap();
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.last_active_private_chat(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupt_index_loads_empty() {
        let dir = tmp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("chats.json"), "{not json").unwrap();
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.current_session(100), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
