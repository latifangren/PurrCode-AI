// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime harness guards for `AgentLoop`:
//! - Read-before-edit tracking to block blind overwrites of existing unread files
//! - Repeated identical-failure detection with `<system-reminder>` recovery guidance
//! - Read-only parallel batch eligibility classification
//! - Workspace path confinement inspection for approval routing

use purrcode_api::provider::{ChatMessage, MessageRole};
use purrcode_api::tool::ApprovalTier;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Tracks files read or created during a session so existing files cannot be
/// blindly overwritten by `edit` or `write` without first inspecting them.
#[derive(Debug, Default)]
pub struct SessionFileGuard {
    known_paths: Mutex<HashSet<PathBuf>>,
}

impl SessionFileGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds the guard from prior conversation history so multi-turn WebUI/CLI
    /// runs remember files read or written in earlier turns.
    pub fn seed_from_history(&self, workspace_root: &Path, messages: &[ChatMessage]) {
        for msg in messages {
            if msg.role != MessageRole::Assistant {
                continue;
            }
            let Some(ref calls) = msg.tool_calls else {
                continue;
            };
            for call in calls {
                if matches!(call.name.as_str(), "read" | "write" | "edit") {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&call.arguments) {
                        if let Some(raw_path) = val.get("path").and_then(|v| v.as_str()) {
                            let trimmed = raw_path.trim();
                            if !trimmed.is_empty() {
                                self.record_read(workspace_root, trimmed);
                            }
                        }
                    }
                }
            }
        }
    }

    fn collect_path_keys(workspace_root: &Path, raw_path: &str) -> Vec<PathBuf> {
        let candidate = Path::new(raw_path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_root.join(candidate)
        };
        let mut keys = Vec::with_capacity(2);
        let lexical = normalize_lexical(&joined);
        keys.push(lexical.clone());
        if let Ok(canon) = std::fs::canonicalize(&joined) {
            if canon != lexical {
                keys.push(canon);
            }
        }
        keys
    }

    pub fn record_read(&self, workspace_root: &Path, raw_path: &str) {
        let keys = Self::collect_path_keys(workspace_root, raw_path);
        if let Ok(mut guard) = self.known_paths.lock() {
            for k in keys {
                guard.insert(k);
            }
        }
    }

    pub fn record_write(&self, workspace_root: &Path, raw_path: &str) {
        self.record_read(workspace_root, raw_path);
    }

    /// Returns `Some(guidance)` when `tool_name` (`edit` or `write`) targets an
    /// existing regular file that has not yet been read in this session.
    pub fn check_mutation_allowed(
        &self,
        workspace_root: &Path,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Option<String> {
        if tool_name != "edit" && tool_name != "write" {
            return None;
        }
        let raw_path = args.get("path").and_then(|v| v.as_str())?.trim();
        if raw_path.is_empty() {
            return None;
        }

        let candidate = Path::new(raw_path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace_root.join(candidate)
        };

        // Allow creating brand-new files via `write`, and let `edit` return its
        // own missing-file error for nonexistent targets.
        if !joined.is_file() {
            return None;
        }

        let keys = Self::collect_path_keys(workspace_root, raw_path);
        let already_known = self
            .known_paths
            .lock()
            .map(|guard| keys.iter().any(|k| guard.contains(k)))
            .unwrap_or(true);

        if already_known {
            None
        } else {
            Some(format!(
                "Read-before-edit guard: '{}' already exists and has not been read in this session. Call 'read' on '{}' first to inspect its current content and line numbers before modifying it.",
                raw_path, raw_path
            ))
        }
    }
}

/// Detects consecutive identical failing tool calls so the agent receives a
/// `<system-reminder>` to change strategy rather than looping blindly.
#[derive(Debug, Default)]
pub struct FailureLoopTracker {
    last_call: Option<(String, String)>,
    streak: usize,
}

