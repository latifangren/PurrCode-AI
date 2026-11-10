use purrcode_api::error::Result;
use purrcode_api::memory::{MemoryBackend, MemoryFact};
use async_trait::async_trait;
use std::path::PathBuf;

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
}

#[async_trait]
impl MemoryBackend for MarkdownMemory {
    fn name(&self) -> &str {
        "markdown"
    }

    async fn remember(&self, _fact: MemoryFact) -> Result<()> {
        // Appends fact to MEMORY.md
        Ok(())
    }

    async fn recall(&self, _query: &str, _limit: usize) -> Result<Vec<MemoryFact>> {
        // Scans MEMORY.md
        Ok(Vec::new())
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
