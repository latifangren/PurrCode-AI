// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::{
    Plugin, PluginCapabilities, PluginHookDispatcher, PluginManifest, PluginTransport,
};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub struct PluginManager {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    /// Discovers plugins from `~/.purrcode/plugins/` and `.purrcode/plugins/`.
    /// Broken manifests are skipped with a warning rather than crashing startup.
    pub fn discover_all(&mut self, workspace_root: &Path) {
        if let Some(home) = dirs_home() {
            self.scan_dir(&home.join(".purrcode").join("plugins"));
        }
        self.scan_dir(&workspace_root.join(".purrcode").join("plugins"));
    }

    fn scan_dir(&mut self, dir: &Path) {
        if !dir.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let manifest_path = entry.path().join("plugin.yaml");
                if manifest_path.exists() {
                    match parse_plugin_yaml(&manifest_path) {
                        Ok((manifest, command)) => {
                            if let Some(cmd) = command {
                                self.register(Arc::new(ProcessPlugin::new(manifest, cmd)));
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Skipping invalid plugin manifest '{}': {}",
                                manifest_path.display(),
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    pub async fn dispatch_session_start(&self, session_id: &str) {
        for p in &self.plugins {
            let _ = p.on_session_start(session_id).await;
        }
    }

    /// Dispatches `pre_tool_use` to all registered plugins. Any `Ok(false)` vetoes execution.
    pub async fn dispatch_pre_tool_use(&self, tool: &str, args: &Value) -> bool {
        for p in &self.plugins {
            match p.pre_tool_use(tool, args).await {
                Ok(true) => {}
                Ok(false) | Err(_) => return false,
            }
        }
        true
    }

    pub async fn dispatch_post_tool_use(&self, tool: &str, result: &Value) {
        for p in &self.plugins {
            let _ = p.post_tool_use(tool, result).await;
        }
    }

    pub async fn dispatch_session_stop(&self, session_id: &str) {
        for p in &self.plugins {
            let _ = p.on_session_stop(session_id).await;
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PluginHookDispatcher for PluginManager {
    async fn dispatch_session_start(&self, session_id: &str) {
        PluginManager::dispatch_session_start(self, session_id).await;
    }

    async fn dispatch_pre_tool_use(&self, tool: &str, args: &Value) -> bool {
        PluginManager::dispatch_pre_tool_use(self, tool, args).await
    }

    async fn dispatch_post_tool_use(&self, tool: &str, result: &Value) {
        PluginManager::dispatch_post_tool_use(self, tool, result).await;
    }

    async fn dispatch_session_stop(&self, session_id: &str) {
        PluginManager::dispatch_session_stop(self, session_id).await;
    }
}

/// External process plugin communicating over JSON-RPC 2.0 stdio with scrubbed environment
pub struct ProcessPlugin {
    manifest: PluginManifest,
    command: String,
}

impl ProcessPlugin {
    pub fn new(manifest: PluginManifest, command: impl Into<String>) -> Self {
        Self {
            manifest,
            command: command.into(),
        }
    }

    /// Checks default-deny capability enforcement: secrets are never passed unless declared.
    pub fn is_secret_allowed(&self, secret_name: &str) -> bool {
        self.manifest
            .capabilities
            .secrets
            .iter()
            .any(|s| s == secret_name)
    }

    async fn call_rpc(&self, method: &str, params: Value) -> Result<Value> {
        let mut cmd = Command::new(&self.command);
        scrub_plugin_env(&mut cmd, &self.manifest.capabilities);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| PurrError::Plugin {
            plugin: self.manifest.name.clone(),
            message: format!("Failed to spawn process plugin: {e}"),
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            let req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params
            });
            let line = serde_json::to_string(&req)? + "\n";
            let _ = stdin.write_all(line.as_bytes()).await;
            let _ = stdin.flush().await;
        }

        if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::new(stdout);
            let mut resp_line = String::new();
            if reader.read_line(&mut resp_line).await.is_ok() {
                if let Ok(val) = serde_json::from_str::<Value>(resp_line.trim()) {
                    return Ok(val.get("result").cloned().unwrap_or(Value::Bool(true)));
                }
            }
        }

        Ok(Value::Bool(true))
    }
}

#[async_trait]
impl Plugin for ProcessPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn on_session_start(&self, session_id: &str) -> Result<()> {
        let _ = self
            .call_rpc(
                "on_session_start",
                serde_json::json!({ "session_id": session_id }),
            )
            .await?;
        Ok(())
    }

    async fn pre_tool_use(&self, tool: &str, args: &Value) -> Result<bool> {
        let res = self
            .call_rpc(
                "pre_tool_use",
                serde_json::json!({ "tool": tool, "args": args }),
            )
            .await?;
        Ok(res.as_bool().unwrap_or(true))
    }

    async fn post_tool_use(&self, tool: &str, result: &Value) -> Result<()> {
        let _ = self
            .call_rpc(
                "post_tool_use",
                serde_json::json!({ "tool": tool, "result": result }),
            )
            .await?;
        Ok(())
    }

    async fn on_session_stop(&self, session_id: &str) -> Result<()> {
        let _ = self
            .call_rpc(
                "on_session_stop",
                serde_json::json!({ "session_id": session_id }),
            )
            .await?;
        Ok(())
    }
}

fn scrub_plugin_env(cmd: &mut Command, caps: &PluginCapabilities) {
    for (key, _) in std::env::vars_os() {
        if let Some(k_str) = key.to_str() {
            let upper = k_str.to_ascii_uppercase();
            let is_secret = upper.starts_with("PURRCODE_")
                || upper.ends_with("_API_KEY")
                || upper.ends_with("_SECRET")
                || upper.ends_with("_TOKEN");
            if is_secret && !caps.secrets.iter().any(|allowed| allowed == k_str) {
                cmd.env_remove(&key);
            }
        }
    }
}

/// Parses a minimal `plugin.yaml` manifest without pulling a heavy YAML parser dependency
pub fn parse_plugin_yaml(path: &Path) -> Result<(PluginManifest, Option<String>)> {
    let content = std::fs::read_to_string(path).map_err(PurrError::Io)?;
    parse_plugin_yaml_str(&content)
}

pub fn parse_plugin_yaml_str(content: &str) -> Result<(PluginManifest, Option<String>)> {
    let mut name = String::new();
    let mut version = "0.1.0".to_string();
    let mut description = None;
    let mut transport = PluginTransport::Builtin;
    let mut command = None;
    let mut secrets = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(v) = trimmed.strip_prefix("name:") {
            name = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = trimmed.strip_prefix("version:") {
            version = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = trimmed.strip_prefix("description:") {
            description = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = trimmed.strip_prefix("transport:") {
            if v.trim().starts_with("process") {
                transport = PluginTransport::Process;
            }
        } else if let Some(v) = trimmed.strip_prefix("command:") {
            command = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = trimmed.strip_prefix("secrets:") {
            let raw_list = v.trim().trim_start_matches('[').trim_end_matches(']');
            for item in raw_list.split(',') {
                let s = item.trim().trim_matches('"').trim_matches('\'');
                if !s.is_empty() {
                    secrets.push(s.to_string());
                }
            }
        }
    }

    if name.is_empty() {
        return Err(PurrError::Plugin {
            plugin: "unknown".to_string(),
            message: "Missing 'name' in plugin.yaml".to_string(),
        });
    }

    Ok((
        PluginManifest {
            name,
            version,
            description,
            transport,
            capabilities: PluginCapabilities {
                network: Vec::new(),
                filesystem: Vec::new(),
                secrets,
            },
        },
        command,
    ))
}

fn dirs_home() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(std::path::PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugin_yaml_and_default_deny_secrets() {
        let yaml = r#"
name: security-hook
version: 0.1.0
transport: process
command: ./my-plugin
"#;
        let (manifest, cmd) = parse_plugin_yaml_str(yaml).unwrap();
        assert_eq!(manifest.name, "security-hook");
        assert_eq!(manifest.transport, PluginTransport::Process);
        assert_eq!(cmd.as_deref(), Some("./my-plugin"));
        let p = ProcessPlugin::new(manifest, "./my-plugin");
        assert!(!p.is_secret_allowed("OPENAI_API_KEY"));
    }
}
