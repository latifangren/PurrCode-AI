// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::path::Path;
use walkdir::WalkDir;

pub struct FindTool;

#[derive(Deserialize)]
struct FindArgs {
    pattern: String,
    path: Option<String>,
}

#[async_trait]
impl Tool for FindTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "find".to_string(),
            description: "Find files and directories by filename or substring".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Filename or substring to search for" },
                    "path": { "type": "string", "description": "Subdirectory to search (relative to workspace)" }
                },
                "required": ["pattern"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: FindArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "find".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let search_root = match args.path {
            Some(ref p) => crate::fs::read::resolve_confined_path(p, &ctx.workspace_root, "find")?,
            None => ctx.workspace_root.clone(),
        };

        let pat = args.pattern.to_lowercase();
        let workspace_root = ctx.workspace_root.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            let max_results = 100;

            for entry in WalkDir::new(&search_root)
                .into_iter()
                .filter_entry(|e| !is_ignored(e.path()))
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.contains(&pat) {
                    let rel = entry
                        .path()
                        .strip_prefix(&workspace_root)
                        .unwrap_or(entry.path())
                        .to_string_lossy();
                    results.push(rel.to_string());
                    if results.len() >= max_results {
                        return results;
                    }
                }
            }
            results
        })
        .await
        .map_err(|e| PurrError::Tool {
            tool: "find".to_string(),
            message: format!("Find task failed: {e}"),
        })?;

        let output = if matches.is_empty() {
            "No files found matching pattern.".to_string()
        } else {
            matches.join("\n")
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
