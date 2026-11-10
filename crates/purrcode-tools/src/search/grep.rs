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
}

#[async_trait]
impl Tool for GrepTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search text patterns in workspace files".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search regex or literal pattern" },
                    "path": { "type": "string", "description": "Subdirectory or file to search (relative to workspace)" },
                    "case_sensitive": { "type": "boolean", "description": "Case-sensitive matching (default: false)" }
                },
                "required": ["pattern"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: GrepArgs = serde_json::from_value(arguments)
            .map_err(|e| PurrError::Tool {
                tool: "grep".to_string(),
                message: format!("Invalid arguments: {e}"),
            })?;

        let search_root = match args.path {
            Some(ref p) => ctx.workspace_root.join(p),
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

        let workspace_root = ctx.workspace_root.clone();
        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            let max_matches = 100;

            for entry in WalkDir::new(&search_root).into_iter().filter_entry(|e| !is_ignored(e.path())).flatten() {
                if entry.file_type().is_file() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        let rel_path = entry
                            .path()
                            .strip_prefix(&workspace_root)
                            .unwrap_or(entry.path())
                            .to_string_lossy();

                        for (i, line) in content.lines().enumerate() {
                            if regex.is_match(line) {
                                results.push(format!("{}:{}:{}", rel_path, i + 1, line.trim()));
                                if results.len() >= max_matches {
                                    return results;
                                }
                            }
                        }
                    }
                }
            }
            results
        })
        .await
        .map_err(|e| PurrError::Tool {
            tool: "grep".to_string(),
            message: format!("Search task failed: {e}"),
        })?;

        let output = if matches.is_empty() {
            "No matches found.".to_string()
        } else {
            let count = matches.len();
            let mut res = matches.join("\n");
            if count >= 100 {
                res.push_str("\n(stopped at 100 matches limit)");
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
