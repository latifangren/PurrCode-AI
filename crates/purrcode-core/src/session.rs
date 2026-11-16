// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{
    ChatMessage, CompletionRequest, MessageRole, ModelProvider, ToolCallRequest,
};
use purrcode_store::rusqlite::{params, Connection, OptionalExtension};
use purrcode_store::{sql, Store, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

/// Web-session metadata (title + per-session preset/model overrides).
/// Formerly `sessions/<id>.meta.json`; stored on the `sessions` row
/// (`title`, `web_preset`, `web_model`). The file is gone (ADR-015).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurnEntry {
    pub version: String,
    pub session_id: String,
    pub timestamp: String,
    pub message: ChatMessage,
}

/// Durable session transcripts, backed by the SQLite store (ADR-015).
/// One row per session in `sessions`, one row per message in `turns`,
/// one row per provider tool call in `tool_calls`. The old flat files
/// (`sessions/<id>.jsonl`) are gone; subagent handoff directories
/// (`sessions/<id>/subagents/<id>/brief.md` + `result.md`) still live
/// on disk under [`SessionStore::session_dir`].
pub struct SessionStore {
    root_dir: PathBuf,
    store: Arc<Store>,
}

/// One row of the `turns` table as read back for a fork copy:
/// (id, seq, role, content, tool_call_id, tool_calls_json,
/// prompt_tokens, completion_tokens, created_at).
type TurnRow = (
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    String,
);

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn role_str(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn parse_role(s: &str) -> Option<MessageRole> {
    match s {
        "system" => Some(MessageRole::System),
        "user" => Some(MessageRole::User),
        "assistant" => Some(MessageRole::Assistant),
        "tool" => Some(MessageRole::Tool),
        _ => None,
    }
}

fn ensure_session(
    conn: &Connection,
    session_id: &str,
    now: &str,
) -> std::result::Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![session_id, now],
    )?;
    Ok(())
}

