use super::read::resolve_path;
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
}

#[async_trait]
impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit".to_string(),
            description: "Replace one unique exact text match in a file with unified diff preview".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to file" },
                    "find": { "type": "string", "description": "Exact text to replace (must match uniquely in file)" },
                    "replace": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "find", "replace"]
            }),
            tier: ApprovalTier::Write,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: EditArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        let full_path = resolve_path(&args.path, &ctx.workspace_root);
        if !full_path.exists() {
            return Err(PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("File not found: '{}'", args.path),
            });
        }

        let original = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("Failed to read file '{}': {e}", args.path),
            })?;

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
                metadata: None,
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
                "inserted_bytes": args.replace.len()
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_unique_match() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("purr_edit_test.txt");
        tokio::fs::write(&file_path, "hello world\nfoo bar\nbaz qux\n").await.unwrap();

        let tool = EditTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "tinker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
        };

        let res = tool
            .execute(
                serde_json::json!({
                    "path": "purr_edit_test.txt",
                    "find": "foo bar",
                    "replace": "foo modified"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res.output.contains("Successfully edited"));
        assert!(res.diff_preview.unwrap().contains("-foo bar"));

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("foo modified"));

        let _ = tokio::fs::remove_file(file_path).await;
    }
}
