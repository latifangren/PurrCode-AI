// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use regex::RegexBuilder;
use serde::Deserialize;
use std::path::Path;
use walkdir::WalkDir;

pub struct GrepTool;

#[derive(Deserialize)]
struct GrepArgs {
    pattern: String,
    path: Option<String>,
    case_sensitive: Option<bool>,
    context: Option<usize>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for GrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description:
                "Search text patterns in workspace files with optional surrounding context lines"
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search regex or literal pattern" },
                    "path": { "type": "string", "description": "Subdirectory or file to search (relative to workspace)" },
                    "case_sensitive": { "type": "boolean", "description": "Case-sensitive matching (default: false)" },
                    "context": { "type": "integer", "description": "Surrounding context lines before and after match (default: 0, max: 10)" },
                    "limit": { "type": "integer", "description": "Maximum number of matches to return (default: 100)" }
                },
                "required": ["pattern"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: GrepArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "grep".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let search_root = match args.path {
            Some(ref p) => crate::fs::read::resolve_confined_path(p, &ctx.workspace_root, "grep")?,
            None => ctx.workspace_root.clone(),
        };

        let case_insensitive = !args.case_sensitive.unwrap_or(false);
        let regex = RegexBuilder::new(&args.pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| PurrError::Tool {
                tool: "grep".to_string(),
                message: format!("Invalid regex pattern '{}': {e}", args.pattern),
            })?;

        let max_matches = args.limit.unwrap_or(100).min(500);
        let context_radius = args.context.unwrap_or(0).min(10);
        let workspace_root = ctx.workspace_root.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            let mut total_matches = 0;

            for entry in WalkDir::new(&search_root)
                .into_iter()
                .filter_entry(|e| !is_ignored(e.path()))
                .flatten()
            {
                if entry.file_type().is_file() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        let rel_path = entry
                            .path()
                            .strip_prefix(&workspace_root)
                            .unwrap_or(entry.path())
                            .to_string_lossy();

                        let lines: Vec<&str> = content.lines().collect();
                        let matched_line_indices: Vec<usize> = lines
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, line)| {
                                if regex.is_match(line) {
                                    Some(idx)
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if matched_line_indices.is_empty() {
                            continue;
                        }

                        if context_radius == 0 {
                            for &idx in &matched_line_indices {
                                results.push(format!(
                                    "{}:{}:{}",
                                    rel_path,
                                    idx + 1,
                                    lines[idx].trim()
                                ));
                                total_matches += 1;
                                if total_matches >= max_matches {
                                    return (results, true);
                                }
                            }
                        } else {
                            // Merge overlapping intervals around matches
                            let mut intervals: Vec<(usize, usize)> = Vec::new();
                            for &idx in &matched_line_indices {
                                let start = idx.saturating_sub(context_radius);
                                let end = (idx + context_radius).min(lines.len().saturating_sub(1));

                                if let Some(last) = intervals.last_mut() {
                                    if start <= last.1 + 1 {
                                        last.1 = last.1.max(end);
                                    } else {
                                        intervals.push((start, end));
                                    }
                                } else {
                                    intervals.push((start, end));
                                }
                            }

                            for (int_idx, (start, end)) in intervals.iter().enumerate() {
                                if int_idx > 0 {
                                    results.push("--".to_string());
                                }
                                for (l, line_content) in
                                    lines.iter().enumerate().take(*end + 1).skip(*start)
                                {
                                    let is_match = matched_line_indices.contains(&l);
                                    let sep = if is_match { ':' } else { '-' };
                                    results.push(format!(
                                        "{}{sep}{}{sep}{}",
                                        rel_path,
                                        l + 1,
                                        line_content
                                    ));
                                    if is_match {
                                        total_matches += 1;
                                    }
                                }
                                if total_matches >= max_matches {
                                    return (results, true);
                                }
                            }
                        }
                    }
                }
            }
            (results, false)
        })
        .await
        .map_err(|e| PurrError::Tool {
            tool: "grep".to_string(),
            message: format!("Search task failed: {e}"),
        })?;

        let (items, hit_limit) = matches;
        let output = if items.is_empty() {
            "No matches found.".to_string()
        } else {
            let mut res = items.join("\n");
            if hit_limit {
                res.push_str(&format!("\n(stopped at limit of {max_matches} matches)"));
            }
            res
        };

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: None,
        })
    }
}

fn is_ignored(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(
        name,
        ".git" | "target" | "node_modules" | "references" | ".tmp" | "dist" | ".purrcode"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_grep_basic_and_context_lines() {
        let temp_dir = std::env::temp_dir().join(format!("purr_grep_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        let file_path = temp_dir.join("sample.txt");
        let content = "line 1\nline 2: TARGET\nline 3\nline 4\nline 5: TARGET\nline 6\n";
        tokio::fs::write(&file_path, content).await.unwrap();

        let tool = GrepTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "whisker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
            is_cron: false,
            sandbox: None,
        };

        // 1. Basic search without context
        let res_no_ctx = tool
            .execute(
                serde_json::json!({
                    "pattern": "TARGET"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res_no_ctx.output.contains("sample.txt:2:line 2: TARGET"));
        assert!(res_no_ctx.output.contains("sample.txt:5:line 5: TARGET"));
        assert!(!res_no_ctx.output.contains("sample.txt-1-line 1"));

        // 2. Search with context=1
        let res_ctx = tool
            .execute(
                serde_json::json!({
                    "pattern": "TARGET",
                    "context": 1
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res_ctx.output.contains("sample.txt:2:line 2: TARGET"));
        assert!(res_ctx.output.contains("sample.txt-1-line 1"));
        assert!(res_ctx.output.contains("sample.txt-3-line 3"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
