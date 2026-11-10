// SPDX-License-Identifier: GPL-3.0-or-later

use super::read::{compute_fnv1a_hex, resolve_confined_path};
use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use similar::TextDiff;

pub struct EditTool;

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    find: String,
    replace: String,
    expected_hash: Option<String>,
}

#[async_trait]
impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit".to_string(),
            description: "Replace one unique exact text match in a file with hash-anchoring and unified diff preview".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to file" },
                    "find": { "type": "string", "description": "Exact text to replace (must match uniquely in file)" },
                    "replace": { "type": "string", "description": "Replacement text" },
                    "expected_hash": { "type": "string", "description": "Optional FNV-1a content_hash from prior read/edit to verify file has not changed concurrently" }
                },
                "required": ["path", "find", "replace"]
            }),
            tier: ApprovalTier::Write,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: EditArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "edit".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let full_path = resolve_confined_path(&args.path, &ctx.workspace_root, "edit")?;
        if !full_path.exists() {
            return Err(PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("File not found: '{}'", args.path),
            });
        }

        let original =
            tokio::fs::read_to_string(&full_path)
                .await
                .map_err(|e| PurrError::Tool {
                    tool: "edit".to_string(),
                    message: format!("Failed to read file '{}': {e}", args.path),
                })?;

        let actual_hash = compute_fnv1a_hex(&original);
        if let Some(ref exp_hash) = args.expected_hash {
            if !exp_hash.trim().is_empty() && !exp_hash.eq_ignore_ascii_case(&actual_hash) {
                return Err(PurrError::Tool {
                    tool: "edit".to_string(),
                    message: format!(
                        "Hash anchor mismatch for '{}': expected '{}', found '{}'. Re-read the file before editing.",
                        args.path, exp_hash, actual_hash
                    ),
                });
            }
        }

        let matches: Vec<_> = original.match_indices(&args.find).collect();
        if matches.is_empty() {
            return Err(PurrError::Tool {
                tool: "edit".to_string(),
                message: format!(
                    "Exact text match not found in '{}'. Please check indentation and surrounding context.",
                    args.path
                ),
            });
        }

        if matches.len() > 1 {
            return Err(PurrError::Tool {
                tool: "edit".to_string(),
                message: format!(
                    "Ambiguous edit in '{}': found {} matches. Include more surrounding lines in 'find' to match uniquely.",
                    args.path,
                    matches.len()
                ),
            });
        }

        let match_idx = matches[0].0;
        let mut new_content = String::with_capacity(original.len() + args.replace.len());
        new_content.push_str(&original[..match_idx]);
        new_content.push_str(&args.replace);
        new_content.push_str(&original[match_idx + args.find.len()..]);

        let new_hash = compute_fnv1a_hex(&new_content);
        let diff = TextDiff::from_lines(&original, &new_content);
        let diff_preview = diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{}", args.path), &format!("b/{}", args.path))
            .to_string();

        if ctx.is_dry_run {
            return Ok(ToolResult {
                output: format!("Dry-run: edit preview for '{}'", args.path),
                diff_preview: Some(diff_preview),
                metadata: Some(serde_json::json!({
                    "before_hash": actual_hash,
                    "after_hash": new_hash
                })),
            });
        }

        // Atomic write
        let tmp_path = full_path.with_extension(format!("tmp_{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp_path, new_content.as_bytes())
            .await
            .map_err(|e| PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("Failed to write temporary file: {e}"),
            })?;

        tokio::fs::rename(&tmp_path, &full_path)
            .await
            .map_err(|e| PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("Failed to atomically rename file to '{}': {e}", args.path),
            })?;

        Ok(ToolResult {
            output: format!("Successfully edited '{}'", args.path),
            diff_preview: Some(diff_preview),
            metadata: Some(serde_json::json!({
                "path": args.path,
                "replaced_bytes": args.find.len(),
                "inserted_bytes": args.replace.len(),
                "before_hash": actual_hash,
                "content_hash": new_hash
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_unique_match_and_hash_anchor() {
        let temp_dir = std::env::temp_dir().join(format!("purr_edit_ws_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("purr_edit_test.txt");
        let initial = "hello world\nfoo bar\nbaz qux\n";
        tokio::fs::write(&file_path, initial).await.unwrap();
        let init_hash = compute_fnv1a_hex(initial);

        let tool = EditTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "tinker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
        };

        // 1. Mismatched hash must fail
        let bad_res = tool
            .execute(
                serde_json::json!({
                    "path": "purr_edit_test.txt",
                    "find": "foo bar",
                    "replace": "foo modified",
                    "expected_hash": "0000000000000000"
                }),
                &ctx,
            )
            .await;
        assert!(bad_res.is_err());

        // 2. Correct hash succeeds
        let res = tool
            .execute(
                serde_json::json!({
                    "path": "purr_edit_test.txt",
                    "find": "foo bar",
                    "replace": "foo modified",
                    "expected_hash": init_hash
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res.output.contains("Successfully edited"));
        assert!(res.diff_preview.unwrap().contains("-foo bar"));

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("foo modified"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
