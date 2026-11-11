// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::Result;
use purrcode_api::memory::{MemoryBackend, MemoryFact};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

pub struct MarkdownMemory {
    root_path: PathBuf,
    last_mtime: Mutex<Option<SystemTime>>,
}

impl MarkdownMemory {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
            last_mtime: Mutex::new(None),
        }
    }

    pub fn memory_file(&self) -> PathBuf {
        self.root_path.join("MEMORY.md")
    }

    pub fn user_file(&self) -> PathBuf {
        self.root_path.join("USER.md")
    }

    pub fn soul_file(&self) -> PathBuf {
        self.root_path.join("SOUL.md")
    }

    /// Checks whether `MEMORY.md` has been modified on disk since the last check (hot-reload via mtime).
    pub fn has_changed_on_disk(&self) -> bool {
        let current_mtime = std::fs::metadata(self.memory_file())
            .and_then(|m| m.modified())
            .ok();
        if let Ok(mut guard) = self.last_mtime.lock() {
            if *guard != current_mtime {
                *guard = current_mtime;
                return true;
            }
        }
        false
    }

    /// Loads combined memory context (`SOUL.md`, `USER.md`, `MEMORY.md`, and `.purrcode/MEMORY.md`)
    /// bounded to ~2200 characters per section to conserve context headroom.
    pub async fn load_context(&self, local_project_dir: Option<&Path>) -> String {
        let _ = self.has_changed_on_disk();
        let mut context = String::new();

        // 1. Persona & Tone (SOUL.md)
        if let Ok(soul_content) = tokio::fs::read_to_string(self.soul_file()).await {
            if !soul_content.trim().is_empty() {
                context.push_str("\n### Persona & Soul (SOUL.md)\n");
                context.push_str(truncate_chars(soul_content.trim(), 1375));
                context.push('\n');
            }
        }

        // 2. User preferences (USER.md)
        if let Ok(user_content) = tokio::fs::read_to_string(self.user_file()).await {
            if !user_content.trim().is_empty() {
                context.push_str("\n### User Information & Preferences (USER.md)\n");
                context.push_str(truncate_chars(user_content.trim(), 1375));
                context.push('\n');
            }
        }

        // 3. Global Curated Memory (MEMORY.md)
        if let Ok(mem_content) = tokio::fs::read_to_string(self.memory_file()).await {
            if !mem_content.trim().is_empty() {
                context.push_str("\n### Curated Memory (MEMORY.md)\n");
                context.push_str(truncate_chars(mem_content.trim(), 2200));
                context.push('\n');
            }
        }

        // 4. Project-local Memory (.purrcode/MEMORY.md) — wins over global when present
        if let Some(proj) = local_project_dir {
            let local_mem = proj.join(".purrcode").join("MEMORY.md");
            if let Ok(local_content) = tokio::fs::read_to_string(local_mem).await {
                if !local_content.trim().is_empty() {
                    context.push_str("\n### Project Memory (.purrcode/MEMORY.md)\n");
                    context.push_str(truncate_chars(local_content.trim(), 2200));
                    context.push('\n');
                }
            }
        }

        context
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[async_trait]
impl MemoryBackend for MarkdownMemory {
    fn name(&self) -> &str {
        "markdown"
    }

    async fn remember(&self, fact: MemoryFact) -> Result<()> {
        let path = self.memory_file();
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let mut existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        if existing.is_empty() {
            existing.push_str("# PurrCode Memory\n\n");
        }

        let id_tag = if fact.id.is_empty() {
            uuid::Uuid::new_v4().to_string()[..8].to_string()
        } else {
            fact.id
        };
        let entry = format!("- ({}) [{}]: {}\n", id_tag, fact.category, fact.content);
        existing.push_str(&entry);

        tokio::fs::write(&path, existing.as_bytes())
            .await
            .map_err(purrcode_api::PurrError::Io)?;

        Ok(())
    }

    async fn recall(&self, query: &str, limit: usize) -> Result<Vec<MemoryFact>> {
        let path = self.memory_file();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(purrcode_api::PurrError::Io)?;
        let q = query.to_lowercase();
        let mut facts = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('-') && trimmed.to_lowercase().contains(&q) {
                let id = extract_fact_id(trimmed).unwrap_or_else(|| format!("line_{}", idx + 1));
                facts.push(MemoryFact {
                    id,
                    category: "general".to_string(),
                    content: trimmed.trim_start_matches('-').trim().to_string(),
                    source: Some("MEMORY.md".to_string()),
                });
                if facts.len() >= limit {
                    break;
                }
            }
        }

        Ok(facts)
    }

    async fn forget(&self, id: &str) -> Result<()> {
        let path = self.memory_file();
        if !path.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(purrcode_api::PurrError::Io)?;
        let kept: Vec<&str> = content
            .lines()
            .filter(|line| {
                let t = line.trim();
                if t.starts_with('-') {
                    extract_fact_id(t).as_deref() != Some(id)
                } else {
                    true
                }
            })
            .collect();

        let new_content = kept.join("\n") + "\n";
        tokio::fs::write(&path, new_content.as_bytes())
            .await
            .map_err(purrcode_api::PurrError::Io)?;
        Ok(())
    }

    async fn export_markdown(&self) -> Result<String> {
        if self.memory_file().exists() {
            tokio::fs::read_to_string(self.memory_file())
                .await
                .map_err(purrcode_api::PurrError::Io)
        } else {
            Ok(String::new())
        }
    }
}

fn extract_fact_id(line: &str) -> Option<String> {
    let rest = line.trim_start_matches('-').trim();
    if let Some(after_open) = rest.strip_prefix('(') {
        if let Some(close_idx) = after_open.find(')') {
            return Some(after_open[..close_idx].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_markdown_memory_remember_recall_and_forget() {
        let temp_dir = std::env::temp_dir().join(format!("purr_mem_{}", uuid::Uuid::new_v4()));
        let mem = MarkdownMemory::new(&temp_dir);

        mem.remember(MemoryFact {
            id: "fact_01".to_string(),
            category: "preference".to_string(),
            content: "User prefers Rust with musl static builds".to_string(),
            source: None,
        })
        .await
        .unwrap();

        mem.remember(MemoryFact {
            id: "fact_010".to_string(),
            category: "preference".to_string(),
            content: "Keep fact_010 intact when deleting fact_01".to_string(),
            source: None,
        })
        .await
        .unwrap();

        assert!(mem.has_changed_on_disk());
        let recalled = mem.recall("musl", 5).await.unwrap();
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].id, "fact_01");

        mem.forget("fact_01").await.unwrap();
        let after_forget = mem.recall("musl", 5).await.unwrap();
        assert!(after_forget.is_empty());

        let remaining = mem.recall("fact_010", 5).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "fact_010");

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
