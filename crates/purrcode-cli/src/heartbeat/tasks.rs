// SPDX-License-Identifier: GPL-3.0-or-later

//! `HEARTBEAT.md` task file (v0.3 F22, ADR-014 item 6).
//!
//! The heartbeat's work list is a USER-written file, one check per
//! line — the shared pattern from PicoClaw and ZeroClaw. The file is
//! read from the workspace first (`.purrcode/HEARTBEAT.md`), then the
//! user's global one (`~/.purrcode/HEARTBEAT.md`). When neither exists
//! the tick creates the global template and ends silently (PicoClaw
//! rule: a missing file means "not configured yet", not an error —
//! and an absent file costs no LLM call at all).

use purrcode_api::error::{PurrError, Result};
use std::path::{Path, PathBuf};

/// One recurring check from `HEARTBEAT.md`. `index` is 1-based — the
/// number the Phase-1 decision refers to as `run: 1, 3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatTask {
    pub index: usize,
    pub text: String,
}

/// Template written when no task file exists anywhere. The example
/// lines are comments (`#`), so the file parses to zero tasks until
/// the user writes real checks.
pub const HEARTBEAT_TEMPLATE: &str = "\
# HEARTBEAT.md — Recurring checks for your heartbeat worker.\n\
# Add one check per line, and the next tick picks it up.\n\
# Lines starting with `#` and blank lines are ignored.\n\
# Example (delete these and write your own):\n\
# - Check whether the nightly build on the CI dashboard passed.\n\
# - Summarize unread emails from the last hour.\n\
";

/// The marker line from PicoClaw's template: when it is present, only
/// the lines BELOW it are tasks (everything above is header).
pub const TASK_MARKER: &str = "Add your heartbeat tasks below this line:";

/// File name inside `.purrcode/` (workspace) and `~/.purrcode/`.
pub const HEARTBEAT_FILE_NAME: &str = "HEARTBEAT.md";

/// Parse task texts from file content. Pure function (unit-tested):
/// skip blank lines and `#` comments; when the PicoClaw marker is
/// present, only lines below it count; strip one leading list marker
/// (`-`, `*`, `+`, `1. `, `1) `, `[ ] `/`[x] `) per line.
pub fn parse_tasks(text: &str) -> Vec<String> {
    let mut below_marker = !text.contains(TASK_MARKER);
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if !below_marker {
            if line.contains(TASK_MARKER) {
                below_marker = true;
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut task = line;
        // One leading list marker.
        for marker in ["- ", "* ", "+ ", "[ ] ", "[x] ", "[X] "] {
            if let Some(rest) = task.strip_prefix(marker) {
                task = rest.trim_start();
                break;
            }
        }
        // "1. " / "12) " numbered marker.
        if let Some((num, rest)) = task.split_once(['.', ')']) {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) && rest.starts_with(' ') {
                task = rest.trim_start();
            }
        }
        let task = task.trim();
        if !task.is_empty() {
            out.push(task.to_string());
        }
    }
    out
}

/// Where the tasks came from, if anywhere.
pub struct TaskLoad {
    pub tasks: Vec<HeartbeatTask>,
    pub source: Option<PathBuf>,
    /// True when this call created the global template (the file did
    /// not exist anywhere before).
    pub created_template: bool,
}

/// Find the task file: workspace `.purrcode/HEARTBEAT.md` wins over
/// the global `~/.purrcode/HEARTBEAT.md`.
pub fn find_task_file(workspace_root: &Path, purrcode_dir: &Path) -> Option<PathBuf> {
    let project = workspace_root.join(".purrcode").join(HEARTBEAT_FILE_NAME);
    if project.exists() {
        return Some(project);
    }
    let global = purrcode_dir.join(HEARTBEAT_FILE_NAME);
    if global.exists() {
        return Some(global);
    }
    None
}

/// Load the tasks. When no file exists anywhere, create the global
/// template (under `purrcode_dir`, creating the directory if needed)
/// and return zero tasks — the tick ends silently after that.
pub fn load_tasks(workspace_root: &Path, purrcode_dir: &Path) -> Result<TaskLoad> {
    match find_task_file(workspace_root, purrcode_dir) {
        Some(path) => {
            let raw = std::fs::read_to_string(&path).map_err(|e| {
                PurrError::Session(format!(
                    "Failed to read heartbeat task file {}: {e}",
                    path.display()
                ))
            })?;
            let tasks = parse_tasks(&raw)
                .into_iter()
                .enumerate()
                .map(|(i, text)| HeartbeatTask { index: i + 1, text })
                .collect();
            Ok(TaskLoad {
                tasks,
                source: Some(path),
                created_template: false,
            })
        }
        None => {
            let global = purrcode_dir.join(HEARTBEAT_FILE_NAME);
            if let Some(parent) = global.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    PurrError::Session(format!("Failed to create {}: {e}", parent.display()))
                })?;
            }
            std::fs::write(&global, HEARTBEAT_TEMPLATE).map_err(|e| {
                PurrError::Session(format!(
                    "Failed to write heartbeat template {}: {e}",
                    global.display()
                ))
            })?;
            Ok(TaskLoad {
                tasks: Vec::new(),
                source: Some(global),
                created_template: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tasks_plain_lines() {
        let tasks = parse_tasks("Check the build.\nSummarize email.\n");
        assert_eq!(tasks, vec!["Check the build.", "Summarize email."]);
    }

    #[test]
    fn test_parse_tasks_comments_blanks_and_markers() {
        let text = "# header\n\n- Check the build.\n* Summarize email.\n1. Watch the repo.\n12) Ping the server.\n[ ] Review the queue.\n";
        let tasks = parse_tasks(text);
        assert_eq!(
            tasks,
            vec![
                "Check the build.",
                "Summarize email.",
                "Watch the repo.",
                "Ping the server.",
                "Review the queue."
            ]
        );
    }

    #[test]
    fn test_parse_tasks_marker_gates_the_header() {
        let text =
            "Check the build.\nAdd your heartbeat tasks below this line:\n- Watch the repo.\n";
        // The line above the marker is header, not a task.
        assert_eq!(parse_tasks(text), vec!["Watch the repo."]);
    }

    #[test]
    fn test_parse_tasks_empty_and_comment_only() {
        assert!(parse_tasks("").is_empty());
        assert!(parse_tasks("# nothing here\n\n# still nothing\n").is_empty());
    }
}
