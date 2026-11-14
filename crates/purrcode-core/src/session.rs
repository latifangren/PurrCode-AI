// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{ChatMessage, CompletionRequest, MessageRole, ModelProvider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    pub fn replace_messages(&self, session_id: &str, messages: &[ChatMessage]) -> Result<()> {
        let dir = self.sessions_dir();
        std::fs::create_dir_all(&dir).map_err(PurrError::Io)?;
        let path = self.session_jsonl_path(session_id);
        let tmp = dir.join(format!("{}.jsonl.tmp", session_id));
        let mut buf = String::new();
        let now = chrono::Utc::now().to_rfc3339();
        for message in messages {
            let entry = SessionTurnEntry {
                version: "session/v1".to_string(),
                session_id: session_id.to_string(),
                timestamp: now.clone(),
                message: message.clone(),
            };
            buf.push_str(&serde_json::to_string(&entry)?);
            buf.push('\n');
        }
        std::fs::write(&tmp, buf.as_bytes()).map_err(PurrError::Io)?;
        std::fs::rename(&tmp, &path).map_err(PurrError::Io)?;
        Ok(())
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
                "### Compacted Session Summary ({} earlier messages preserved in session JSONL)\n{}",
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
                        "### Compacted Session Summary ({} earlier messages preserved in session JSONL)\n{}\n\n<system-reminder>\n[Old tool outputs cleared during compaction] Never invent omitted file contents or command outputs; re-read files or re-run checks if exact details are needed.\n</system-reminder>",
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
}
