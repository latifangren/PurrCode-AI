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
}

#[async_trait]
impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: "Execute a shell command with timeout and working directory support".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "workdir": { "type": "string", "description": "Optional working directory relative to workspace root" },
                    "timeout_ms": { "type": "integer", "description": "Execution timeout in milliseconds (default: 30000)" }
                },
                "required": ["command"]
            }),
            tier: ApprovalTier::Exec,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: BashArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "bash".to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        let cwd = match args.workdir {
            Some(ref dir) => ctx.workspace_root.join(dir),
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

                Ok(ToolResult {
                    output: combined,
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "exit_code": exit_code,
                        "timed_out": false
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
