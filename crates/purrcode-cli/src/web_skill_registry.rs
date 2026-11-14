// SPDX-License-Identifier: GPL-3.0-or-later

//! `SkillRegistry` implementation for the WebUI (v0.4 B5).
//!
//! A stateless adapter: every call builds a fresh `SkillManager`,
//! re-discovers from disk, and routes the mutation through the
//! `manage_skill` tool — exactly the path a Tinker turn takes —
//! so the WebUI never writes a skill file itself and never holds
//! a stale in-memory index (disk is the source of truth; turns
//! re-discover the same way). There is no runtime-only skill
//! state to share, unlike the B4 plugin manager's mounted set,
//! so no shared instance is needed.
//!
//! Scope directories: project is `<workspace>/.purrcode/skills`,
//! global is `<purrcode_dir>/skills`. The tool's global scope
//! resolves through `dirs_home()`; in production the two agree
//! because `purrcode_dir` is `HOME/.purrcode` by construction
//! (`purrcode_service::purrcode_dir`).

use async_trait::async_trait;
use purrcode_api::error::PurrError;
use purrcode_api::skill::{
    SkillDetail, SkillError, SkillInfo, SkillRegistry, SkillScope, SkillSource,
};
use purrcode_api::tool::{Tool, ToolContext};
use purrcode_tools::skill::{ManageSkillTool, SkillManager};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct WebSkillRegistry {
    workspace_root: PathBuf,
    purrcode_dir: PathBuf,
    /// Serializes web skill mutations (create/update/delete)
    /// so the check-then-act pairs in them — the same-scope
    /// 409 check, the existence pre-check — hold against
    /// concurrent requests (fix pass B5 r1 P2-4: parallel
    /// creates of one name used to slip past the 409 check and
    /// overwrite each other). Reads stay lock-free: every read
    /// re-discovers from disk.
    mutation_lock: Mutex<()>,
}

impl WebSkillRegistry {
    pub fn new(workspace_root: PathBuf, purrcode_dir: PathBuf) -> Self {
        Self {
            workspace_root,
            purrcode_dir,
            mutation_lock: Mutex::new(()),
        }
    }

    /// Fresh manager + discovery for the read path (list/get).
    /// Disk is the source of truth; a fresh discovery per call
    /// can never serve a stale index.
    fn read_manager(&self) -> SkillManager {
        let mut mgr = SkillManager::new();
        mgr.discover_all(&self.workspace_root);
        mgr
    }

    /// The `manage_skill` tool bound to a freshly discovered
    /// manager — the one write path for every mutation below.
    fn tool(&self) -> ManageSkillTool {
        let mut mgr = SkillManager::new();
        mgr.discover_all(&self.workspace_root);
        ManageSkillTool::new(Arc::new(Mutex::new(mgr)))
    }

    fn ctx(&self) -> ToolContext {
        ToolContext {
            session_id: "web".to_string(),
            agent_role: "web".to_string(),
            workspace_root: self.workspace_root.clone(),
            is_dry_run: false,
            is_cron: false,
            sandbox: None,
        }
    }

    /// The directory a scope's copy of `name` lives in.
    fn scope_dir(&self, scope: SkillScope, name: &str) -> PathBuf {
        match scope {
            SkillScope::Project => self
                .workspace_root
                .join(".purrcode")
                .join("skills")
                .join(name),
            SkillScope::Global => self.purrcode_dir.join("skills").join(name),
        }
    }

    fn scope_file(&self, scope: SkillScope, name: &str) -> PathBuf {
        self.scope_dir(scope, name).join("SKILL.md")
    }
}

fn info_of(detail: &purrcode_tools::skill::SkillDetail) -> SkillInfo {
    SkillInfo {
        name: detail.name.clone(),
        description: detail.description.clone(),
        source: match detail.source {
            purrcode_tools::skill::SkillSource::Embedded => SkillSource::Embedded,
            purrcode_tools::skill::SkillSource::Global => SkillSource::Global,
            purrcode_tools::skill::SkillSource::Project => SkillSource::Project,
        },
        path: detail.path.clone(),
    }
}

/// `manage_skill` validation failures come back as
/// `PurrError::Tool`; filesystem failures as `PurrError::Io`.
fn map_tool_error(e: PurrError) -> SkillError {
    match e {
        PurrError::Tool { message, .. } => SkillError::InvalidInput(message),
        other => SkillError::Internal(other.to_string()),
    }
}

