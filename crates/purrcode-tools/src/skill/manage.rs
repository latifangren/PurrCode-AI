// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::{SkillManager, SkillScope};

pub struct ManageSkillTool {
    manager: Arc<Mutex<SkillManager>>,
}

impl ManageSkillTool {
    pub fn new(manager: Arc<Mutex<SkillManager>>) -> Self {
        Self { manager }
    }
}

#[derive(Deserialize)]
struct ManageSkillArgs {
    action: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    old_string: Option<String>,
    #[serde(default)]
    new_string: Option<String>,
    #[serde(default)]
    scope: Option<SkillScope>,
}

#[async_trait]
impl Tool for ManageSkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "manage_skill".to_string(),
            description: "Manage procedural skills (create, patch, delete, list). Allows the agent to codify and self-improve repeatable workflows into .purrcode/skills/ (project) or ~/.purrcode/skills/ (global).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "patch", "delete", "list"],
                        "description": "Action to perform on skills"
                    },
                    "name": {
                        "type": "string",
                        "description": "Skill name/slug (e.g. 'docker-deploy', 'release-audit')"
                    },
                    "description": {
                        "type": "string",
                        "description": "Short 1-sentence routing contract description for system prompt index"
                    },
                    "content": {
                        "type": "string",
                        "description": "Full markdown body of the skill (required for 'create')"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Target string to replace (required for 'patch')"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "New replacement string (required for 'patch')"
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["project", "global"],
                        "description": "Scope: 'project' (.purrcode/skills/ - default) or 'global' (~/.purrcode/skills/)"
                    }
                },
                "required": ["action"]
            }),
            tier: ApprovalTier::Write,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: ManageSkillArgs =
            serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!("Invalid manage_skill arguments: {e}"),
            })?;

        match args.action.as_str() {
            "create" => {
                let name = args.name.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'create' requires 'name'".to_string(),
                })?;
                let description = args.description.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'create' requires 'description'".to_string(),
                })?;
                let content = args.content.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'create' requires 'content'".to_string(),
                })?;
                let scope = args.scope.unwrap_or(SkillScope::Project);

                if ctx.is_dry_run {
                    return Ok(ToolResult {
                        output: format!(
                            "Dry-run: would create skill '{}' ({:?} scope)",
                            name, scope
                        ),
                        diff_preview: None,
                        metadata: Some(serde_json::json!({
                            "action": "create",
                            "name": name,
                            "scope": scope,
                        })),
                    });
                }

                let mut mgr = self.manager.lock().await;
                let (path, full_content) =
                    mgr.create_skill(name, description, content, scope, &ctx.workspace_root)?;

                Ok(ToolResult {
                    output: format!(
                        "Successfully created skill '{}' at '{}'",
                        name,
                        path.display()
                    ),
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "action": "create",
                        "skill_name": name,
                        "path": path.display().to_string(),
                        "scope": scope,
                        "bytes": full_content.len(),
                    })),
                })
            }
            "patch" => {
                let name = args.name.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'patch' requires 'name'".to_string(),
                })?;
                let old_string = args.old_string.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'patch' requires 'old_string'".to_string(),
                })?;
                let new_string = args.new_string.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'patch' requires 'new_string'".to_string(),
                })?;

                if ctx.is_dry_run {
                    return Ok(ToolResult {
                        output: format!("Dry-run: would patch skill '{}'", name),
                        diff_preview: None,
                        metadata: Some(serde_json::json!({
                            "action": "patch",
                            "name": name,
                        })),
                    });
                }

                let mut mgr = self.manager.lock().await;
                let (path, diff, _) = mgr.patch_skill(
                    name,
                    old_string,
                    new_string,
                    args.scope,
                    &ctx.workspace_root,
                )?;

                Ok(ToolResult {
                    output: format!(
                        "Successfully patched skill '{}' at '{}'",
                        name,
                        path.display()
                    ),
                    diff_preview: Some(diff),
                    metadata: Some(serde_json::json!({
                        "action": "patch",
                        "skill_name": name,
                        "path": path.display().to_string(),
                    })),
                })
            }
            "delete" => {
                let name = args.name.as_deref().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Action 'delete' requires 'name'".to_string(),
                })?;

                if ctx.is_dry_run {
                    return Ok(ToolResult {
                        output: format!("Dry-run: would delete skill '{}'", name),
                        diff_preview: None,
                        metadata: Some(serde_json::json!({
                            "action": "delete",
                            "name": name,
                        })),
                    });
                }

                let mut mgr = self.manager.lock().await;
                let msg = mgr.delete_skill(name, args.scope, &ctx.workspace_root)?;

                Ok(ToolResult {
                    output: msg,
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "action": "delete",
                        "skill_name": name,
                    })),
                })
            }
            "list" => {
                let mgr = self.manager.lock().await;
                let list = mgr.list_skills_detailed();

                let mut output = String::from("### Available Procedural Skills\n\n| Name | Source | Description | Path |\n| --- | --- | --- | --- |\n");
                for s in &list {
                    output.push_str(&format!(
                        "| `{}` | {:?} | {} | {} |\n",
                        s.name,
                        s.source,
                        s.description,
                        s.path.as_deref().unwrap_or("embedded")
                    ));
                }

                Ok(ToolResult {
                    output,
                    diff_preview: None,
                    metadata: Some(serde_json::json!({
                        "skills": list,
                    })),
                })
            }
            other => Err(PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!(
                    "Unknown action '{}'. Valid actions are: create, patch, delete, list",
                    other
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct TempTestDir(PathBuf);
    impl TempTestDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("purrcode-test-{}", uuid::Uuid::new_v4()));
            let _ = std::fs::create_dir_all(&p);
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_ctx(workspace: &Path) -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            workspace_root: workspace.to_path_buf(),
            is_dry_run: false,
            is_cron: false,
            agent_role: "tinker".to_string(),
            sandbox: None,
        }
    }

    #[tokio::test]
    async fn test_manage_skill_lifecycle_create_patch_list_delete() {
        let tmp = TempTestDir::new();
        let mgr = Arc::new(Mutex::new(SkillManager::new()));
        let tool = ManageSkillTool::new(mgr.clone());
        let ctx = test_ctx(tmp.path());

        // 1. Create
        let create_res = tool
            .execute(
                serde_json::json!({
                    "action": "create",
                    "name": "release-check",
                    "description": "Pre-release quality and verification gates",
                    "content": "# Release Check\n- Check 1: run tests\n- Check 2: check git status",
                    "scope": "project"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(create_res.output.contains("Successfully created skill"));

        // 2. List
        let list_res = tool
            .execute(serde_json::json!({ "action": "list" }), &ctx)
            .await
            .unwrap();
        assert!(list_res.output.contains("release-check"));
        assert!(list_res.output.contains("Pre-release quality"));

        // 3. Patch
        let patch_res = tool
            .execute(
                serde_json::json!({
                    "action": "patch",
                    "name": "release-check",
                    "old_string": "- Check 1: run tests",
                    "new_string": "- Check 1: cargo test --workspace"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(patch_res.output.contains("Successfully patched skill"));
        assert!(patch_res.diff_preview.is_some());
        let diff = patch_res.diff_preview.unwrap();
        assert!(diff.contains("cargo test --workspace"));

        // 4. Delete
        let del_res = tool
            .execute(
                serde_json::json!({
                    "action": "delete",
                    "name": "release-check"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(del_res.output.contains("Successfully deleted skill"));
    }

    #[tokio::test]
    async fn test_manage_skill_dry_run() {
        let tmp = TempTestDir::new();
        let mgr = Arc::new(Mutex::new(SkillManager::new()));
        let tool = ManageSkillTool::new(mgr);
        let mut ctx = test_ctx(tmp.path());
        ctx.is_dry_run = true;

        let res = tool
            .execute(
                serde_json::json!({
                    "action": "create",
                    "name": "dry-skill",
                    "description": "Dry skill desc",
                    "content": "# Content"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(res
            .output
            .contains("Dry-run: would create skill 'dry-skill'"));
        assert!(!tmp.path().join(".purrcode/skills/dry-skill").exists());
    }
}
