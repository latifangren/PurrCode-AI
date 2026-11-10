use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub network: Vec<String>,
    pub filesystem: Vec<String>,
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub transport: PluginTransport,
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginTransport {
    Builtin,
    Process,
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    async fn on_session_start(&self, session_id: &str) -> Result<()>;
    async fn pre_tool_use(&self, tool: &str, args: &serde_json::Value) -> Result<bool>;
    async fn post_tool_use(&self, tool: &str, result: &serde_json::Value) -> Result<()>;
    async fn on_session_stop(&self, session_id: &str) -> Result<()>;
}
