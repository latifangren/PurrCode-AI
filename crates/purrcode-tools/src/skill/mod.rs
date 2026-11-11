// SPDX-License-Identifier: GPL-3.0-or-later

pub mod manage;
pub mod tool;

use purrcode_api::error::{PurrError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use manage::ManageSkillTool;
pub use tool::ReadSkillTool;

const BUNDLED_SKILLS: &[(&str, &str)] = &[
    ("debug", include_str!("../../../../skills/debug/SKILL.md")),
    ("doctor", include_str!("../../../../skills/doctor/SKILL.md")),
    (
        "frontend-design",
        include_str!("../../../../skills/frontend-design/SKILL.md"),
    ),
    ("git", include_str!("../../../../skills/git/SKILL.md")),
    ("plan", include_str!("../../../../skills/plan/SKILL.md")),
    ("review", include_str!("../../../../skills/review/SKILL.md")),
    (
        "scaffold",
        include_str!("../../../../skills/scaffold/SKILL.md"),
    ),
    (
        "webapp-testing",
        include_str!("../../../../skills/webapp-testing/SKILL.md"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Embedded,
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    Project,
    Global,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub file_path: Option<PathBuf>,
    #[serde(skip)]
    pub embedded_body: Option<&'static str>,
    pub source: SkillSource,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub path: Option<String>,
}

pub struct SkillManager {
    skills: HashMap<String, SkillMetadata>,
}

impl SkillManager {
    pub fn new() -> Self {
        let mut mgr = Self {
            skills: HashMap::new(),
        };
        mgr.load_embedded_defaults();
        mgr
    }

    fn load_embedded_defaults(&mut self) {
        for (default_name, raw_content) in BUNDLED_SKILLS {
            let (name, description) = parse_skill_frontmatter(raw_content, default_name);
            self.skills.insert(
                name.clone(),
                SkillMetadata {
                    name,
                    description,
                    file_path: None,
                    embedded_body: Some(raw_content),
                    source: SkillSource::Embedded,
                },
            );
        }
    }

    /// Discovers skills from bundled path, global user path, and local project path
    pub fn discover_all(&mut self, workspace_root: &Path) {
        // 0. Embedded bundled skills are already loaded as base fallback
        self.load_embedded_defaults();

        // 1. Bundled skills in workspace/skills/
        self.scan_directory(&workspace_root.join("skills"), SkillSource::Project);

        // 2. User home ~/.purrcode/skills/
        if let Some(home) = dirs_home() {
            self.scan_directory(&home.join(".purrcode").join("skills"), SkillSource::Global);
        }

        // 3. Project-local .purrcode/skills/ (highest precedence)
        self.scan_directory(
            &workspace_root.join(".purrcode").join("skills"),
            SkillSource::Project,
        );
    }

    fn scan_directory(&mut self, base_dir: &Path, source: SkillSource) {
        if !base_dir.exists() || !base_dir.is_dir() {
            return;
        }

        if let Ok(entries) = std::fs::read_dir(base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let skill_file = if path.is_dir() {
                    path.join("SKILL.md")
                } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    path
                } else {
                    continue;
                };

                if skill_file.exists() {
                    if let Ok(meta) = parse_skill_file(&skill_file, source) {
                        self.skills.insert(meta.name.clone(), meta);
                    }
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&SkillMetadata> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&SkillMetadata> {
        let mut list: Vec<&SkillMetadata> = self.skills.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn list_skills_detailed(&self) -> Vec<SkillDetail> {
        let mut list: Vec<SkillDetail> = self
            .skills
            .values()
            .map(|m| SkillDetail {
                name: m.name.clone(),
                description: m.description.clone(),
                source: m.source,
                path: m.file_path.as_ref().map(|p| p.display().to_string()),
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    /// Formats a concise index of available skills for the system prompt (~50-100 tokens)
    pub fn format_prompt_index(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        lines.push("<available-skills>".to_string());
        for meta in self.list() {
            lines.push(format!("- {}: {}", meta.name, meta.description));
        }
        lines.push(
            "Call tool 'read_skill' to inspect any skill's full instructions on-demand."
                .to_string(),
        );
        lines.push("</available-skills>".to_string());

        lines.join("\n")
    }

    pub fn read_skill_body(&self, name: &str) -> Result<String> {
        if let Some(meta) = self.skills.get(name) {
            if let Some(ref path) = meta.file_path {
                if let Ok(content) = std::fs::read_to_string(path) {
                    return Ok(content);
                }
            }
            if let Some(embedded) = meta.embedded_body {
                return Ok(embedded.to_string());
            }
            Err(PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Skill '{}' has no readable source", name),
            })
        } else {
            Err(PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Skill '{}' not found", name),
            })
        }
    }

    pub fn create_skill(
        &mut self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
        workspace_root: &Path,
    ) -> Result<(PathBuf, String)> {
        let valid_name = validate_skill_name(name).map_err(|msg| PurrError::Tool {
            tool: "manage_skill".to_string(),
            message: msg,
        })?;
        let valid_desc =
            validate_skill_description(description).map_err(|msg| PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: msg,
            })?;

        if content.len() > 100_000 {
            return Err(PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!(
                    "Skill content exceeds 100,000 characters limit (was {})",
                    content.len()
                ),
            });
        }

        let full_content = if content.trim_start().starts_with("---") {
            let (fm_name, fm_desc) = parse_skill_frontmatter(content, &valid_name);
            if fm_name.is_empty() || fm_desc.is_empty() {
                format!(
                    "---\nname: {}\ndescription: {}\n---\n\n{}",
                    valid_name,
                    valid_desc,
                    content.trim()
                )
            } else {
                content.to_string()
            }
        } else {
            format!(
                "---\nname: {}\ndescription: {}\n---\n\n{}",
                valid_name,
                valid_desc,
                content.trim()
            )
        };

        let target_dir = match scope {
            SkillScope::Project => workspace_root
                .join(".purrcode")
                .join("skills")
                .join(&valid_name),
            SkillScope::Global => {
                let home = dirs_home().ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: "Could not determine user home directory for global skill".to_string(),
                })?;
                home.join(".purrcode").join("skills").join(&valid_name)
            }
        };

        std::fs::create_dir_all(&target_dir).map_err(PurrError::Io)?;
        let file_path = target_dir.join("SKILL.md");
        std::fs::write(&file_path, full_content.as_bytes()).map_err(PurrError::Io)?;

        let source = match scope {
            SkillScope::Project => SkillSource::Project,
            SkillScope::Global => SkillSource::Global,
        };

        self.skills.insert(
            valid_name.clone(),
            SkillMetadata {
                name: valid_name,
                description: valid_desc,
                file_path: Some(file_path.clone()),
                embedded_body: None,
                source,
            },
        );

        Ok((file_path, full_content))
    }

    pub fn patch_skill(
        &mut self,
        name: &str,
        old_string: &str,
        new_string: &str,
        scope: Option<SkillScope>,
        workspace_root: &Path,
    ) -> Result<(PathBuf, String, String)> {
        let valid_name = validate_skill_name(name).map_err(|msg| PurrError::Tool {
            tool: "manage_skill".to_string(),
            message: msg,
        })?;

        if old_string.is_empty() {
            return Err(PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: "The 'old_string' for patch cannot be empty".to_string(),
            });
        }

        let (original, file_path, source) = {
            let meta = self
                .skills
                .get(&valid_name)
                .ok_or_else(|| PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: format!("Skill '{}' not found", valid_name),
                })?;

            if let Some(ref path) = meta.file_path {
                let content = std::fs::read_to_string(path).map_err(PurrError::Io)?;
                (content, path.clone(), meta.source)
            } else if let Some(embedded) = meta.embedded_body {
                let target_scope = scope.unwrap_or(SkillScope::Project);
                let target_dir = match target_scope {
                    SkillScope::Project => workspace_root
                        .join(".purrcode")
                        .join("skills")
                        .join(&valid_name),
                    SkillScope::Global => {
                        let home = dirs_home().ok_or_else(|| PurrError::Tool {
                            tool: "manage_skill".to_string(),
                            message: "Could not determine user home directory".to_string(),
                        })?;
                        home.join(".purrcode").join("skills").join(&valid_name)
                    }
                };
                std::fs::create_dir_all(&target_dir).map_err(PurrError::Io)?;
                let ejected_path = target_dir.join("SKILL.md");
                (
                    embedded.to_string(),
                    ejected_path,
                    match target_scope {
                        SkillScope::Project => SkillSource::Project,
                        SkillScope::Global => SkillSource::Global,
                    },
                )
            } else {
                return Err(PurrError::Tool {
                    tool: "manage_skill".to_string(),
                    message: format!("Skill '{}' has no readable content", valid_name),
                });
            }
        };

        let (start, end) =
            crate::fs::find_match_range(&original, old_string).map_err(|msg| PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!("Failed to patch skill '{}': {}", valid_name, msg),
            })?;

        let mut new_content = String::with_capacity(original.len() + new_string.len());
        new_content.push_str(&original[..start]);
        new_content.push_str(new_string);
        new_content.push_str(&original[end..]);

        if new_content.len() > 100_000 {
            return Err(PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!(
                    "Patched skill content exceeds 100,000 characters limit (was {})",
                    new_content.len()
                ),
            });
        }

        let (parsed_name, parsed_desc) = parse_skill_frontmatter(&new_content, &valid_name);

        let diff = similar::TextDiff::from_lines(&original, &new_content);
        let diff_preview = diff
            .unified_diff()
            .context_radius(3)
            .header(
                &format!("a/{}", file_path.display()),
                &format!("b/{}", file_path.display()),
            )
            .to_string();

        std::fs::write(&file_path, new_content.as_bytes()).map_err(PurrError::Io)?;

        self.skills.insert(
            parsed_name.clone(),
            SkillMetadata {
                name: parsed_name,
                description: parsed_desc,
                file_path: Some(file_path.clone()),
                embedded_body: None,
                source,
            },
        );

        Ok((file_path, diff_preview, new_content))
    }

    pub fn delete_skill(
        &mut self,
        name: &str,
        _scope: Option<SkillScope>,
        _workspace_root: &Path,
    ) -> Result<String> {
        let valid_name = validate_skill_name(name).map_err(|msg| PurrError::Tool {
            tool: "manage_skill".to_string(),
            message: msg,
        })?;

        let meta = self
            .skills
            .get(&valid_name)
            .ok_or_else(|| PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!("Skill '{}' not found", valid_name),
            })?;

        if meta.source == SkillSource::Embedded && meta.file_path.is_none() {
            return Err(PurrError::Tool {
                tool: "manage_skill".to_string(),
                message: format!(
                    "Cannot delete embedded bundled skill '{}'. Bundled skills are part of the core binary.",
                    valid_name
                ),
            });
        }

        let path_opt = meta.file_path.clone();
        if let Some(file_path) = path_opt {
            if file_path.exists() {
                std::fs::remove_file(&file_path).map_err(PurrError::Io)?;
                if let Some(parent) = file_path.parent() {
                    if parent.file_name().and_then(|s| s.to_str()) == Some(&valid_name) {
                        let _ = std::fs::remove_dir(parent);
                    }
                }
            }
        }

        let restored = self.restore_embedded_fallback_if_exists(&valid_name);
        if !restored {
            self.skills.remove(&valid_name);
        }

        let msg = if restored {
            format!(
                "Deleted local override for skill '{}'. Restored embedded default skill.",
                valid_name
            )
        } else {
            format!("Successfully deleted skill '{}'.", valid_name)
        };

        Ok(msg)
    }

    fn restore_embedded_fallback_if_exists(&mut self, name: &str) -> bool {
        for (default_name, raw_content) in BUNDLED_SKILLS {
            if *default_name == name {
                let (fm_name, description) = parse_skill_frontmatter(raw_content, default_name);
                self.skills.insert(
                    fm_name.clone(),
                    SkillMetadata {
                        name: fm_name,
                        description,
                        file_path: None,
                        embedded_body: Some(raw_content),
                        source: SkillSource::Embedded,
                    },
                );
                return true;
            }
        }
        false
    }
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn validate_skill_name(name: &str) -> std::result::Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }
    if trimmed.len() > 64 {
        return Err(format!(
            "Skill name exceeds 64 characters limit (was {})",
            trimmed.len()
        ));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err("Skill name cannot contain path separators ('/', '\\') or '..'".to_string());
    }
    let starts_valid = trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let chars_valid = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !starts_valid || !chars_valid {
        return Err("Invalid skill name. Must start with an alphanumeric character and contain only letters, digits, hyphens, underscores, or dots.".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn validate_skill_description(desc: &str) -> std::result::Result<String, String> {
    let trimmed = desc.trim();
    if trimmed.is_empty() {
        return Err("Skill description cannot be empty".to_string());
    }
    if trimmed.len() > 500 {
        return Err(format!(
            "Skill description exceeds 500 characters limit (was {})",
            trimmed.len()
        ));
    }
    Ok(trimmed.to_string())
}

fn parse_skill_frontmatter(content: &str, default_name: &str) -> (String, String) {
    let mut name = default_name.to_string();
    let mut description = format!("Skill {}", name);

    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end_frontmatter) = rest.find("---") {
            let frontmatter = &rest[..end_frontmatter];
            for line in frontmatter.lines() {
                let trimmed = line.trim();
                if let Some(n) = trimmed.strip_prefix("name:") {
                    name = n.trim().to_string();
                } else if let Some(d) = trimmed.strip_prefix("description:") {
                    description = d.trim().to_string();
                }
            }
        }
    }

    (name, description)
}

