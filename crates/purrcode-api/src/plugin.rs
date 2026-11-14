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

/// Where a plugin was discovered from. Global wins over project
/// when two manifests share a name (see `PluginRegistry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginSource {
    Global,
    Project,
}

/// One plugin as the WebUI sees it: the manifest fields, where it
/// lives, and whether it is currently mounted (registered in the
/// live manager, so hook dispatches reach it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub transport: PluginTransport,
    pub capabilities: PluginCapabilities,
    pub source: PluginSource,
    /// Plugin directory (the folder holding `plugin.yaml`).
    pub path: String,
    pub mounted: bool,
    /// False when the manifest could not be parsed; `name` is then
    /// the directory name and the other manifest fields are empty.
    pub manifest_ok: bool,
    pub manifest_error: Option<String>,
}

/// Why a mount/unmount was refused. The WebUI maps these to HTTP
/// statuses: `NotFound` → 404, `AlreadyMounted` / `NotMounted` →
/// 409, the rest → 400 with this message as the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginMountError {
    NotFound(String),
    AlreadyMounted(String),
    NotMounted(String),
    InvalidManifest(String),
    NoCommand(String),
    SpawnFailed(String),
}

impl std::fmt::Display for PluginMountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "no plugin named '{name}'"),
            Self::AlreadyMounted(name) => write!(f, "plugin '{name}' is already mounted"),
            Self::NotMounted(name) => write!(f, "plugin '{name}' is not mounted"),
            Self::InvalidManifest(msg) => write!(f, "invalid plugin manifest: {msg}"),
            Self::NoCommand(name) => write!(
                f,
                "plugin '{name}' has no command to run (builtin plugins cannot be mounted)"
            ),
            Self::SpawnFailed(msg) => write!(f, "plugin command failed to start: {msg}"),
        }
    }
}

impl std::error::Error for PluginMountError {}

/// The live plugin registry the WebUI mounts/unmounts against
/// (v0.4 B4). Implemented by `SharedPluginManager` in
/// `purrcode-plugin`; the channel crate only ever sees this trait,
/// so the crate chain stays one-way (`channel → api ← plugin`).
///
/// Discovery order is global (`<purrcode_dir>/plugins`) first,
/// then project (`<workspace>/.purrcode/plugins`); when two
/// manifests share a name, the first match wins the mount.
#[async_trait]
pub trait PluginRegistry: Send + Sync {
    /// Every discovered plugin (parseable or broken) with its live
    /// mounted state.
    async fn list_plugins(&self) -> Vec<PluginInfo>;
    /// Parse the manifest, verify the command spawns, and register
    /// the plugin so subsequent hook dispatches reach it.
    async fn mount_plugin(&self, name: &str) -> std::result::Result<PluginInfo, PluginMountError>;
    /// Remove the plugin from the live manager. Hooks already in
    /// flight hold their own `Arc` and finish; later dispatches
    /// skip it.
    async fn unmount_plugin(&self, name: &str)
        -> std::result::Result<PluginInfo, PluginMountError>;
}
