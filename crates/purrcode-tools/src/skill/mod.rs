pub mod tool;

pub use tool::ReadSkillTool;
use purrcode_api::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
}

pub struct SkillManager {
    skills: HashMap<String, SkillMetadata>,
}

impl SkillManager {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Discovers skills from bundled path, global user path, and local project path
    pub fn discover_all(&mut self, workspace_root: &Path) {
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
        self.skills.values().collect()
    }

    /// Formats a concise index of available skills for the system prompt (~50-100 tokens)
    pub fn format_prompt_index(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        lines.push("<available-skills>".to_string());
        for (name, meta) in &self.skills {
            lines.push(format!("- {}: {}", name, meta.description));
        }
        lines.push("Call tool 'read_skill' to inspect any skill's full instructions on-demand.".to_string());
        lines.push("</available-skills>".to_string());

        lines.join("\n")
    }

    pub fn read_skill_body(&self, name: &str) -> Result<String> {
        if let Some(meta) = self.skills.get(name) {
            let content = std::fs::read_to_string(&meta.file_path)
                .map_err(purrcode_api::PurrError::Io)?;
            Ok(content)
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

fn parse_skill_file(path: &Path) -> Result<SkillMetadata> {
    let content = std::fs::read_to_string(path).map_err(purrcode_api::PurrError::Io)?;
    let mut name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
    if name == "SKILL" {
        if let Some(parent) = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
            name = parent.to_string();
        }
    }

    let mut description = format!("Skill {}", name);

    if content.starts_with("---") {
        if let Some(end_frontmatter) = content[3..].find("---") {
            let frontmatter = &content[3..3 + end_frontmatter];
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

    Ok(SkillMetadata {
        name,
        description,
        file_path: path.to_path_buf(),
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
