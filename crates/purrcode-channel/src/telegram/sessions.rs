// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-chat session index (v0.2, ADR-013; SQLite since ADR-015).
//!
//! Maps Telegram `chat_id` → the PurrCode session that chat is talking
//! in. One row per chat in the `chats` table of
//! `<purrcode_dir>/purrcode.db` (formerly one small JSON file,
//! `~/.purrcode/telegram/chats.json`). The session transcripts
//! themselves live in `SessionStore`; this table is only the index.
//! `/new` archives the current session into history instead of
//! deleting it — `/resume` can bring it back.
//!
//! The index opens the database on every operation (like the old file
//! code opened the file per load/save), so `load()` never fails —
//! open errors surface per operation. Reads degrade to empty on a
//! broken store (the old corrupt-file behaviour); writes fail closed.

use purrcode_api::error::{PurrError, Result};
use purrcode_store::rusqlite::{params, OptionalExtension};
use purrcode_store::{sql, Store};
use std::path::{Path, PathBuf};

/// Sessions a single chat has used. `current` is the session new
/// messages go to; `history` holds archived ones, oldest first.
#[derive(Debug, Clone, Default)]
pub struct ChatSessions {
    pub current: Option<String>,
    pub history: Vec<String>,
    /// Unix seconds of the last accepted message/command from this
    /// chat. Drives heartbeat delivery routing ("last active chat").
    /// `None` for chats never touched (or last seen before this field
    /// existed, v0.3 F22).
    pub last_active_at: Option<i64>,
    /// Telegram chat type as reported by the Bot API (`private`,
    /// `group`, `supergroup`, `channel`). Recorded at touch time
    /// (v0.3 F22 fix): only private chats are heartbeat delivery
    /// targets — a group mention must never make a group the "home
    /// chat", or a heartbeat summary (which may quote the user's
    /// private checks) would leak to the group's members. `None` for
    /// chats last seen before this field existed; those chats are NOT
    /// eligible for delivery routing (fail-closed — the
    /// `allow_from[0]` fallback covers the normal single-user case).
    pub kind: Option<String>,
}

/// The `chat_id → ChatSessions` index.
#[derive(Debug)]
pub struct ChatSessionIndex {
    dir: PathBuf,
}

/// One `chats` row: (current_session, history_json, kind, last_active_at).
type ChatRow = (Option<String>, String, Option<String>, Option<i64>);