fn insert_turn(
    conn: &Connection,
    session_id: &str,
    seq: i64,
    message: &ChatMessage,
    now: &str,
) -> std::result::Result<i64, StoreError> {
    let tool_calls_json = match &message.tool_calls {
        Some(calls) => Some(serde_json::to_string(calls)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO turns (session_id, seq, role, content, tool_call_id, tool_calls_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id,
            seq,
            role_str(&message.role),
            message.content,
            message.tool_call_id,
            tool_calls_json,
            now
        ],
    )?;
    let turn_id = conn.last_insert_rowid();
    if let Some(calls) = &message.tool_calls {
        for call in calls {
            conn.execute(
                "INSERT INTO tool_calls (id, turn_id, session_id, name, args_json, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'done', ?6)",
                params![call.id, turn_id, session_id, call.name, call.arguments, now],
            )?;
        }
    }
    Ok(turn_id)
}

impl SessionStore {
    /// Open the store at `<root_dir>/purrcode.db`, creating it (and the
    /// directory) on first use. Fails hard when the database cannot be
    /// opened or migrated — never a silent fallback to flat files.
    pub fn new(root_dir: impl Into<PathBuf>) -> Result<Self> {
        let root_dir = root_dir.into();
        let store = Store::open(&root_dir)?;
        Ok(Self {
            root_dir,
            store: Arc::new(store),
        })
    }

    /// On-disk home of a session's subagent handoff directories
    /// (`sessions/<id>/subagents/<id>/`). Kept on the filesystem:
    /// handoff `brief.md`/`result.md` are Markdown, not machine state.
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root_dir.join("sessions").join(session_id)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let guard = self.store.lock()?;
        // CASCADE removes the turns and tool calls; a forked child
        // keeps existing (its parent_id goes NULL, it is not deleted).
        sql(guard.execute("DELETE FROM sessions WHERE id = ?1", [session_id]))?;
        drop(guard);
        let dir_path = self.session_dir(session_id);
        if dir_path.exists() && dir_path.is_dir() {
            std::fs::remove_dir_all(&dir_path).map_err(PurrError::Io)?;
        }
        Ok(())
    }

    /// Create an empty session row so a fresh session shows up in
    /// `list_sessions` before its first message is appended (v0.4 B1:
    /// the WebUI "new session" button must list the session at once).
    /// Idempotent: an existing row is left untouched.
    pub fn create_session(&self, session_id: &str) -> Result<()> {
        let now = now_rfc3339();
        let guard = self.store.lock()?;
        ensure_session(&guard, session_id, &now)?;
        Ok(())
    }

    /// True when the session has a row in the store.
    pub fn session_exists(&self, session_id: &str) -> Result<bool> {
        let guard = self.store.lock()?;
        let exists: bool = sql(guard.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            [session_id],
            |row| row.get(0),
        ))?;
        Ok(exists)
    }

    pub fn append_message(&self, session_id: &str, message: &ChatMessage) -> Result<()> {
        let now = now_rfc3339();
        let mut guard = self.store.lock()?;
        let tx = sql(guard.transaction())?;
        ensure_session(&tx, session_id, &now)?;
        let seq: i64 = sql(tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM turns WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        ))?;
        insert_turn(&tx, session_id, seq, message, &now)?;
        sql(tx.execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![session_id, now],
        ))?;
        sql(tx.commit())?;
        Ok(())
    }

    /// Full history of one session, oldest first, with the stored
    /// per-turn timestamps (for the raw JSONL export).
    fn load_entries(&self, session_id: &str) -> Result<Vec<SessionTurnEntry>> {
        let guard = self.store.lock()?;
        let mut stmt = sql(guard.prepare(
            "SELECT role, content, tool_call_id, tool_calls_json, created_at
             FROM turns WHERE session_id = ?1 ORDER BY seq",
        ))?;
        let rows = sql(stmt.query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        }))?;
        let mut entries = Vec::new();
        for row in rows {
            let (role_s, content, tool_call_id, tool_calls_json, timestamp) =
                row.map_err(StoreError::from)?;
            let Some(role) = parse_role(&role_s) else {
                return Err(PurrError::Session(format!(
                    "corrupt session '{session_id}': unknown role '{role_s}'"
                )));
            };
            let tool_calls = match tool_calls_json {
                Some(json) => Some(serde_json::from_str::<Vec<ToolCallRequest>>(&json)?),
                None => None,
            };
            entries.push(SessionTurnEntry {
                version: "session/v1".to_string(),
                session_id: session_id.to_string(),
                timestamp,
                message: ChatMessage {
                    role,
                    content,
                    tool_calls,
                    tool_call_id,
                },
            });
        }
        Ok(entries)
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        Ok(self
            .load_entries(session_id)?
            .into_iter()
            .map(|entry| entry.message)
            .collect())
    }

    pub fn replace_messages(&self, session_id: &str, messages: &[ChatMessage]) -> Result<()> {
        let now = now_rfc3339();
        let mut guard = self.store.lock()?;
        let tx = sql(guard.transaction())?;
        ensure_session(&tx, session_id, &now)?;
        // CASCADE removes the old tool calls with the turns.
        sql(tx.execute("DELETE FROM turns WHERE session_id = ?1", [session_id]))?;
        for (i, message) in messages.iter().enumerate() {
            insert_turn(&tx, session_id, i as i64 + 1, message, &now)?;
        }
        sql(tx.execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![session_id, now],
        ))?;
        sql(tx.commit())?;
        Ok(())
    }

    pub fn fork_session(&self, source_id: &str, new_id: &str) -> Result<()> {
        let now = now_rfc3339();
        let mut guard = self.store.lock()?;
        let tx = sql(guard.transaction())?;
        let source_exists: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            [source_id],
            |row| row.get(0),
        ))?;
        if !source_exists {
            return Err(PurrError::Session(format!(
                "Source session '{source_id}' does not exist"
            )));
        }
        let (title, preset_json, web_preset, web_model): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = sql(tx.query_row(
            "SELECT title, preset_json, web_preset, web_model FROM sessions WHERE id = ?1",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ))?;
        sql(tx.execute(
            "INSERT INTO sessions (id, title, parent_id, preset_json, web_preset, web_model, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![new_id, title, source_id, preset_json, web_preset, web_model, now],
        ))?;
        // Copy turns, remapping the AUTOINCREMENT turn ids by seq so
        // the tool calls land on the copied turns.
        let old_turns: Vec<TurnRow> = {
            let mut stmt = sql(tx.prepare(
                "SELECT id, seq, role, content, tool_call_id, tool_calls_json,
                        prompt_tokens, completion_tokens, created_at
                 FROM turns WHERE session_id = ?1 ORDER BY seq",
            ))?;
            let rows = sql(stmt.query_map([source_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            }))?;
            sql(rows.collect::<purrcode_store::rusqlite::Result<Vec<_>>>())?
        };
        let mut id_map: HashMap<i64, i64> = HashMap::new();
        for (
            old_id,
            seq,
            role,
            content,
            tool_call_id,
            tool_calls_json,
            prompt_tokens,
            completion_tokens,
            created_at,
        ) in &old_turns
        {
            sql(tx.execute(
                "INSERT INTO turns (session_id, seq, role, content, tool_call_id, tool_calls_json, prompt_tokens, completion_tokens, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new_id,
                    seq,
                    role,
                    content,
                    tool_call_id,
                    tool_calls_json,
                    prompt_tokens,
                    completion_tokens,
                    created_at
                ],
            ))?;
            id_map.insert(*old_id, tx.last_insert_rowid());
        }
        // Copy tool calls with remapped turn ids. Provider call ids
        // are kept verbatim — the composite PK (session_id, id) keeps
        // the fork legal, and the ids stay linked to their turns.
        let old_calls: Vec<(String, i64, String, String, String, String)> = {
            let mut stmt = sql(tx.prepare(
                "SELECT id, turn_id, name, args_json, status, created_at
                 FROM tool_calls WHERE session_id = ?1",
            ))?;
            let rows = sql(stmt.query_map([source_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            }))?;
            sql(rows.collect::<purrcode_store::rusqlite::Result<Vec<_>>>())?
        };
        for (call_id, old_turn_id, name, args_json, status, created_at) in &old_calls {
            let Some(new_turn_id) = id_map.get(old_turn_id).copied() else {
                return Err(PurrError::Internal(format!(
                    "session store: fork of '{source_id}': tool call '{call_id}' references unknown turn {old_turn_id}"
                )));
            };
            sql(tx.execute(
                "INSERT INTO tool_calls (id, turn_id, session_id, name, args_json, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![call_id, new_turn_id, new_id, name, args_json, status, created_at],
            ))?;
        }
        sql(tx.commit())?;
        Ok(())
    }

    /// Snapshot the active mode + model presets for a session (TUI
    /// session start). Stored as one JSON object in
    /// `sessions.preset_json`; nothing reads it back yet — it is the
    /// resume payload the spec reserves. Upsert: never bumps
    /// `updated_at`, so a snapshot does not reorder the session list.
    pub fn save_preset_snapshots(
        &self,
        session_id: &str,
        preset_json: &serde_json::Value,
        model_preset_json: &serde_json::Value,
    ) -> Result<()> {
        let now = now_rfc3339();
        let combined =
            serde_json::json!({"preset": preset_json, "model_preset": model_preset_json});
        let guard = self.store.lock()?;
        sql(guard.execute(
            "INSERT INTO sessions (id, preset_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(id) DO UPDATE SET preset_json = excluded.preset_json",
            params![session_id, combined.to_string(), now],
        ))?;
        Ok(())
    }

    /// Web-session metadata for one session. A missing row (or an
    /// empty title) reads as the default — metadata is best-effort.
    pub fn load_meta(&self, session_id: &str) -> Result<SessionMeta> {
        let guard = self.store.lock()?;
        let row: Option<(String, Option<String>, Option<String>)> = sql(guard
            .query_row(
                "SELECT title, web_preset, web_model FROM sessions WHERE id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional())?;
        Ok(match row {
            Some((title, preset, model)) => SessionMeta {
                title: if title.is_empty() { None } else { Some(title) },
                preset,
                model,
            },
            None => SessionMeta::default(),
        })
    }

    /// Persist web-session metadata. Upsert: never bumps `updated_at`
    /// on an existing row, so renaming a session does not reorder
    /// the session list.
    pub fn save_meta(&self, session_id: &str, meta: &SessionMeta) -> Result<()> {
        let now = now_rfc3339();
        let guard = self.store.lock()?;
        sql(guard.execute(
            "INSERT INTO sessions (id, title, web_preset, web_model, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               web_preset = excluded.web_preset,
               web_model = excluded.web_model",
            params![
                session_id,
                meta.title.clone().unwrap_or_default(),
                meta.preset,
                meta.model,
                now
            ],
        ))?;
        Ok(())
    }

    /// Rough character-based token estimate (~4 chars per token plus overhead)
    /// used to decide when automatic session compaction should run.
    pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .map(|m| {
                let mut chars = m.content.len() + 16;
                if let Some(ref calls) = m.tool_calls {
                    for call in calls {
                        chars += call.name.len() + call.arguments.len() + 24;
                    }
                }
                chars / 4
            })
            .sum()
    }

    /// Finds a safe `tail_start` index >= `start_idx` that never orphans a
    /// `Tool` response away from its preceding `Assistant(tool_calls)` turn.
    fn safe_compaction_tail_start(
        messages: &[ChatMessage],
        start_idx: usize,
        keep_last: usize,
    ) -> usize {
        let mut tail_start = messages.len().saturating_sub(keep_last).max(start_idx);
        while tail_start > start_idx && tail_start < messages.len() {
            if messages[tail_start].role == MessageRole::Tool {
                tail_start -= 1;
            } else {
                break;
            }
        }
        tail_start
    }

    /// Non-destructive structured compaction: preserves raw JSONL on disk while
    /// returning a compacted in-memory message vector keeping system prompt, a
    /// structured engineering summary (with cleared-tool-output warning), and
    /// the most recent `keep_last` messages without splitting tool-call pairs.
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

        let tail_start = Self::safe_compaction_tail_start(messages, start_idx, keep_last);
        if tail_start <= start_idx {
            return messages.to_vec();
        }
        let summarized_slice = &messages[start_idx..tail_start];

        let mut user_requests = Vec::new();
        let mut files_touched = BTreeSet::new();
        let mut commands_run = Vec::new();
        let mut failures = Vec::new();
        let mut assistant_notes = Vec::new();
        let mut cleared_tool_count = 0usize;

        for msg in summarized_slice {
            match msg.role {
                MessageRole::User => {
                    let snippet: String = msg.content.trim().chars().take(240).collect();
                    if !snippet.is_empty() {
                        user_requests.push(snippet);
                    }
                }
                MessageRole::Assistant => {
                    if let Some(ref calls) = msg.tool_calls {
                        for call in calls {
                            if let Ok(val) =
                                serde_json::from_str::<serde_json::Value>(&call.arguments)
                            {
                                if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                                    files_touched.insert(format!("{} ({})", path, call.name));
                                }
                                if call.name == "bash" {
                                    if let Some(cmd) = val.get("command").and_then(|v| v.as_str()) {
                                        let short: String = cmd.trim().chars().take(120).collect();
                                        commands_run.push(short);
                                    }
                                }
                            }
                        }
                    }
                    let trimmed = msg.content.trim();
                    if !trimmed.is_empty() {
                        let snippet: String = trimmed.chars().take(240).collect();
                        assistant_notes.push(snippet);
                    }
                }
                MessageRole::Tool => {
                    cleared_tool_count += 1;
                    let lower = msg.content.to_lowercase();
                    if lower.contains("error")
                        || lower.contains("failed")
                        || lower.contains("denied")
                        || lower.contains("vetoed")
                    {
                        let snippet: String = msg.content.trim().chars().take(200).collect();
                        failures.push(snippet);
                    }
                }
                MessageRole::System => {
                    let trimmed = msg.content.trim();
                    if !trimmed.is_empty() {
                        let snippet: String = trimmed.chars().take(240).collect();
                        assistant_notes.push(snippet);
                    }
                }
            }
        }

        let mut sections = Vec::new();
        if let Some(focus) = focus_note {
            sections.push(format!("Focus: {}", focus));
        }
        if !user_requests.is_empty() {
            sections.push(format!(
                "User Requests:\n{}",
                user_requests
                    .iter()
                    .map(|r| format!("- {}", r))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !files_touched.is_empty() {
            sections.push(format!(
                "Files Touched/Inspected:\n{}",
                files_touched
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !commands_run.is_empty() {
            sections.push(format!(
                "Commands Run:\n{}",
                commands_run
                    .iter()
                    .take(12)
                    .map(|c| format!("- `{}`", c))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !failures.is_empty() {
            sections.push(format!(
                "Errors / Denials Observed:\n{}",
                failures
                    .iter()
                    .take(8)
                    .map(|e| format!("- {}", e))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !assistant_notes.is_empty() {
            let tail_notes: Vec<_> = assistant_notes.iter().rev().take(6).rev().collect();
            sections.push(format!(
                "Key Findings / Decisions:\n{}",
                tail_notes
                    .iter()
                    .map(|n| format!("- {}", n))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if cleared_tool_count > 0 {
            sections.push(format!(
                "<system-reminder>\n[{} earlier tool result(s) cleared during compaction] Never invent or extrapolate exact file contents, line numbers, or command outputs that were omitted above. Re-read the file or re-run the check if exact details are needed.\n</system-reminder>",
                cleared_tool_count
            ));
        }

        compacted.push(ChatMessage {
            role: MessageRole::System,
            content: format!(
                "### Compacted Session Summary ({} earlier messages preserved in the session store)\n{}",
                summarized_slice.len(),
                sections.join("\n\n")
            ),
            tool_calls: None,
            tool_call_id: None,
        });

        compacted.extend_from_slice(&messages[tail_start..]);
        compacted
    }

    /// LLM-assisted structured compaction with deterministic fallback.
    /// Summarizes earlier turns using the active provider before falling back
    /// to `compact_messages` if the provider call fails or produces empty output.
    pub async fn compact_messages_with_provider(
        provider: &Arc<dyn ModelProvider>,
        model: &str,
        messages: &[ChatMessage],
        focus_note: Option<&str>,
        keep_last: usize,
    ) -> Vec<ChatMessage> {
        let mut start_idx = 0;
        let mut system_head = None;
        if let Some(first) = messages.first() {
            if first.role == MessageRole::System {
                system_head = Some(first.clone());
                start_idx = 1;
            }
        }
        if messages.len().saturating_sub(start_idx) <= keep_last {
            return messages.to_vec();
        }
        let tail_start = Self::safe_compaction_tail_start(messages, start_idx, keep_last);
        if tail_start <= start_idx {
            return messages.to_vec();
        }

        let slice = &messages[start_idx..tail_start];
        let mut transcript = String::new();
        for msg in slice {
            let role = match msg.role {
                MessageRole::System => "SYSTEM",
                MessageRole::User => "USER",
                MessageRole::Assistant => "ASSISTANT",
                MessageRole::Tool => "TOOL",
            };
            let snippet: String = msg.content.chars().take(1200).collect();
            transcript.push_str(&format!("[{}]: {}\n", role, snippet));
            if let Some(ref calls) = msg.tool_calls {
                for c in calls {
                    let args_short: String = c.arguments.chars().take(300).collect();
                    transcript.push_str(&format!("  -> tool_call {}({})\n", c.name, args_short));
                }
            }
            if transcript.len() > 24_000 {
                transcript.push_str("\n...[earlier transcript truncated for compaction]...\n");
                break;
            }
        }

        let focus_clause = focus_note
            .map(|f| format!("Prioritize preserving details related to: {}.\n", f))
            .unwrap_or_default();
        let prompt = format!(
            "Summarize this engineering session slice into concise structured sections:\n\
            1. Primary User Requests & Open Objectives\n\
            2. Files Read/Modified (exact paths)\n\
            3. Technical Decisions & Current State\n\
            4. Verification Status & Errors Observed\n\
            {}\
            Be factual and concise. Omit raw verbose tool outputs.\n\nTranscript:\n{}",
            focus_clause, transcript
        );

        let req = CompletionRequest {
            model: model.to_string(),
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "You are a factual session compaction summarizer. Output concise markdown sections only.".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: None,
            temperature: Some(0.1),
            max_tokens: Some(1500),
            reasoning_effort: Some("low".to_string()),
            stop: None,
        };

        if let Ok(mut rx) = provider.stream_completion(req).await {
            let mut summary = String::new();
            let mut had_err = false;
            while let Some(chunk) = rx.recv().await {
                if chunk.stream_error.is_some() {
                    had_err = true;
                    break;
                }
                if !chunk.delta.is_empty() {
                    summary.push_str(&chunk.delta);
                }
            }
            let summary = summary.trim().to_string();
            if !had_err && !summary.is_empty() {
                let mut out = Vec::new();
                if let Some(sys) = system_head {
                    out.push(sys);
                }
                out.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "### Compacted Session Summary ({} earlier messages preserved in the session store)\n{}\n\n<system-reminder>\n[Old tool outputs cleared during compaction] Never invent omitted file contents or command outputs; re-read files or re-run checks if exact details are needed.\n</system-reminder>",
                        slice.len(),
                        summary
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                });
                out.extend_from_slice(&messages[tail_start..]);
                return out;
            }
        }

        Self::compact_messages(messages, focus_note, keep_last)
    }

    /// All session ids, sorted by id (CLI `session list`).
    pub fn list_sessions(&self) -> Vec<String> {
        let Ok(guard) = self.store.lock() else {
            return Vec::new();
        };
        let Ok(mut stmt) = guard.prepare("SELECT id FROM sessions ORDER BY id") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    /// All session ids, most recently updated first (WebUI list).
    pub fn list_sessions_by_updated(&self) -> Vec<String> {
        let Ok(guard) = self.store.lock() else {
            return Vec::new();
        };
        let Ok(mut stmt) =
            guard.prepare("SELECT id FROM sessions ORDER BY updated_at DESC, id ASC")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    pub fn export_session(&self, session_id: &str, format: &str) -> Result<String> {
        if !self.session_exists(session_id)? {
            return Err(PurrError::Session(format!(
                "Session '{session_id}' does not exist"
            )));
        }
        let entries = self.load_entries(session_id)?;
        if format.eq_ignore_ascii_case("html") {
            let mut html = String::from(
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>PurrCode Session</title></head><body>\n",
            );
            html.push_str(&format!("<h1>Session: {}</h1>\n", session_id));
            for entry in entries {
                html.push_str(&format!(
                    "<div class=\"msg\"><strong>{:?}:</strong><pre>{}</pre></div>\n",
                    entry.message.role,
                    entry
                        .message
                        .content
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                ));
            }
            html.push_str("</body></html>");
            Ok(html)
        } else {
            // Raw JSONL, line-for-line the old `sessions/<id>.jsonl`
            // format, reconstructed from the stored turns.
            let mut out = String::new();
            for entry in entries {
                out.push_str(&serde_json::to_string(&entry)?);
                out.push('\n');
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_append_fork_and_compact() {
        let temp_dir = std::env::temp_dir().join(format!("purr_sess_{}", uuid::Uuid::new_v4()));
        let store = SessionStore::new(&temp_dir).unwrap();

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

        assert!(store.session_exists("purr_test1").unwrap());
        store.delete_session("purr_test1").unwrap();
        assert!(!store.session_exists("purr_test1").unwrap());

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_compact_preserves_tool_call_pairs_and_structured_sections() {
        use purrcode_api::provider::ToolCallRequest;
        let msgs = vec![
            ChatMessage {
                role: MessageRole::System,
                content: "sys".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::User,
                content: "Inspect src/lib.rs".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCallRequest {
                    id: "c1".to_string(),
                    name: "read".to_string(),
                    arguments: "{\"path\":\"src/lib.rs\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::Tool,
                content: "Error: file missing".to_string(),
                tool_calls: None,
                tool_call_id: Some("c1".to_string()),
            },
            ChatMessage {
                role: MessageRole::User,
                content: "Try Cargo.toml".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCallRequest {
                    id: "c2".to_string(),
                    name: "read".to_string(),
                    arguments: "{\"path\":\"Cargo.toml\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::Tool,
                content: "[package]".to_string(),
                tool_calls: None,
                tool_call_id: Some("c2".to_string()),
            },
        ];

        // keep_last = 1 would land directly on Tool(c2); safe boundary must back up to Assistant(c2)
        let compacted = SessionStore::compact_messages(&msgs, Some("audit"), 1);
        assert_eq!(compacted[0].role, MessageRole::System);
        assert!(compacted[1].content.contains("src/lib.rs (read)"));
        assert!(compacted[1].content.contains("Errors / Denials Observed"));
        assert!(compacted[1].content.contains("<system-reminder>"));
        assert_eq!(compacted[2].role, MessageRole::Assistant);
        assert_eq!(compacted[3].role, MessageRole::Tool);
    }

    fn test_store() -> (SessionStore, PathBuf) {
        let temp_dir = std::env::temp_dir().join(format!("purr_sess_{}", uuid::Uuid::new_v4()));
        (SessionStore::new(&temp_dir).unwrap(), temp_dir)
    }

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: MessageRole::User,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_fork_keeps_tool_call_ids() {
        use purrcode_api::provider::ToolCallRequest;
        let (store, dir) = test_store();
        let assistant = ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls: Some(vec![ToolCallRequest {
                id: "call-9".to_string(),
                name: "read".to_string(),
                arguments: "{\"path\":\"x\"}".to_string(),
            }]),
            tool_call_id: None,
        };
        let tool = ChatMessage {
            role: MessageRole::Tool,
            content: "contents".to_string(),
            tool_calls: None,
            tool_call_id: Some("call-9".to_string()),
        };
        store.append_message("purr_src", &user_msg("hi")).unwrap();
        store.append_message("purr_src", &assistant).unwrap();
        store.append_message("purr_src", &tool).unwrap();
        // Same provider call id in both sessions: legal only because
        // the tool_calls PK is (session_id, id).
        store.fork_session("purr_src", "purr_dst").unwrap();
        let src = store.load_messages("purr_src").unwrap();
        let dst = store.load_messages("purr_dst").unwrap();
        assert_eq!(src.len(), 3);
        assert_eq!(dst.len(), 3);
        assert_eq!(dst[1].tool_calls.as_ref().unwrap()[0].id, "call-9");
        assert_eq!(dst[2].tool_call_id.as_deref(), Some("call-9"));
        assert!(store.fork_session("purr_nope", "purr_x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_export_raw_round_trips_jsonl() {
        let (store, dir) = test_store();
        store.append_message("purr_exp", &user_msg("one")).unwrap();
        store.append_message("purr_exp", &user_msg("two")).unwrap();
        let raw = store.export_session("purr_exp", "jsonl").unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let entry: SessionTurnEntry = serde_json::from_str(line).unwrap();
            assert_eq!(entry.session_id, "purr_exp");
            assert_eq!(entry.version, "session/v1");
        }
        assert!(store.export_session("purr_nope", "jsonl").is_err());
        let html = store.export_session("purr_exp", "html").unwrap();
        assert!(html.contains("Session: purr_exp"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_meta_save_load_round_trip() {
        let (store, dir) = test_store();
        assert_eq!(
            store.load_meta("purr_nope").unwrap(),
            SessionMeta::default()
        );
        let meta = SessionMeta {
            title: Some("My chat".to_string()),
            preset: Some("tabby".to_string()),
            model: Some("magpie:foo/bar".to_string()),
        };
        store.save_meta("purr_meta", &meta).unwrap();
        assert_eq!(store.load_meta("purr_meta").unwrap(), meta);
        // Saving meta creates the row: the session lists.
        assert!(store.list_sessions().contains(&"purr_meta".to_string()));
        // Empty title reads back as no title.
        store
            .save_meta(
                "purr_meta",
                &SessionMeta {
                    title: Some(String::new()),
                    preset: None,
                    model: None,
                },
            )
            .unwrap();
        assert_eq!(store.load_meta("purr_meta").unwrap().title, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_list_sorted_by_updated() {
        let (store, dir) = test_store();
        store.append_message("purr_old", &user_msg("old")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        store.append_message("purr_new", &user_msg("new")).unwrap();
        assert_eq!(store.list_sessions_by_updated()[0], "purr_new");
        assert_eq!(store.list_sessions(), vec!["purr_new", "purr_old"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_replace_and_delete_cascade() {
        let (store, dir) = test_store();
        store.append_message("purr_rep", &user_msg("a")).unwrap();
        store.append_message("purr_rep", &user_msg("b")).unwrap();
        store
            .replace_messages("purr_rep", &[user_msg("only")])
            .unwrap();
        let loaded = store.load_messages("purr_rep").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "only");
        store.delete_session("purr_rep").unwrap();
        assert!(!store.session_exists("purr_rep").unwrap());
        assert!(store.load_messages("purr_rep").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
