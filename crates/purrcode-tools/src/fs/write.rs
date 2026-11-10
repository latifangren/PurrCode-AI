// SPDX-License-Identifier: GPL-3.0-or-later

use super::read::{compute_fnv1a_hex, resolve_confined_path};
use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use similar::TextDiff;

pub struct WriteTool;

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write".to_string(),
            description: "Create or overwrite a complete UTF-8 file atomically".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to file" },
                    "content": { "type": "string", "description": "Complete file content to write" }
                },
                "required": ["path", "content"]
            }),
            tier: ApprovalTier::Write,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: WriteArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "write".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let full_path = resolve_confined_path(&args.path, &ctx.workspace_root, "write")?;

        let existing_content = if full_path.exists() {
            tokio::fs::read_to_string(&full_path).await.ok()
        } else {
            None
        };

        // Generate diff preview
        let diff_preview = if let Some(ref old) = existing_content {
            let diff = TextDiff::from_lines(old, &args.content);
            diff.unified_diff()
                .header(&args.path, &args.path)
                .to_string()
        } else {
            format!(
                "+++ new file: {}\n({} lines)",
                args.path,
                args.content.lines().count()
            )
        };

        if ctx.is_dry_run {
            return Ok(ToolResult {
                output: format!(
                    "Dry-run: would write {} bytes to '{}'",
                    args.content.len(),
                    args.path
                ),
                diff_preview: Some(diff_preview),
                metadata: None,
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| PurrError::Tool {
                    tool: "write".to_string(),
                    message: format!("Failed to create parent directory: {e}"),
                })?;
        }

        // Atomic write via temporary file
        let tmp_path = full_path.with_extension(format!("tmp_{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp_path, args.content.as_bytes())
            .await
            .map_err(|e| PurrError::Tool {
                tool: "write".to_string(),
                message: format!("Failed to write temporary file: {e}"),
            })?;

        tokio::fs::rename(&tmp_path, &full_path)
            .await
            .map_err(|e| PurrError::Tool {
                tool: "write".to_string(),
                message: format!("Failed to atomically rename file to '{}': {e}", args.path),
            })?;

        let content_hash = compute_fnv1a_hex(&args.content);

        Ok(ToolResult {
            output: format!(
                "Successfully wrote {} bytes to '{}'",
                args.content.len(),
                args.path
            ),
            diff_preview: Some(diff_preview),
            metadata: Some(serde_json::json!({
                "path": args.path,
                "bytes_written": args.content.len(),
                "lines": args.content.lines().count(),
                "content_hash": content_hash
            })),
        })
    }
}
