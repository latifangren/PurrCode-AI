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

        let (match_start, match_end) =
            find_match_range(&original, &args.find).map_err(|msg| PurrError::Tool {
                tool: "edit".to_string(),
                message: format!("{msg} in '{}'", args.path),
            })?;

        let mut new_content = String::with_capacity(original.len() + args.replace.len());
        new_content.push_str(&original[..match_start]);
        new_content.push_str(&args.replace);
        new_content.push_str(&original[match_end..]);

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

pub fn find_match_range(original: &str, find: &str) -> std::result::Result<(usize, usize), String> {
    if find.is_empty() {
        return Err("The 'find' string must not be empty".to_string());
    }

    // 1. Exact match
    let exact_matches: Vec<_> = original.match_indices(find).collect();
    if exact_matches.len() == 1 {
        let start = exact_matches[0].0;
        return Ok((start, start + find.len()));
    }
    if exact_matches.len() > 1 {
        return Err(format!(
            "Ambiguous edit: found {} exact matches. Include more surrounding lines in 'find' to match uniquely.",
            exact_matches.len()
        ));
    }

    // 2. Whitespace-tolerant line-based matching fallback (handles CRLF / indentation differences from LLM)
    let find_lines: Vec<&str> = find.lines().collect();
    if find_lines.is_empty() {
        return Err(
            "Exact text match not found. Please check indentation and surrounding context."
                .to_string(),
        );
    }

    let mut orig_lines: Vec<(&str, usize, usize)> = Vec::new();
    let mut cursor = 0;
    for line in original.split_inclusive('\n') {
        let line_len = line.len();
        orig_lines.push((line, cursor, cursor + line_len));
        cursor += line_len;
    }

    if find_lines.len() > orig_lines.len() {
        return Err(
            "Exact text match not found. Please check indentation and surrounding context."
                .to_string(),
        );
    }

    // First try: trim_end match (handles CRLF / trailing whitespace differences)
    let mut candidate_ranges = Vec::new();
    for i in 0..=orig_lines.len() - find_lines.len() {
        let mut matched = true;
        for (j, find_line) in find_lines.iter().enumerate() {
            let orig_line_content = orig_lines[i + j]
                .0
                .trim_end_matches(['\r', '\n', ' ', '\t']);
            let find_line_content = find_line.trim_end_matches(['\r', '\n', ' ', '\t']);
            if orig_line_content != find_line_content {
                matched = false;
                break;
            }
        }
        if matched {
            let start = orig_lines[i].1;
            let end = orig_lines[i + find_lines.len() - 1].2;
            candidate_ranges.push((start, end));
        }
    }

    if candidate_ranges.len() == 1 {
        return Ok(candidate_ranges[0]);
    }
    if candidate_ranges.len() > 1 {
        return Err(format!(
            "Ambiguous edit: found {} line-level matches. Include more surrounding lines in 'find' to match uniquely.",
            candidate_ranges.len()
        ));
    }

    // Second try: full trim match (handles indentation differences)
    let mut trim_ranges = Vec::new();
    for i in 0..=orig_lines.len() - find_lines.len() {
        let mut matched = true;
        for (j, find_line) in find_lines.iter().enumerate() {
            let orig_trimmed = orig_lines[i + j].0.trim();
            let find_trimmed = find_line.trim();
            if orig_trimmed != find_trimmed {
                matched = false;
                break;
            }
        }
        if matched {
            let start = orig_lines[i].1;
            let end = orig_lines[i + find_lines.len() - 1].2;
            trim_ranges.push((start, end));
        }
    }

    if trim_ranges.len() == 1 {
        return Ok(trim_ranges[0]);
    }
    if trim_ranges.len() > 1 {
        return Err(format!(
            "Ambiguous edit: found {} indentation-tolerant matches. Include more surrounding lines in 'find' to match uniquely.",
            trim_ranges.len()
        ));
    }

    Err("Exact text match not found. Please check indentation and surrounding context.".to_string())
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

    #[tokio::test]
    async fn test_edit_fuzzy_whitespace_and_crlf_fallback() {
        let temp_dir =
            std::env::temp_dir().join(format!("purr_edit_fuzzy_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("fuzzy.rs");

        // Original file has 4 spaces and CRLF endings
        let initial = "fn example() {\r\n    let x = 1;\r\n    let y = 2;\r\n}\r\n";
        tokio::fs::write(&file_path, initial).await.unwrap();

        let tool = EditTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "tinker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
        };

        // LLM provides 2 spaces indentation and LF endings
        let res = tool
            .execute(
                serde_json::json!({
                    "path": "fuzzy.rs",
                    "find": "fn example() {\n  let x = 1;\n  let y = 2;\n}",
                    "replace": "fn example() {\n    let x = 100;\n    let y = 200;\n}"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res.output.contains("Successfully edited"));
        let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(updated.contains("let x = 100;"));
        assert!(updated.contains("let y = 200;"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[test]
    fn test_find_match_range_rejects_ambiguity() {
        let text = "bar\nbar\n";
        let res = find_match_range(text, "bar\n");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Ambiguous edit"));
    }
}