fn parse_skill_file(path: &Path, source: SkillSource) -> Result<SkillMetadata> {
    let content = std::fs::read_to_string(path).map_err(PurrError::Io)?;
    let mut fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    if fallback_name == "SKILL" {
        if let Some(parent) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
        {
            fallback_name = parent.to_string();
        }
    }

    let (name, description) = parse_skill_frontmatter(&content, &fallback_name);

    Ok(SkillMetadata {
        name,
        description,
        file_path: Some(path.to_path_buf()),
        embedded_body: None,
        source,
    })
}

fn dirs_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_embedded_skills_always_available() {
        let mgr = SkillManager::new();
        assert_eq!(mgr.list().len(), 8);
        let body = mgr.read_skill_body("git").unwrap();
        assert!(body.contains("Git Safety Skill"));
    }

    #[test]
    fn test_create_and_read_custom_skill() {
        let tmp = TempTestDir::new();
        let mut mgr = SkillManager::new();
        let (path, _) = mgr
            .create_skill(
                "docker-deploy",
                "Deployment guidelines for container environments",
                "# Docker Deploy\nRun compose up -d.",
                SkillScope::Project,
                tmp.path(),
            )
            .unwrap();

        assert!(path.exists());
        let meta = mgr.get("docker-deploy").unwrap();
        assert_eq!(meta.source, SkillSource::Project);
        assert_eq!(
            meta.description,
            "Deployment guidelines for container environments"
        );

        let body = mgr.read_skill_body("docker-deploy").unwrap();
        assert!(body.contains("Run compose up -d."));
        assert!(body.contains("name: docker-deploy"));
    }

    #[test]
    fn test_patch_skill_fuzzy_matching() {
        let tmp = TempTestDir::new();
        let mut mgr = SkillManager::new();
        mgr.create_skill(
            "custom-test",
            "Initial custom test skill",
            "# Test Skill\nStep 1: run cargo test\nStep 2: finish",
            SkillScope::Project,
            tmp.path(),
        )
        .unwrap();

        let (_path, diff, new_body) = mgr
            .patch_skill(
                "custom-test",
                "Step 1: run cargo test",
                "Step 1: run cargo test --workspace",
                None,
                tmp.path(),
            )
            .unwrap();

        assert!(new_body.contains("run cargo test --workspace"));
        assert!(diff.contains("-Step 1: run cargo test"));
        assert!(diff.contains("+Step 1: run cargo test --workspace"));
    }

    #[test]
    fn test_delete_custom_skill_and_embedded_protection() {
        let tmp = TempTestDir::new();
        let mut mgr = SkillManager::new();

        // Attempting to delete embedded bundled skill should fail
        let err = mgr.delete_skill("git", None, tmp.path()).unwrap_err();
        assert!(err
            .to_string()
            .contains("Cannot delete embedded bundled skill"));

        // Creating and deleting custom skill should succeed
        mgr.create_skill(
            "temp-skill",
            "Temporary skill description",
            "Temporary content",
            SkillScope::Project,
            tmp.path(),
        )
        .unwrap();
        assert!(mgr.get("temp-skill").is_some());

        let msg = mgr.delete_skill("temp-skill", None, tmp.path()).unwrap();
        assert!(msg.contains("Successfully deleted skill 'temp-skill'"));
        assert!(mgr.get("temp-skill").is_none());
    }

    #[test]
    fn test_skill_name_and_description_validation() {
        assert!(validate_skill_name("valid-skill_1").is_ok());
        assert!(validate_skill_name("../escape").is_err());
        assert!(validate_skill_name("has/slash").is_err());
        assert!(validate_skill_name("").is_err());

        assert!(validate_skill_description("A concise routing contract").is_ok());
        assert!(validate_skill_description("").is_err());
        let too_long = "a".repeat(501);
        assert!(validate_skill_description(&too_long).is_err());
    }
}
