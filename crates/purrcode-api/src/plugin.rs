// SPDX-License-Identifier: GPL-3.0-or-later

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// What a plugin manifest declares it needs.
///
/// Honesty note (v0.1): only `secrets` is enforced today — a process plugin
/// never receives a secret unless it is declared (`scrub_plugin_env`).
/// `network` and `filesystem` are declarations for the v0.3 OS sandbox
/// (Landlock/seccomp, PRD F21); in v0.1 they are parsed and recorded but
/// NOT enforced — a process plugin runs with full user rights. Callers
/// must surface that (see `PluginManager::unenforced_capabilities`).
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

#[async_trait]
pub trait PluginHookDispatcher: Send + Sync {
    async fn dispatch_session_start(&self, session_id: &str);
    async fn dispatch_pre_tool_use(&self, tool: &str, args: &serde_json::Value) -> bool;
    async fn dispatch_post_tool_use(&self, tool: &str, result: &serde_json::Value);
    async fn dispatch_session_stop(&self, session_id: &str);
}
