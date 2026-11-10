use purrcode_api::error::Result;
use purrcode_api::memory::{MemoryBackend, MemoryFact};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct MarkdownMemory {
    root_path: PathBuf,
}

impl MarkdownMemory {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
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

    /// Loads combined memory context to inject into LLM system prompt
    pub async fn load_context(&self, local_project_dir: Option<&Path>) -> String {
        let mut context = String::new();

        // 1. User preferences (USER.md)
        if let Ok(user_content) = tokio::fs::read_to_string(self.user_file()).await {
            if !user_content.trim().is_empty() {
                context.push_str("\n### User Information & Preferences (USER.md)\n");
                context.push_str(user_content.trim());
                context.push('\n');
            }
        }

        // 2. Global Curated Memory (MEMORY.md)
        if let Ok(mem_content) = tokio::fs::read_to_string(self.memory_file()).await {
            if !mem_content.trim().is_empty() {
                context.push_str("\n### Curated Memory (MEMORY.md)\n");
                context.push_str(mem_content.trim());
                context.push('\n');
            }
        }

        // 3. Project-local Memory (.purrcode/MEMORY.md)
        if let Some(proj) = local_project_dir {
            let local_mem = proj.join(".purrcode").join("MEMORY.md");
            if let Ok(local_content) = tokio::fs::read_to_string(local_mem).await {
                if !local_content.trim().is_empty() {
                    context.push_str("\n### Project Memory (.purrcode/MEMORY.md)\n");
                    context.push_str(local_content.trim());
                    context.push('\n');
                }
            }
        }

        context
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

        let entry = format!("- [{}]: {}\n", fact.category, fact.content);
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

        let content = tokio::fs::read_to_string(&path).await.map_err(purrcode_api::PurrError::Io)?;
        let q = query.to_lowercase();
        let mut facts = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('-') && trimmed.to_lowercase().contains(&q) {
                facts.push(MemoryFact {
                    id: uuid::Uuid::new_v4().to_string(),
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

    async fn forget(&self, _id: &str) -> Result<()> {
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
