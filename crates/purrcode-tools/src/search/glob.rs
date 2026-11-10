// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use regex::RegexBuilder;
use serde::Deserialize;
use std::path::Path;
use walkdir::WalkDir;

pub struct GlobTool;

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    path: Option<String>,
}

#[async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "glob".to_string(),
            description:
                "Find files matching a glob pattern (e.g. '**/*.rs', 'docs/*.md', 'src/**/lib.rs')"
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match files against" },
                    "path": { "type": "string", "description": "Subdirectory to search (relative to workspace root)" }
                },
                "required": ["pattern"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: GlobArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "glob".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let search_root = match args.path {
            Some(ref p) => crate::fs::read::resolve_confined_path(p, &ctx.workspace_root, "glob")?,
            None => ctx.workspace_root.clone(),
        };

        let regex_pattern = glob_to_regex(&args.pattern);
        let regex = RegexBuilder::new(&regex_pattern)
            .case_insensitive(true)
            .build()
            .map_err(|e| PurrError::Tool {
                tool: "glob".to_string(),
                message: format!("Failed to compile glob regex for '{}': {e}", args.pattern),
            })?;

        let workspace_root = ctx.workspace_root.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            let max_results = 100;

            for entry in WalkDir::new(&search_root)
                .into_iter()
                .filter_entry(|e| !is_ignored(e.path()))
                .flatten()
            {
                if entry.file_type().is_file() {
                    let rel_path = entry
                        .path()
                        .strip_prefix(&workspace_root)
                        .unwrap_or(entry.path())
                        .to_string_lossy();
                    let normalized = rel_path.replace('\\', "/");

                    if regex.is_match(&normalized) {
                        results.push(normalized);
                        if results.len() >= max_results {
                            return results;
                        }
                    }
                }
            }
            results
        })
        .await
        .map_err(|e| PurrError::Tool {
            tool: "glob".to_string(),
            message: format!("Glob search task failed: {e}"),
        })?;

        let output = if matches.is_empty() {
            "No files matched glob pattern.".to_string()
        } else {
            let count = matches.len();
            let mut out = matches.join("\n");
            if count >= 100 {
                out.push_str("\n(stopped at 100 matches limit)");
            }
            out
        };

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "pattern": args.pattern,
                "matches_count": matches.len()
            })),
        })
    }
}

fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::from("^");
    let mut chars = glob.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        regex.push_str("(?:.*/)?");
                    } else {
                        regex.push_str(".*");
                    }
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '.' | '(' | ')' | '+' | '|' | '^' | '$' | '@' | '%' => {
                regex.push('\\');
                regex.push(c);
            }
            '/' | '\\' => regex.push('/'),
            _ => regex.push(c),
        }
    }

    regex.push('$');
    regex
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

    #[test]
    fn test_glob_to_regex() {
        let r1 = glob_to_regex("**/*.rs");
        let reg1 = regex::Regex::new(&r1).unwrap();
        assert!(reg1.is_match("crates/purrcode-tools/src/lib.rs"));
        assert!(reg1.is_match("main.rs"));
        assert!(!reg1.is_match("Cargo.toml"));

        let r2 = glob_to_regex("docs/*.md");
        let reg2 = regex::Regex::new(&r2).unwrap();
        assert!(reg2.is_match("docs/PRD.md"));
        assert!(!reg2.is_match("docs/sub/PRD.md"));
    }
}
