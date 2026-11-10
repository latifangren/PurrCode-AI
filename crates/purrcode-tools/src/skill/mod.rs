// SPDX-License-Identifier: GPL-3.0-or-later

pub mod tool;

use purrcode_api::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub file_path: Option<PathBuf>,
    pub embedded_body: Option<&'static str>,
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
                },
            );
        }
    }

    /// Discovers skills from bundled path, global user path, and local project path
    pub fn discover_all(&mut self, workspace_root: &Path) {
        // 0. Embedded bundled skills are already loaded as base fallback
        self.load_embedded_defaults();

        // 1. Bundled skills in workspace/skills/
        self.scan_directory(&workspace_root.join("skills"));

        // 2. User home ~/.purrcode/skills/
        if let Some(home) = dirs_home() {
            self.scan_directory(&home.join(".purrcode").join("skills"));
        }

        // 3. Project-local .purrcode/skills/ (highest precedence)
        self.scan_directory(&workspace_root.join(".purrcode").join("skills"));
    }

    fn scan_directory(&mut self, base_dir: &Path) {
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
                    if let Ok(meta) = parse_skill_file(&skill_file) {
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
            Err(purrcode_api::PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Skill '{}' has no readable source", name),
            })
        } else {
            Err(purrcode_api::PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Skill '{}' not found", name),
            })
        }
    }
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
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

fn parse_skill_file(path: &Path) -> Result<SkillMetadata> {
    let content = std::fs::read_to_string(path).map_err(purrcode_api::PurrError::Io)?;
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

    #[test]
    fn test_embedded_skills_always_available() {
        let mgr = SkillManager::new();
        assert_eq!(mgr.list().len(), 8);
        let body = mgr.read_skill_body("git").unwrap();
        assert!(body.contains("Git Safety Skill"));
    }
}
