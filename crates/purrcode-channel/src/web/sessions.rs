// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-session registry for the WebUI (v0.4 B1).
//!
//! R3 had exactly one web session (`purr_web`) with one shared bus,
//! channel, and turn lock. B1 gives every web chat its own runtime
//! session — its own [`EventBus`], [`WebChannel`], turn lock, and
//! `session_start` flag — so two browser tabs can hold two different
//! chats without seeing each other's events or approvals.
//!
//! The registry owns the *runtime* entries only. The durable list
//! comes from the [`SessionStore`] on disk: a web session is any
//! `sessions/*.jsonl` file whose stem is a valid web session id, so
//! chats survive a daemon restart and are resumed from the same
//! store the TUI and cron use. New ids are minted as
//! `purr_web_<12 hex>` (same shape as the cron `cron_<12 hex>` ids).
//!
//! Security: a client-supplied session id is used to build a file
//! path (`sessions/<id>.jsonl`), so it is validated strictly —
//! `purr_web` or `purr_web_` + exactly 12 lowercase hex chars.
//! Anything else is rejected before it touches the store or the
//! filesystem, so `../` traversal and foreign session ids
//! (`purr_cron_*`, `purr_hb_*`, `purr_<hex>`) can never be reached
//! through the WebUI.

use crate::web::channel::WebChannel;
use purrcode_api::channel::Channel;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{ChatMessage, MessageRole};
use purrcode_core::session::SessionTurnEntry;
use purrcode_core::{EventBus, SessionStore};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

/// The legacy single-session id from R3. Kept as the default when a
/// client sends no `session_id`: old pages and old clients subscribe
/// and prompt without one, and an upgraded daemon still resumes the
/// chat stored under this id.
pub const DEFAULT_WEB_SESSION_ID: &str = "purr_web";

/// Prefix for web session ids minted by [`WebSessionRegistry`].
const WEB_SESSION_PREFIX: &str = "purr_web_";

/// Maximum live runtime entries. Eviction is safe by design:
/// sessions are durable on disk, a turn holds its session's `Arc`
/// (so eviction never kills a running turn), and a dropped entry is
/// re-created on demand by `ensure_session_if_exists`. Without a
/// cap, every `POST /api/sessions` would add one bus + channel +
/// forwarder task that lives for the daemon's lifetime.
const MAX_LIVE_SESSIONS: usize = 64;

/// Maximum characters of the first user message kept as the session
/// title in the list response.
const TITLE_CHARS: usize = 80;

/// A live web session: everything one browser chat needs that must
/// not leak into another chat.
pub struct WebSession {
    /// Session id (`purr_web` or `purr_web_<12 hex>`).
    pub id: String,
    /// This session's event bus: the session's loop publishes here,
    /// and only here.
    pub bus: Arc<EventBus>,
    /// This session's channel: the forwarder pumps `bus` into it,
    /// and this session's SSE subscribers read from it.
    pub channel: Arc<WebChannel>,
    /// One lock per session: turns in the same chat never
    /// interleave; turns in different chats run concurrently.
    pub turn_lock: AsyncMutex<()>,
    /// Whether the `session_start` plugin hook already fired for
    /// this session (once per session, not once per daemon).
    pub started: AsyncMutex<bool>,
}

/// One row of `GET /api/sessions`: enough for the sidebar to list
/// and resume a chat without reading its full history.
#[derive(Debug, Clone, Serialize)]
pub struct WebSessionInfo {
    pub id: String,
    pub message_count: usize,
    /// First user message, one line, truncated to [`TITLE_CHARS`]
    /// characters; empty when the session has no user message yet.
    pub title: String,
}

/// Strict shape check for a web session id. Only the legacy default
/// and freshly minted ids pass; everything else — including `../`
/// traversal and foreign `purr_*` sessions — is rejected.
pub fn is_valid_web_session_id(id: &str) -> bool {
    if id == DEFAULT_WEB_SESSION_ID {
        return true;
    }
    let Some(hex) = id.strip_prefix(WEB_SESSION_PREFIX) else {
        return false;
    };
    hex.len() == 12
        && hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Mint a fresh web session id (`purr_web_<12 hex>`), mirroring the
/// cron `new_job_id` shape (first 12 hex chars of a v4 UUID).
pub fn new_web_session_id() -> String {
    format!(
        "{}{}",
        WEB_SESSION_PREFIX,
        &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]
    )
}