impl FailureLoopTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &mut self,
        tool_name: &str,
        canonical_args: &str,
        is_success: bool,
    ) -> Option<String> {
        if is_success {
            self.last_call = None;
            self.streak = 0;
            return None;
        }
        let key = (tool_name.to_string(), canonical_args.to_string());
        if self.last_call.as_ref() == Some(&key) {
            self.streak += 1;
        } else {
            self.last_call = Some(key);
            self.streak = 1;
        }

        if self.streak >= 2 {
            Some(
                "\n\n<system-reminder>\nYou have repeated this exact failing tool call multiple times. Stop retrying the identical call verbatim. Inspect the error output and target file/state, then change your approach or ask the user if blocked.\n</system-reminder>"
                    .to_string(),
            )
        } else {
            None
        }
    }
}

/// Returns `true` for pure read-only / non-interactive tools that are safe to
/// execute concurrently when multiple appear together in a single model step.
pub fn is_parallel_read_eligible(tool_name: &str, tier: ApprovalTier) -> bool {
    tier == ApprovalTier::Read && tool_name != "ask" && tool_name != "todo"
}

pub(crate) fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

pub(crate) fn is_args_outside_workspace(args: &serde_json::Value, workspace_root: &Path) -> bool {
    let candidate_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("workdir").and_then(|v| v.as_str()));

    let Some(raw) = candidate_str else {
        return false;
    };

    let abs_root = if workspace_root.is_absolute() {
        workspace_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace_root)
    };
    let canon_root =
        std::fs::canonicalize(&abs_root).unwrap_or_else(|_| normalize_lexical(&abs_root));
    let p = Path::new(raw);
    let raw_candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        canon_root.join(p)
    };

    let normalized = normalize_lexical(&raw_candidate);
    if !normalized.starts_with(&canon_root) {
        return true;
    }

    // Resolve symlinks on existing target or parent directory to prevent symlink escape
    if normalized.exists() {
        if let Ok(canon_target) = std::fs::canonicalize(&normalized) {
            if !canon_target.starts_with(&canon_root) {
                return true;
            }
        }
    } else if let Some(parent) = normalized.parent() {
        if parent.exists() {
            if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                if !canon_parent.starts_with(&canon_root) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::ToolCallRequest;

    #[test]
    fn test_read_before_edit_guard_blocks_unread_existing_file_then_allows_after_read() {
        let dir = std::env::temp_dir().join(format!("purr_rbe_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let guard = SessionFileGuard::new();
        let args = serde_json::json!({ "path": "sample.rs", "find": "main", "replace": "run" });

        assert!(guard.check_mutation_allowed(&dir, "edit", &args).is_some());

        let new_args = serde_json::json!({ "path": "brand_new.rs", "content": "// new\n" });
        assert!(guard
            .check_mutation_allowed(&dir, "write", &new_args)
            .is_none());

        guard.record_read(&dir, "sample.rs");
        assert!(guard.check_mutation_allowed(&dir, "edit", &args).is_none());
        assert!(guard.check_mutation_allowed(&dir, "write", &args).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_seed_from_history_records_prior_reads() {
        let dir = std::env::temp_dir().join(format!("purr_seed_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known.rs");
        std::fs::write(&file, "pub fn ok() {}\n").unwrap();

        let guard = SessionFileGuard::new();
        let history = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls: Some(vec![ToolCallRequest {
                id: "1".to_string(),
                name: "read".to_string(),
                arguments: "{\"path\":\"known.rs\"}".to_string(),
            }]),
            tool_call_id: None,
        }];
        guard.seed_from_history(&dir, &history);

        let args = serde_json::json!({ "path": "known.rs", "find": "ok", "replace": "yes" });
        assert!(guard.check_mutation_allowed(&dir, "edit", &args).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_failure_loop_tracker_injects_reminder_after_two_identical_failures() {
        let mut tracker = FailureLoopTracker::new();
        assert!(tracker.record("edit", "{\"path\":\"a\"}", false).is_none());
        let second = tracker.record("edit", "{\"path\":\"a\"}", false);
        assert!(second.is_some());
        assert!(second.unwrap().contains("<system-reminder>"));

        assert!(tracker.record("edit", "{\"path\":\"a\"}", true).is_none());
        assert!(tracker.record("edit", "{\"path\":\"a\"}", false).is_none());
    }
}
