use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: String,
    pub category: String,
    pub content: String,
    pub source: Option<String>,
}

#[async_trait]
pub trait MemoryBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn remember(&self, fact: MemoryFact) -> Result<()>;
    async fn recall(&self, query: &str, limit: usize) -> Result<Vec<MemoryFact>>;
    async fn forget(&self, id: &str) -> Result<()>;
    async fn export_markdown(&self) -> Result<String>;
}