/// Runtime registry for web sessions. The map holds live entries
/// only; the durable list is read from the store on every call so a
/// restart (or a session written by another process) is visible
/// without a restart of this daemon.
pub struct WebSessionRegistry {
    store: SessionStore,
    sessions: Mutex<HashMap<String, Arc<WebSession>>>,
    /// Insertion order of live entries, oldest first; drives the
    /// [`MAX_LIVE_SESSIONS`] eviction in `ensure_session`.
    order: Mutex<VecDeque<String>>,
}

impl WebSessionRegistry {
    pub fn new(purrcode_dir: impl Into<PathBuf>) -> Self {
        Self {
            store: SessionStore::new(purrcode_dir),
            sessions: Mutex::new(HashMap::new()),
            order: Mutex::new(VecDeque::new()),
        }
    }

    /// Get the live entry for `id`, creating it (bus + channel +
    /// forwarder) on first use. The id must be valid; an invalid id
    /// fails instead of creating anything.
    pub fn ensure_session(&self, id: &str) -> Result<Arc<WebSession>> {
        if !is_valid_web_session_id(id) {
            return Err(PurrError::Session(format!("invalid web session id '{id}'")));
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| PurrError::Internal("web session registry lock poisoned".to_string()))?;
        if let Some(session) = sessions.get(id) {
            return Ok(session.clone());
        }
        let session = Arc::new(WebSession {
            id: id.to_string(),
            bus: Arc::new(EventBus::new(256)),
            channel: Arc::new(WebChannel::new()),
            turn_lock: AsyncMutex::new(()),
            started: AsyncMutex::new(false),
        });
        // Forwarder: the bus is the single event source for this
        // session; every event the session's loop publishes is sent
        // to this session's channel exactly once. Mirrors the R3
        // forwarder, per session instead of per daemon.
        {
            let mut bus_rx = session.bus.subscribe();
            let ch = session.channel.clone();
            tokio::spawn(async move {
                loop {
                    match bus_rx.recv().await {
                        Ok(ev) => {
                            let _ = ch.send_event(&ev).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        sessions.insert(id.to_string(), session.clone());
        // Bound the live entries: evict the oldest-inserted IDLE
        // ones past the cap. A session with a running turn — its
        // turn_lock held, which also covers a turn blocked on a
        // pending approval, since the lock is held for the whole
        // turn — is never evicted: evicting it would orphan the
        // pending approval (a later resolve would hit a fresh empty
        // channel and answer `resolved: false`) and let a second
        // turn start on a fresh entry while the first is still
        // blocked (re-checker round 2, P3-1). If every live entry
        // is busy, the map overshoots the cap until a turn
        // finishes — bounded, and safer than orphaning an
        // approval. The just-created entry is newest, so it is
        // never the one evicted here.
        // Residual race (documented): the idle probe and the
        // removal are adjacent synchronous instructions, so a turn
        // that starts in the microsecond between them can still be
        // evicted; its consequence is the same bounded fail-closed
        // timeout as before the fix. Closing even that would need
        // the turn to re-validate against the registry at start,
        // which is out of proportion for this cap.
        if let Ok(mut order) = self.order.lock() {
            order.push_back(id.to_string());
            while sessions.len() > MAX_LIVE_SESSIONS {
                // Oldest entry whose turn lock is free right now.
                // The just-created entry is never a candidate: it
                // was ensured a moment ago and evicting it at once
                // would only force the caller to re-create it.
                let victim = order.iter().find(|candidate| {
                    candidate.as_str() != id
                        && sessions
                            .get(*candidate)
                            .is_some_and(|s| s.turn_lock.try_lock().is_ok())
                });
                let Some(victim) = victim.cloned() else {
                    break; // all busy: overshoot the cap for now
                };
                sessions.remove(&victim);
                order.retain(|c| c != &victim);
            }
        }
        Ok(session)
    }

    /// Live entry for `id` when it already exists; `None` for an
    /// unknown or invalid id. Never creates anything.
    pub fn get_session(&self, id: &str) -> Option<Arc<WebSession>> {
        if !is_valid_web_session_id(id) {
            return None;
        }
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(id).cloned())
    }

    /// Live entry for `id`, creating it when the session exists on
    /// disk. This is the call the HTTP handlers must use for a
    /// client-supplied id: after a daemon restart the map is empty,
    /// and a resumed session has a file but no live entry yet, so
    /// `get_session` alone would 404 a chat the user can see in the
    /// list (re-checker P2-1). A valid id with no file still fails
    /// instead of materializing a ghost session.
    ///
    /// Note: re-creating the entry also resets the per-session
    /// `session_start` flag, so the plugin hook fires again for a
    /// resumed (or evicted-and-recreated) session. There is no
    /// durable marker for "hook already fired"; the hook is
    /// idempotent setup, and a restart is a new daemon lifetime.
    pub fn ensure_session_if_exists(&self, id: &str) -> Result<Arc<WebSession>> {
        if !is_valid_web_session_id(id) {
            return Err(PurrError::Session(format!("invalid web session id '{id}'")));
        }
        if !self.store.session_exists(id) {
            return Err(PurrError::Session(format!(
                "web session '{id}' does not exist"
            )));
        }
        self.ensure_session(id)
    }

    /// The legacy default session (`purr_web`), created on first use.
    pub fn default_session(&self) -> Result<Arc<WebSession>> {
        self.ensure_session(DEFAULT_WEB_SESSION_ID)
    }

    /// Mint a fresh session: a new id, an empty JSONL file on disk
    /// (so it shows up in the list at once, before its first
    /// message), and a live runtime entry.
    pub fn create_session(&self) -> Result<Arc<WebSession>> {
        let id = new_web_session_id();
        self.store.create_session(&id)?;
        self.ensure_session(&id)
    }

    /// All web sessions on disk (valid ids only), most recently
    /// written first. A session written by an older daemon version
    /// (`purr_web`) and sessions minted here both appear.
    pub fn list_sessions(&self) -> Vec<WebSessionInfo> {
        let mut infos: Vec<WebSessionInfo> = self
            .store
            .list_sessions()
            .into_iter()
            .filter(|id| is_valid_web_session_id(id))
            .map(|id| {
                let (message_count, title) = self.scan_session(&id);
                WebSessionInfo {
                    id,
                    message_count,
                    title,
                }
            })
            .collect();
        infos.sort_by(|a, b| {
            let mtime = |id: &str| {
                std::fs::metadata(self.store.session_jsonl_path(id))
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            };
            mtime(&b.id)
                .cmp(&mtime(&a.id))
                .then_with(|| a.id.cmp(&b.id))
        });
        infos
    }

    /// Full history of one session, for replay when the user opens
    /// it. Fails on an invalid id and on a session with no file on
    /// disk (a minted-but-never-written session has an empty file,
    /// so it returns an empty vec instead).
    pub fn transcript(&self, id: &str) -> Result<Vec<ChatMessage>> {
        if !is_valid_web_session_id(id) {
            return Err(PurrError::Session(format!("invalid web session id '{id}'")));
        }
        if !self.store.session_exists(id) {
            return Err(PurrError::Session(format!(
                "web session '{id}' does not exist"
            )));
        }
        self.store.load_messages(id)
    }

    /// One cheap pass over a session's JSONL: count parseable
    /// message lines and take the first user message as the title.
    /// Parses each line as a [`SessionTurnEntry`] — the exact same
    /// parse [`SessionStore::load_messages`] uses — so the count
    /// always equals the transcript length, even on a hand-edited or
    /// corrupt file (re-checker P3-1: counting any line with a
    /// `"message"` key disagreed with the transcript on unknown
    /// roles and wire-shaped `tool_calls`).
    fn scan_session(&self, id: &str) -> (usize, String) {
        let path = self.store.session_jsonl_path(id);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return (0, String::new()),
        };
        let mut count = 0;
        let mut title = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<SessionTurnEntry>(trimmed) else {
                continue;
            };
            count += 1;
            if title.is_empty() && entry.message.role == MessageRole::User {
                title = entry
                    .message
                    .content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(TITLE_CHARS)
                    .collect();
            }
        }
        (count, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::{ChatMessage, MessageRole};

    fn test_registry(name: &str) -> (WebSessionRegistry, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("purrcode-web-sess-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (WebSessionRegistry::new(&dir), dir)
    }

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: MessageRole::User,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: MessageRole::Assistant,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_session_id_validation() {
        assert!(is_valid_web_session_id("purr_web"));
        assert!(is_valid_web_session_id("purr_web_0123456789ab"));
        assert!(is_valid_web_session_id("purr_web_abcdefabcdef"));
        // Foreign sessions, traversal, and malformed ids all fail.
        assert!(!is_valid_web_session_id("purr_web_0123456789AB")); // uppercase hex
        assert!(!is_valid_web_session_id("purr_web_0123456789a")); // 11 hex
        assert!(!is_valid_web_session_id("purr_web_0123456789abc")); // 13 hex
        assert!(!is_valid_web_session_id("purr_web_"));
        assert!(!is_valid_web_session_id("purr_web_x"));
        assert!(!is_valid_web_session_id("purr_cron_0123456789ab"));
        assert!(!is_valid_web_session_id("purr_hb_0123456789ab"));
        assert!(!is_valid_web_session_id("purr_0123456789ab"));
        assert!(!is_valid_web_session_id("purr_web_../../etc/passwd"));
        assert!(!is_valid_web_session_id("purr_web_0123456789ab/../x"));
        assert!(!is_valid_web_session_id(""));
        assert!(!is_valid_web_session_id("purr_web_0123456789a g"));
    }

    #[test]
    fn test_new_session_id_shape() {
        for _ in 0..10 {
            let id = new_web_session_id();
            assert!(
                is_valid_web_session_id(&id),
                "minted id must be valid: {id}"
            );
        }
    }

    #[tokio::test]
    async fn test_ensure_same_session_twice_returns_same_entry() {
        let (reg, dir) = test_registry("same");
        let a = reg.ensure_session("purr_web_0123456789ab").unwrap();
        let b = reg.ensure_session("purr_web_0123456789ab").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(reg.ensure_session("purr_web_../x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_create_list_transcript_round_trip() {
        let (reg, dir) = test_registry("roundtrip");
        // A foreign session on disk must never appear in the web list.
        let store = SessionStore::new(&dir);
        store
            .append_message("purr_cron_0123456789ab", &user_msg("cron"))
            .unwrap();
        store
            .append_message("purr_web", &user_msg("hello from legacy"))
            .unwrap();
        store
            .append_message("purr_web", &assistant_msg("hi"))
            .unwrap();

        let created = reg.create_session().unwrap();
        assert!(is_valid_web_session_id(&created.id));
        assert!(store.session_exists(&created.id));

        // The new (empty) session lists with 0 messages; the legacy
        // one lists with 2 and a title from its first user message.
        let list = reg.list_sessions();
        assert_eq!(list.len(), 2);
        let legacy = list.iter().find(|s| s.id == "purr_web").unwrap();
        assert_eq!(legacy.message_count, 2);
        assert_eq!(legacy.title, "hello from legacy");
        let fresh = list.iter().find(|s| s.id == created.id).unwrap();
        assert_eq!(fresh.message_count, 0);
        assert_eq!(fresh.title, "");

        // Transcript: legacy replays both messages; unknown fails.
        let msgs = reg.transcript("purr_web").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "hello from legacy");
        assert!(reg.transcript("purr_web_0123456789ab").is_err());
        assert!(reg.transcript("purr_web_../x").is_err());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_list_sorted_by_mtime_desc() {
        let (reg, dir) = test_registry("mtime");
        let store = SessionStore::new(&dir);
        store.append_message("purr_web", &user_msg("old")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let fresh = reg.create_session().unwrap();
        store.append_message(&fresh.id, &user_msg("new")).unwrap();
        let list = reg.list_sessions();
        assert_eq!(list[0].id, fresh.id);
        assert_eq!(list[1].id, "purr_web");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_ensure_session_if_exists_requires_file() {
        let (reg, dir) = test_registry("ensure-exists");
        // Valid id, no file on disk: no ghost live entry.
        assert!(reg
            .ensure_session_if_exists("purr_web_0123456789ab")
            .is_err());
        assert!(reg.get_session("purr_web_0123456789ab").is_none());
        // Invalid id fails too.
        assert!(reg.ensure_session_if_exists("purr_web_../x").is_err());
        // Once the file exists (a resumed session after restart),
        // the live entry materializes on demand.
        let store = SessionStore::new(&dir);
        store
            .append_message("purr_web_0123456789ab", &user_msg("hi"))
            .unwrap();
        let session = reg
            .ensure_session_if_exists("purr_web_0123456789ab")
            .unwrap();
        assert_eq!(session.id, "purr_web_0123456789ab");
        assert!(reg.get_session("purr_web_0123456789ab").is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_scan_counts_only_loadable_entries() {
        // Re-checker P3-1: a hand-edited JSONL with lines that
        // `load_messages` drops must not inflate `message_count`.
        let (reg, dir) = test_registry("scan-parity");
        let store = SessionStore::new(&dir);
        let id = "purr_web_0123456789ab";
        let entry = |role: &str, content: &str| {
            serde_json::json!({
                "version": "session/v1",
                "session_id": id,
                "timestamp": "2026-11-13T00:00:00Z",
                "message": {"role": role, "content": content,
                            "tool_calls": null, "tool_call_id": null}
            })
            .to_string()
        };
        let wire_tool_calls = serde_json::json!({
            "version": "session/v1", "session_id": id,
            "timestamp": "2026-11-13T00:00:00Z",
            "message": {"role": "assistant", "content": "",
                        "tool_calls": [{"id": "c1", "type": "function",
                                        "function": {"name": "bash", "arguments": "{}"}}],
                        "tool_call_id": null}
        })
        .to_string();
        let raw = [
            entry("user", "first user message"),
            entry("assistant", "reply"),
            entry("weird", "unknown role"),
            wire_tool_calls,
            "{not json".to_string(),
            r#"{"no_message_key": true}"#.to_string(),
        ]
        .join("\n");
        std::fs::create_dir_all(store.sessions_dir()).unwrap();
        std::fs::write(store.session_jsonl_path(id), raw).unwrap();

        let list = reg.list_sessions();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message_count, 2);
        assert_eq!(list[0].title, "first user message");
        // The transcript agrees by construction (same parse).
        assert_eq!(reg.transcript(id).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_live_entries_evict_oldest_past_cap() {
        // Re-checker P3-2: live entries are bounded; eviction never
        // touches the durable list, and an evicted session can be
        // re-created on demand.
        let (reg, dir) = test_registry("evict");
        let mut ids = Vec::new();
        for _ in 0..70 {
            ids.push(reg.create_session().unwrap().id.clone());
        }
        // Disk still lists all 70 sessions.
        assert_eq!(reg.list_sessions().len(), 70);
        // But only the newest 64 have live entries.
        assert!(reg.get_session(&ids[0]).is_none());
        assert!(reg.get_session(&ids[5]).is_none());
        assert!(reg.get_session(&ids[6]).is_some());
        assert!(reg.get_session(&ids[69]).is_some());
        // An evicted session re-materializes on demand.
        let back = reg.ensure_session_if_exists(&ids[0]).unwrap();
        assert_eq!(back.id, ids[0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_eviction_skips_busy_session() {
        // Re-checker round 2, P3-1: a session with a running turn
        // (its turn_lock held — which also covers a turn blocked
        // on a pending approval) is never evicted, even when it
        // is the oldest entry; once idle it becomes evictable.
        let (reg, dir) = test_registry("evict-busy");
        let busy = reg.create_session().unwrap();
        let guard = busy.turn_lock.try_lock().unwrap();
        let mut ids = vec![busy.id.clone()];
        for _ in 0..65 {
            ids.push(reg.create_session().unwrap().id.clone());
        }
        // 66 live entries -> 2 evicted, both idle; the busy one
        // (oldest) survives, as does the newest.
        assert!(reg.get_session(&ids[0]).is_some());
        assert!(reg.get_session(&ids[1]).is_none());
        assert!(reg.get_session(&ids[2]).is_none());
        assert!(reg.get_session(&ids[65]).is_some());
        // Once the turn ends, the next ensure evicts it first.
        drop(guard);
        reg.create_session().unwrap();
        assert!(reg.get_session(&ids[0]).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_eviction_overshoots_when_all_busy() {
        // When every live entry is busy, the map grows past the
        // cap instead of orphaning a running turn's approval.
        let (reg, dir) = test_registry("evict-all-busy");
        let mut sessions = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..64 {
            let s = reg.create_session().unwrap();
            ids.push(s.id.clone());
            sessions.push(s);
        }
        // Hold every turn lock (the guards borrow `sessions`, which
        // stays alive until the end of the test).
        let mut guards = Vec::new();
        for s in &sessions {
            guards.push(s.turn_lock.try_lock().unwrap());
        }
        let extra = reg.create_session().unwrap();
        ids.push(extra.id.clone());
        // All 65 entries are still live (the new one is never an
        // eviction candidate at its own creation).
        for id in &ids {
            assert!(reg.get_session(id).is_some(), "live: {id}");
        }
        drop(guards);
        let _ = std::fs::remove_dir_all(dir);
    }
}