#[async_trait]
impl SkillRegistry for WebSkillRegistry {
    async fn list_skills(&self) -> Vec<SkillInfo> {
        self.read_manager()
            .list_skills_detailed()
            .iter()
            .map(info_of)
            .collect()
    }

    async fn get_skill(&self, name: &str) -> Result<SkillDetail, SkillError> {
        let mgr = self.read_manager();
        let detail = mgr
            .list_skills_detailed()
            .into_iter()
            .find(|d| d.name == name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
        let content = mgr
            .read_skill_body(name)
            .map_err(|e| SkillError::Internal(e.to_string()))?;
        Ok(SkillDetail {
            name: detail.name.clone(),
            description: detail.description.clone(),
            source: match detail.source {
                purrcode_tools::skill::SkillSource::Embedded => SkillSource::Embedded,
                purrcode_tools::skill::SkillSource::Global => SkillSource::Global,
                purrcode_tools::skill::SkillSource::Project => SkillSource::Project,
            },
            path: detail.path,
            content,
        })
    }

    async fn create_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError> {
        let _guard = self.mutation_lock.lock().await;
        if self.scope_file(scope, name).exists() {
            return Err(SkillError::AlreadyExists(name.to_string()));
        }
        let tool = self.tool();
        let result = tool
            .execute(
                serde_json::json!({
                    "action": "create",
                    "name": name,
                    "description": description,
                    "content": content,
                    "scope": scope,
                }),
                &self.ctx(),
            )
            .await
            .map_err(map_tool_error)?;
        let meta = result
            .metadata
            .and_then(|m| m.get("path").and_then(|p| p.as_str()).map(str::to_string));
        // Report the skill as stored, not as requested: the
        // stored description is authoritative when the body's
        // frontmatter carries its own (fix pass B5 r1 P2-5).
        if let Ok(detail) = self.get_skill(name).await {
            return Ok(SkillInfo {
                name: detail.name,
                description: detail.description,
                source: detail.source,
                path: detail.path,
            });
        }
        Ok(SkillInfo {
            name: name.to_string(),
            description: description.to_string(),
            source: match scope {
                SkillScope::Project => SkillSource::Project,
                SkillScope::Global => SkillSource::Global,
            },
            path: meta,
        })
    }

    async fn update_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError> {
        let _guard = self.mutation_lock.lock().await;
        if !self.scope_file(scope, name).exists() {
            return Err(SkillError::NotFound(name.to_string()));
        }
        // `manage_skill` create overwrites an existing file, so an
        // update is a create against an existing name — the
        // existence check above is what makes it an update rather
        // than a silent shadow.
        let tool = self.tool();
        let result = tool
            .execute(
                serde_json::json!({
                    "action": "create",
                    "name": name,
                    "description": description,
                    "content": content,
                    "scope": scope,
                }),
                &self.ctx(),
            )
            .await
            .map_err(map_tool_error)?;
        let meta = result
            .metadata
            .and_then(|m| m.get("path").and_then(|p| p.as_str()).map(str::to_string));
        // Report the skill as stored (see create_skill).
        if let Ok(detail) = self.get_skill(name).await {
            return Ok(SkillInfo {
                name: detail.name,
                description: detail.description,
                source: detail.source,
                path: detail.path,
            });
        }
        Ok(SkillInfo {
            name: name.to_string(),
            description: description.to_string(),
            source: match scope {
                SkillScope::Project => SkillSource::Project,
                SkillScope::Global => SkillSource::Global,
            },
            path: meta,
        })
    }

    async fn delete_skill(&self, name: &str, scope: SkillScope) -> Result<(), SkillError> {
        let _guard = self.mutation_lock.lock().await;
        if !self.scope_file(scope, name).exists() {
            return Err(SkillError::NotFound(name.to_string()));
        }
        let tool = self.tool();
        tool.execute(
            serde_json::json!({
                "action": "delete",
                "name": name,
                "scope": scope,
            }),
            &self.ctx(),
        )
        .await
        .map_err(map_tool_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "purrcode-web-skill-test-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn registry(root: &std::path::Path) -> WebSkillRegistry {
        WebSkillRegistry::new(root.to_path_buf(), root.join(".purrcode"))
    }

    #[tokio::test]
    async fn test_web_skill_registry_crud_project_scope() {
        let root = tmp_root("crud");
        let reg = registry(&root);
        // The 8 embedded defaults are always listed.
        let listed = reg.list_skills().await;
        assert_eq!(listed.len(), 8);
        assert!(listed.iter().all(|s| s.source == SkillSource::Embedded));

        let created = reg
            .create_skill(
                "deploy-notes",
                "Notes on deploys",
                "# Deploy\n",
                SkillScope::Project,
            )
            .await
            .unwrap();
        assert_eq!(created.name, "deploy-notes");
        assert_eq!(created.source, SkillSource::Project);
        assert!(reg.scope_file(SkillScope::Project, "deploy-notes").exists());

        // Same-scope collision is a 409, not a silent overwrite.
        assert!(matches!(
            reg.create_skill("deploy-notes", "Again", "# x\n", SkillScope::Project)
                .await,
            Err(SkillError::AlreadyExists(_))
        ));

        let detail = reg.get_skill("deploy-notes").await.unwrap();
        // create_skill trims the body when it prepends frontmatter.
        assert_eq!(
            detail.content,
            "---\nname: deploy-notes\ndescription: Notes on deploys\n---\n\n# Deploy"
        );
        assert_eq!(detail.source, SkillSource::Project);

        let updated = reg
            .update_skill(
                "deploy-notes",
                "Notes on deploys v2",
                "# Deploy v2\n",
                SkillScope::Project,
            )
            .await
            .unwrap();
        assert_eq!(updated.description, "Notes on deploys v2");
        assert!(reg
            .get_skill("deploy-notes")
            .await
            .unwrap()
            .content
            .contains("Deploy v2"));

        reg.delete_skill("deploy-notes", SkillScope::Project)
            .await
            .unwrap();
        assert!(matches!(
            reg.get_skill("deploy-notes").await,
            Err(SkillError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_web_skill_registry_parallel_create_one_winner() {
        // Fix pass B5 r1 P2-4: parallel creates of one name
        // used to slip past the 409 check and overwrite each
        // other — the mutation lock makes exactly one win.
        let root = tmp_root("race");
        let reg = Arc::new(registry(&root));
        let mut tasks = Vec::new();
        for i in 0..10 {
            let reg = reg.clone();
            tasks.push(tokio::spawn(async move {
                reg.create_skill(
                    "race-one",
                    &format!("Racer {i}"),
                    "# Race\n",
                    SkillScope::Project,
                )
                .await
            }));
        }
        let mut ok = 0;
        let mut dup = 0;
        for t in tasks {
            match t.await.unwrap() {
                Ok(_) => ok += 1,
                Err(SkillError::AlreadyExists(_)) => dup += 1,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
        assert_eq!(ok, 1, "exactly one create must win");
        assert_eq!(dup, 9);
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_web_skill_registry_create_reports_stored_description() {
        // Fix pass B5 r1 P2-5: the response must echo the
        // description as stored, not the form value — a body
        // whose frontmatter carries its own description wins.
        let root = tmp_root("stored-desc");
        let reg = registry(&root);
        let created = reg
            .create_skill(
                "fm-desc",
                "FORM-DESC",
                "---\ndescription: BODY-DESC\n---\n\n# Body\n",
                SkillScope::Project,
            )
            .await
            .unwrap();
        assert_eq!(created.description, "BODY-DESC");
        assert_eq!(
            reg.get_skill("fm-desc").await.unwrap().description,
            "BODY-DESC"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_web_skill_registry_missing_and_invalid() {
        let root = tmp_root("missing");
        let reg = registry(&root);
        assert!(matches!(
            reg.get_skill("ghost").await,
            Err(SkillError::NotFound(_))
        ));
        assert!(matches!(
            reg.update_skill("ghost", "d", "c", SkillScope::Project)
                .await,
            Err(SkillError::NotFound(_))
        ));
        assert!(matches!(
            reg.delete_skill("ghost", SkillScope::Project).await,
            Err(SkillError::NotFound(_))
        ));
        // Embedded skills cannot be updated or deleted (no file).
        assert!(matches!(
            reg.update_skill("plan", "d", "c", SkillScope::Project)
                .await,
            Err(SkillError::NotFound(_))
        ));
        assert!(matches!(
            reg.delete_skill("plan", SkillScope::Project).await,
            Err(SkillError::NotFound(_))
        ));
        // manage_skill validation surfaces as InvalidInput.
        assert!(matches!(
            reg.create_skill("BAD NAME!", "d", "c", SkillScope::Project)
                .await,
            Err(SkillError::InvalidInput(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }
}
