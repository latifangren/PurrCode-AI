// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

pub struct ReadTool;

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_string(),
            description: "Read a UTF-8 text file window with line numbers".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path to file" },
                    "offset": { "type": "integer", "description": "1-based starting line number (default: 1)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to return (default: 500, max: 2000)" }
                },
                "required": ["path"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: ReadArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "read".to_string(),
            message: format!("Invalid arguments: {e}"),
        })?;

        let full_path = resolve_confined_path(&args.path, &ctx.workspace_root, "read")?;
        if !full_path.exists() {
            return Err(PurrError::Tool {
                tool: "read".to_string(),
                message: format!("File not found: '{}'", args.path),
            });
        }

        if full_path.is_dir() {
            return Err(PurrError::Tool {
                tool: "read".to_string(),
                message: format!("Path '{}' is a directory, not a regular file.", args.path),
            });
        }

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| PurrError::Tool {
                tool: "read".to_string(),
                message: format!("Failed to read file '{}': {e}", args.path),
            })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(500).min(2000);

        let start_idx = (offset - 1).min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);

        let mut output = String::new();
        if start_idx > 0 {
            output.push_str(&format!("({} line(s) above)\n", start_idx));
        }

        for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
            let line_num = start_idx + i + 1;
            output.push_str(&format!("{}|{}\n", line_num, line));
        }

        if end_idx < total_lines {
            output.push_str(&format!("(more lines below, total: {})\n", total_lines));
        }

        let content_hash = compute_fnv1a_hex(&content);

        Ok(ToolResult {
            output,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "total_lines": total_lines,
                "offset": offset,
                "lines_returned": end_idx - start_idx,
                "content_hash": content_hash
            })),
        })
    }
}

/// Computes a deterministic 64-bit FNV-1a hex digest for hash-anchored editing
pub fn compute_fnv1a_hex(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

/// Resolves a path and enforces workspace confinement (`restrict_to_workspace: true`).
pub fn resolve_confined_path(
    input_path: &str,
    workspace_root: &Path,
    tool_name: &str,
) -> Result<PathBuf> {
    let abs_root = if workspace_root.is_absolute() {
        workspace_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace_root)
    };
    let canonical_root =
        std::fs::canonicalize(&abs_root).unwrap_or_else(|_| normalize_lexical(&abs_root));

    let clean_root = strip_verbatim_prefix(&canonical_root);

    let p = Path::new(input_path);
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        clean_root.join(p)
    };

    let normalized_candidate = normalize_lexical(&candidate);
    let clean_candidate = strip_verbatim_prefix(&normalized_candidate);

    if !path_starts_with_confinement(&clean_candidate, &clean_root) {
        return Err(PurrError::Tool {
            tool: tool_name.to_string(),
            message: format!(
                "Workspace confinement violation: path '{}' resolves outside workspace root '{}'",
                input_path,
                clean_root.display()
            ),
        });
    }

    if clean_candidate.exists() {
        if let Ok(canon_target) = std::fs::canonicalize(&clean_candidate) {
            let clean_target = strip_verbatim_prefix(&canon_target);
            if !path_starts_with_confinement(&clean_target, &clean_root) {
                return Err(PurrError::Tool {
                    tool: tool_name.to_string(),
                    message: format!(
                        "Workspace confinement violation: symlink '{}' points outside workspace root",
                        input_path
                    ),
                });
            }
            return Ok(clean_target);
        }
    }

    Ok(clean_candidate)
}

/// Strips Windows verbatim prefix `\\?\` or `\\?\UNC\` so paths compare consistently across APIs
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let mut comps = path.components();
        if let Some(Component::Prefix(p)) = comps.next() {
            match p.kind() {
                Prefix::VerbatimDisk(d) => {
                    let disk_str = format!("{}:", d as char);
                    let mut res = PathBuf::from(disk_str);
                    for comp in comps {
                        res.push(comp.as_os_str());
                    }
                    return res;
                }
                Prefix::VerbatimUNC(server, share) => {
                    let mut res = PathBuf::from(format!(
                        r"\\{}\{}",
                        server.to_string_lossy(),
                        share.to_string_lossy()
                    ));
                    for comp in comps {
                        res.push(comp.as_os_str());
                    }
                    return res;
                }
                _ => {}
            }
        }
    }
    path.to_path_buf()
}

pub fn path_starts_with_confinement(path: &Path, prefix: &Path) -> bool {
    #[cfg(windows)]
    {
        let clean_path = strip_verbatim_prefix(path);
        let clean_prefix = strip_verbatim_prefix(prefix);
        let p_comps: Vec<_> = clean_path.components().collect();
        let pref_comps: Vec<_> = clean_prefix.components().collect();
        if p_comps.len() < pref_comps.len() {
            return false;
        }
        for (a, b) in p_comps.iter().zip(pref_comps.iter()) {
            let a_str = a.as_os_str().to_string_lossy().to_lowercase();
            let b_str = b.as_os_str().to_string_lossy().to_lowercase();
            if a_str != b_str {
                return false;
            }
        }
        true
    }
    #[cfg(not(windows))]
    {
        path.starts_with(prefix)
    }
}

pub fn resolve_path(input_path: &str, workspace_root: &Path) -> PathBuf {
    resolve_confined_path(input_path, workspace_root, "fs")
        .unwrap_or_else(|_| workspace_root.join(input_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_tool() {
        let temp_dir = std::env::temp_dir().join(format!("purr_read_ws_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let file_path = temp_dir.join("purr_read_test.txt");
        tokio::fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
            .await
            .unwrap();

        let tool = ReadTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "tinker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
            is_cron: false,
        };

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "purr_read_test.txt",
                    "offset": 2,
                    "limit": 2
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("2|line 2"));
        assert!(result.output.contains("3|line 3"));
        assert!(!result.output.contains("4|line 4"));

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[tokio::test]
    async fn test_read_confinement_blocks_traversal() {
        let temp_dir = std::env::temp_dir().join(format!("purr_confine_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        let tool = ReadTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            agent_role: "tinker".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
            is_cron: false,
        };

        let res = tool
            .execute(serde_json::json!({ "path": "../../etc/passwd" }), &ctx)
            .await;
        assert!(res.is_err());

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[test]
    fn test_strip_verbatim_prefix_and_confinement_windows() {
        #[cfg(windows)]
        {
            let verbatim = Path::new(r"\\?\C:\project\src\main.rs");
            let clean = strip_verbatim_prefix(verbatim);
            assert_eq!(clean, PathBuf::from(r"C:\project\src\main.rs"));

            let root = Path::new(r"C:\project");
            assert!(path_starts_with_confinement(verbatim, root));
            assert!(path_starts_with_confinement(&clean, root));

            // Case insensitivity test on Windows
            let upper_candidate = Path::new(r"C:\PROJECT\SRC\MAIN.RS");
            assert!(path_starts_with_confinement(upper_candidate, root));
        }

        #[cfg(not(windows))]
        {
            let unix_path = Path::new("/workspace/src/main.rs");
            let clean = strip_verbatim_prefix(unix_path);
            assert_eq!(clean, PathBuf::from("/workspace/src/main.rs"));
            let root = Path::new("/workspace");
            assert!(path_starts_with_confinement(unix_path, root));
        }
    }
}
