// SPDX-License-Identifier: GPL-3.0-or-later

use crate::fs::read::resolve_confined_path;
use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::time::Duration;

pub struct BashTool;

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    workdir: Option<String>,
    timeout_ms: Option<u64>,
    max_lines: Option<usize>,
}

#[async_trait]
impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: "Execute a shell command with timeout and smart output truncation to guard context window"
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "workdir": { "type": "string", "description": "Optional working directory relative to workspace root" },
                    "timeout_ms": { "type": "integer", "description": "Execution timeout in milliseconds (default: 30000)" },
                    "max_lines": { "type": "integer", "description": "Maximum output lines before smart head/tail truncation (default: 200)" }
                },
                "required": ["command"]
            }),
            tier: ApprovalTier::Exec,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: BashArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "bash".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        if ctx.is_dry_run {
            return Ok(ToolResult {
                output: format!("Dry-run: would execute command `{}`", args.command),
                diff_preview: Some(format!("$ {}", args.command)),
                metadata: None,
            });
        }

        let cwd = match args.workdir {
            Some(ref dir) => resolve_confined_path(dir, &ctx.workspace_root, "bash")?,
            None => ctx.workspace_root.clone(),
        };

        let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(30_000).max(1000));

        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("powershell.exe");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &args.command]);
            c
        };

        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", &args.command]);
            c
        };

        cmd.current_dir(cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);

        let child = cmd.spawn().map_err(|e| PurrError::Tool {
            tool: "bash".to_string(),
            message: format!("Failed to spawn shell process: {e}"),
        })?;

        let output_res = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match output_res {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&format!("[stderr]:\n{}", stderr));
                }

                if combined.is_empty() {
                    combined = format!("(command exited with code {})", exit_code);
                }

                let max_lines_limit = args.max_lines.unwrap_or(200);
                let (truncated_output, was_truncated) =
                    smart_truncate_output(&combined, max_lines_limit, 48_000);

                Ok(ToolResult {
                    output: truncated_output,
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "exit_code": exit_code,
                        "timed_out": false,
                        "truncated": was_truncated
                    })),
                })
            }
            Ok(Err(e)) => Err(PurrError::Tool {
                tool: "bash".to_string(),
                message: format!("Command execution error: {e}"),
            }),
            Err(_) => Ok(ToolResult {
                output: format!("Command timed out after {} ms", timeout.as_millis()),
                diff_preview: None,
                metadata: Some(serde_json::json!({
                    "exit_code": -1,
                    "timed_out": true
                })),
            }),
        }
    }
}

pub fn smart_truncate_output(text: &str, max_lines: usize, max_bytes: usize) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines && text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    let head_count = (max_lines * 6) / 10;
    let tail_count = max_lines.saturating_sub(head_count);

    if lines.len() > max_lines && head_count + tail_count < lines.len() {
        let omitted = lines.len() - (head_count + tail_count);
        let head_part = lines[..head_count].join("\n");
        let tail_part = lines[lines.len() - tail_count..].join("\n");
        let mut truncated = format!(
            "{head_part}\n\n[... output truncated: {omitted} lines omitted to preserve context window ...]\n\n{tail_part}"
        );
        if truncated.len() > max_bytes {
            truncated = format!(
                "{}\n\n[... output truncated at byte limit ...]",
                safe_truncate_str(&truncated, max_bytes)
            );
        }
        (truncated, true)
    } else if text.len() > max_bytes {
        let truncated = format!(
            "{}\n\n[... output truncated: exceeded {max_bytes} bytes ...]",
            safe_truncate_str(text, max_bytes)
        );
        (truncated, true)
    } else {
        (text.to_string(), false)
    }
}

fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        s
    } else {
        let mut idx = max_bytes;
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
        &s[..idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_truncate_output() {
        let mut lines = Vec::new();
        for i in 1..=100 {
            lines.push(format!("line {i}"));
        }
        let full = lines.join("\n");

        // Truncate to 10 lines
        let (res, truncated) = smart_truncate_output(&full, 10, 10_000);
        assert!(truncated);
        assert!(res.contains("line 1"));
        assert!(res.contains("line 6"));
        assert!(
            res.contains("[... output truncated: 90 lines omitted to preserve context window ...]")
        );
        assert!(res.contains("line 100"));
    }
}