impl ChatSessionIndex {
    /// Open the index over `<dir>/purrcode.db`. Infallible by design:
    /// the database is opened per operation, so a missing or corrupt
    /// store degrades reads to empty (as the old corrupt `chats.json`
    /// did) and fails writes with an error.
    pub fn load(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn open(&self) -> Result<Store> {
        Ok(Store::open(&self.dir)?)
    }

    fn read_row(&self, chat_id: i64) -> Result<Option<ChatSessions>> {
        let store = self.open()?;
        let guard = store.lock()?;
        let row: Option<ChatRow> = sql(guard
            .query_row(
                "SELECT current_session, history_json, kind, last_active_at
                 FROM chats WHERE chat_id = ?1",
                params![chat_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional())
        .map_err(|e| PurrError::Internal(format!("sqlite store: {e}")))?;
        let Some((current, history_json, kind, last_active_at)) = row else {
            return Ok(None);
        };
        let history: Vec<String> = serde_json::from_str(&history_json).map_err(|e| {
            PurrError::Session(format!(
                "corrupt chat index: invalid history for chat {chat_id}: {e}"
            ))
        })?;
        Ok(Some(ChatSessions {
            current,
            history,
            last_active_at,
            kind,
        }))
    }

    fn write_row(&self, chat_id: i64, entry: &ChatSessions) -> Result<()> {
        let history_json = serde_json::to_string(&entry.history).map_err(|e| {
            PurrError::Session(format!(
                "failed to serialize chat history for chat {chat_id}: {e}"
            ))
        })?;
        let store = self.open()?;
        let guard = store.lock()?;
        sql(guard.execute(
            "INSERT INTO chats (chat_id, kind, last_active_at, current_session, history_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(chat_id) DO UPDATE SET
               kind = excluded.kind,
               last_active_at = excluded.last_active_at,
               current_session = excluded.current_session,
               history_json = excluded.history_json",
            params![
                chat_id,
                entry.kind,
                entry.last_active_at,
                entry.current,
                history_json,
            ],
        ))?;
        Ok(())
    }

    /// The session id a chat is currently talking in, if any. A
    /// broken store reads as "no session" (the old corrupt-file
    /// behaviour) — the caller creates a fresh session then.
    pub fn current_session(&self, chat_id: i64) -> Option<String> {
        self.read_row(chat_id)
            .ok()
            .flatten()
            .and_then(|s| s.current)
    }

    /// All sessions a chat has used, most recent first (current first,
    /// then history newest-first). Used by `/resume`. A broken store
    /// reads as "no sessions".
    pub fn sessions_for_chat(&self, chat_id: i64) -> Vec<String> {
        let Ok(Some(entry)) = self.read_row(chat_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(current) = &entry.current {
            out.push(current.clone());
        }
        for old in entry.history.iter().rev() {
            out.push(old.clone());
        }
        out
    }

    /// Point a chat at a session (creating the entry when needed) and
    /// persist the index.
    pub fn attach(&mut self, chat_id: i64, session_id: &str) -> Result<()> {
        let mut entry = self.read_row(chat_id)?.unwrap_or_default();
        if entry.current.as_deref() == Some(session_id) {
            return Ok(());
        }
        if let Some(old) = entry.current.replace(session_id.to_string()) {
            entry.history.push(old);
        }
        self.write_row(chat_id, &entry)
    }

    /// Start a fresh session for a chat: the current session (if any) is
    /// archived into history, never deleted. Returns the new session id.
    pub fn new_session(&mut self, chat_id: i64, new_session_id: &str) -> Result<String> {
        let mut entry = self.read_row(chat_id)?.unwrap_or_default();
        if let Some(old) = entry.current.take() {
            entry.history.push(old);
        }
        entry.current = Some(new_session_id.to_string());
        self.write_row(chat_id, &entry)?;
        Ok(new_session_id.to_string())
    }

    /// Record activity from a chat (an accepted message or command)
    /// and persist the index. One small write per accepted message —
    /// human-paced. `kind` is the Telegram chat type (`private`,
    /// `group`, ...); a `None` touch refreshes the timestamp without
    /// overwriting a recorded kind.
    pub fn touch(&mut self, chat_id: i64, kind: Option<&str>) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut entry = self.read_row(chat_id)?.unwrap_or_default();
        entry.last_active_at = Some(now);
        if let Some(kind) = kind {
            entry.kind = Some(kind.to_string());
        }
        self.write_row(chat_id, &entry)
    }

    /// The most recently active PRIVATE chat, if any. Groups,
    /// supergroups and channels are never delivery targets, and chats
    /// whose kind was never recorded are not eligible either
    /// (fail-closed). The caller falls back to the first `allow_from`
    /// entry then. A broken store reads as "no chat".
    pub fn last_active_private_chat(&self) -> Option<i64> {
        let store = self.open().ok()?;
        let guard = store.lock().ok()?;
        sql(guard
            .query_row(
                "SELECT chat_id FROM chats
                 WHERE kind = 'private' AND last_active_at IS NOT NULL
                 ORDER BY last_active_at DESC, chat_id ASC LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional())
        .ok()
        .flatten()
    }

    /// End a chat's current session (`/stop`): the current session is
    /// archived into history and the chat has no current session until
    /// the next message starts a fresh one.
    pub fn end_session(&mut self, chat_id: i64) -> Result<()> {
        if let Some(mut entry) = self.read_row(chat_id)? {
            if let Some(old) = entry.current.take() {
                entry.history.push(old);
            }
            self.write_row(chat_id, &entry)?;
        }
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
        assert_eq!(idx.current_session(100), Some("purr_aaa111".to_string()));

        // Reload from the store: the index survives.
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.current_session(100), Some("purr_aaa111".to_string()));

        // /new archives the old session instead of deleting it.
        let mut idx = idx;
        idx.new_session(100, "purr_bbb222").unwrap();
        assert_eq!(idx.current_session(100), Some("purr_bbb222".to_string()));
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

        // The store is SQLite now (ADR-015): no `chats.json` anywhere.
        assert!(dir.join("purrcode.db").exists());
        assert!(!dir.join("chats.json").exists());
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

        // The timestamp + kind survive a reload (and a row written
        // before these fields existed — kind NULL — is not eligible).
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.last_active_private_chat(), Some(200));
        {
            let store = Store::open(&dir).unwrap();
            let guard = store.lock().unwrap();
            guard.execute("DELETE FROM chats", []).unwrap();
            guard
                .execute(
                    "INSERT INTO chats (chat_id, kind, last_active_at, current_session, history_json)
                     VALUES (300, NULL, 1780000000, 'purr_ccc333', '[]')",
                    [],
                )
                .unwrap();
        }
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.last_active_private_chat(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupt_row_reads_empty() {
        // A row with unparseable history (direct DB tampering, the SQL
        // equivalent of a corrupt `chats.json`): reads degrade to
        // empty, exactly as the old corrupt-file load did. Writes fail
        // closed instead of silently overwriting the corrupt row.
        let dir = tmp_dir();
        let mut idx = ChatSessionIndex::load(&dir);
        idx.attach(100, "purr_aaa111").unwrap();
        {
            let store = Store::open(&dir).unwrap();
            let guard = store.lock().unwrap();
            guard
                .execute(
                    "UPDATE chats SET history_json = '{not json' WHERE chat_id = 100",
                    [],
                )
                .unwrap();
        }
        let idx = ChatSessionIndex::load(&dir);
        assert_eq!(idx.current_session(100), None);
        assert!(idx.sessions_for_chat(100).is_empty());
        assert_eq!(idx.last_active_private_chat(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
