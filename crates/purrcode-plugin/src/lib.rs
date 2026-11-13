// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::{
    Plugin, PluginCapabilities, PluginHookDispatcher, PluginManifest, PluginMountError,
    PluginSource, PluginTransport,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub mod shared;
pub use shared::SharedPluginManager;

/// One plugin directory as discovered on disk (v0.4 B4). A
/// directory with a `plugin.yaml` is listed even when the manifest
/// does not parse, so the WebUI can show it as broken instead of
/// pretending it does not exist.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub dir: PathBuf,
    pub source: PluginSource,
    /// Manifest name, or the directory name when the manifest is broken.
    pub name: String,
    pub manifest: Option<PluginManifest>,
    pub command: Option<String>,
    pub manifest_error: Option<String>,
}

/// Discover every plugin directory under the global plugins dir
/// and the workspace `.purrcode/plugins` dir, global first (the
/// same order `discover_all` registers in, so the first match for
/// a name is the one startup would have registered).
pub fn discover_plugins(global_dir: &Path, workspace_root: &Path) -> Vec<DiscoveredPlugin> {
    let mut out = Vec::new();
    scan_plugin_dir(global_dir, PluginSource::Global, &mut out);
    scan_plugin_dir(
        &workspace_root.join(".purrcode").join("plugins"),
        PluginSource::Project,
        &mut out,
    );
    out
}

fn scan_plugin_dir(dir: &Path, source: PluginSource, out: &mut Vec<DiscoveredPlugin>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.yaml");
        if !manifest_path.exists() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        match parse_plugin_yaml(&manifest_path) {
            Ok((manifest, command)) => out.push(DiscoveredPlugin {
                name: manifest.name.clone(),
                dir: path,
                source,
                manifest: Some(manifest),
                command,
                manifest_error: None,
            }),
            Err(e) => out.push(DiscoveredPlugin {
                name: dir_name,
                dir: path,
                source,
                manifest: None,
                command: None,
                manifest_error: Some(e.to_string()),
            }),
        }
    }
}

/// Resolve a manifest `command` against its plugin directory when
/// it is relative and contains a path separator. Bare words go
/// through `PATH` lookup at spawn time.
fn resolve_plugin_command(command: &str, dir: &Path) -> String {
    let p = Path::new(command);
    if p.is_relative() && (command.contains('/') || command.contains('\\')) {
        dir.join(p).display().to_string()
    } else {
        command.to_string()
    }
}

/// Spawn a plugin child, retrying briefly on `ETXTBSY` ("Text
/// file busy"). That error is transient: a file just written (an
/// editor saving a plugin script, a test writing a fixture) can
/// still look busy to `execve` for a few milliseconds — reproduced
/// locally at ~10% under parallel load (400 write→spawn cycles,
/// 43 ETXTBSY). Retrying a handful of times turns a spurious
/// failure into a successful spawn; a genuinely un-executable
/// command still fails after the retries.
async fn spawn_with_txtbsy_retry(
    cmd: &mut Command,
) -> std::result::Result<tokio::process::Child, std::io::Error> {
    const ETXTBSY: i32 = 26;
    let mut last_err = None;
    for attempt in 0..4u8 {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e) if e.raw_os_error() == Some(ETXTBSY) && attempt < 3 => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.expect("retry loop always leaves an error"))
}

/// Kill a plugin child and its whole process group, then reap
/// it. A plugin that backgrounds grandchildren (e.g. `(sleep 30)
/// &`) would otherwise leak them past the RPC or the mount check:
/// killing only the direct child leaves the grandchild running
/// with no parent tracking it. The child is its own group leader
/// (`process_group(0)` at spawn), so the group id equals the pid.
/// On non-Unix platforms there is no process group, so the direct
/// kill is the only guard.
async fn kill_child_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            // SAFETY: killpg targets only the child's own fresh
            // group (group id == child pid); nothing else can be
            // in it.
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// Smoke check for a mount: the command must spawn, and a child
/// that dies immediately must have exited successfully. A bad
/// shebang or missing interpreter makes the child exit non-zero
/// (spawn itself succeeds — the exec failure happens inside the
/// child), so the exit status is the only signal. A child still
/// alive after the grace window is killed (whole process group)
/// and reaped (no zombie, no leaked fds, no leaked grandchildren);
/// a child that already exited 0 is a valid no-op
/// plugin — dispatch fails open for it, as today.
async fn verify_command_spawns(
    command: &str,
    name: &str,
) -> std::result::Result<(), PluginMountError> {
    let mut cmd = Command::new(command);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = spawn_with_txtbsy_retry(&mut cmd).await.map_err(|e| {
        PluginMountError::SpawnFailed(format!("'{command}' for plugin '{name}': {e}"))
    })?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    match child.try_wait() {
        Ok(Some(status)) if !status.success() => Err(PluginMountError::SpawnFailed(format!(
            "'{command}' for plugin '{name}' exited immediately with status {status}"
        ))),
        Ok(Some(_)) => Ok(()),
        // Still alive: kill the whole process group explicitly
        // and reap, instead of relying on `kill_on_drop` + the
        // orphan queue.
        Ok(None) => {
            kill_child_group(&mut child).await;
            Ok(())
        }
        Err(e) => Err(PluginMountError::SpawnFailed(format!(
            "could not check '{command}' for plugin '{name}': {e}"
        ))),
    }
}

/// One registered plugin plus the directory it was discovered
/// in. The directory is the plugin's identity for the WebUI list:
/// a global and a project plugin can share a manifest name, and
/// only the directory tells which copy is actually mounted
/// (v0.4 B4 fix pass — list by name alone marked both copies
/// mounted). Empty when registered directly without a discovery
/// context (ad-hoc `register` calls, tests).
struct PluginEntry {
    plugin: Arc<dyn Plugin>,
    dir: PathBuf,
}

pub struct PluginManager {
    plugins: Vec<PluginEntry>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins.push(PluginEntry {
            plugin,
            dir: PathBuf::new(),
        });
    }

    pub fn plugins(&self) -> Vec<Arc<dyn Plugin>> {
        self.plugins.iter().map(|e| e.plugin.clone()).collect()
    }

    /// Names of the currently registered (mounted) plugins.
    pub fn mounted_names(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|e| e.plugin.manifest().name.clone())
            .collect()
    }

    /// (name, discovery dir) of every mounted plugin — the WebUI
    /// matches a discovered entry against this pair, not the name
    /// alone, so a same-name global + project pair lists correctly.
    pub fn mounted_entries(&self) -> Vec<(String, PathBuf)> {
        self.plugins
            .iter()
            .map(|e| (e.plugin.manifest().name.clone(), e.dir.clone()))
            .collect()
    }

    pub fn is_mounted(&self, name: &str) -> bool {
        self.plugins
            .iter()
            .any(|e| e.plugin.manifest().name == name)
    }

    /// Validate a discovered plugin for mounting WITHOUT mutating
    /// anything: manifest must parse, a command must exist, the
    /// resolved command must spawn and not die immediately. This
    /// is the slow part of a mount (~50 ms spawn check), so the
    /// web wrapper runs it with no lock held and only takes the
    /// write lock to register afterwards (the `AlreadyMounted`
    /// check is re-run there). `mount` below keeps the same
    /// checks in the same order for direct callers.
    pub async fn verify_mount_candidate(
        discovered: &DiscoveredPlugin,
    ) -> std::result::Result<PluginManifest, PluginMountError> {
        let manifest = match &discovered.manifest {
            Some(m) => m.clone(),
            None => {
                return Err(PluginMountError::InvalidManifest(
                    discovered
                        .manifest_error
                        .clone()
                        .unwrap_or_else(|| "unparseable manifest".to_string()),
                ));
            }
        };
        let command = match &discovered.command {
            Some(c) => resolve_plugin_command(c, &discovered.dir),
            None => return Err(PluginMountError::NoCommand(manifest.name)),
        };
        verify_command_spawns(&command, &manifest.name).await?;
        Ok(manifest)
    }

    /// Mount one discovered plugin: validate the manifest, verify
    /// its command actually spawns, then register it. A relative
    /// command containing a path separator (`./runner`,
    /// `bin/hook`) is resolved against the plugin directory — the
    /// server's cwd is not the plugin's home, so a bare relative
    /// path would only work by accident. Bare words (`my-plugin`)
    /// are left alone for `PATH` lookup. An already-mounted name
    /// is rejected BEFORE the spawn check (re-mount is not a way
    /// to re-validate a plugin whose command broke on disk — it
    /// must be unmounted first).
    pub async fn mount(
        &mut self,
        discovered: &DiscoveredPlugin,
    ) -> std::result::Result<PluginManifest, PluginMountError> {
        if let Some(m) = &discovered.manifest {
            if self.is_mounted(&m.name) {
                return Err(PluginMountError::AlreadyMounted(m.name.clone()));
            }
        }
        let manifest = Self::verify_mount_candidate(discovered).await?;
        self.register_discovered(discovered);
        Ok(manifest)
    }

    /// Register one discovered plugin without the mount smoke
    /// checks (startup discovery path): skips broken manifests,
    /// command-less manifests, and names that are already mounted.
    /// The command is resolved against the plugin directory, like
    /// `mount` does.
    pub fn register_discovered(&mut self, discovered: &DiscoveredPlugin) {
        let Some(manifest) = &discovered.manifest else {
            return;
        };
        let Some(command) = &discovered.command else {
            return;
        };
        if self.is_mounted(&manifest.name) {
            return;
        }
        let command = resolve_plugin_command(command, &discovered.dir);
        self.plugins.push(PluginEntry {
            plugin: Arc::new(ProcessPlugin::new(manifest.clone(), command)),
            dir: discovered.dir.clone(),
        });
    }

    /// Remove a plugin from the live manager. Hooks already in
    /// flight hold their own `Arc` and finish; later dispatches
    /// skip it. There is no long-lived process to kill: each RPC
    /// spawns a fresh short-lived child (`kill_on_drop`).
    pub fn unmount(&mut self, name: &str) -> std::result::Result<PluginManifest, PluginMountError> {
        let pos = self
            .plugins
            .iter()
            .position(|e| e.plugin.manifest().name == name)
            .ok_or_else(|| PluginMountError::NotMounted(name.to_string()))?;
        let manifest = self.plugins.remove(pos).plugin.manifest().clone();
        Ok(manifest)
    }

    /// Loaded plugins whose manifest declares network/filesystem
    /// capabilities. Those are declarations for the v0.3 OS sandbox; in
    /// v0.1 only `secrets` is enforced. Callers should surface this (audit
    /// log / plugin list) so a user is not misled into thinking the plugin
    /// is sandboxed.
    pub fn unenforced_capabilities(&self) -> Vec<(String, Vec<&'static str>)> {
        self.plugins
            .iter()
            .filter_map(|e| {
                let caps = &e.plugin.manifest().capabilities;
                let mut kinds = Vec::new();
                if !caps.network.is_empty() {
                    kinds.push("network");
                }
                if !caps.filesystem.is_empty() {
                    kinds.push("filesystem");
                }
                if kinds.is_empty() {
                    None
                } else {
                    Some((e.plugin.manifest().name.clone(), kinds))
                }
            })
            .collect()
    }

    /// Discovers plugins from `~/.purrcode/plugins/` and `.purrcode/plugins/`.
    /// Broken manifests are skipped with a warning rather than crashing startup.
    pub fn discover_all(&mut self, workspace_root: &Path) {
        if let Some(home) = dirs_home() {
            self.scan_dir(
                &home.join(".purrcode").join("plugins"),
                PluginSource::Global,
            );
        }
        self.scan_dir(
            &workspace_root.join(".purrcode").join("plugins"),
            PluginSource::Project,
        );
    }

    fn scan_dir(&mut self, dir: &Path, source: PluginSource) {
        if !dir.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let plugin_dir = entry.path();
                let manifest_path = plugin_dir.join("plugin.yaml");
                if !manifest_path.exists() {
                    continue;
                }
                match parse_plugin_yaml(&manifest_path) {
                    Ok((manifest, command)) => {
                        // Same path as the web mount (`register_discovered`):
                        // a relative command is resolved against the
                        // plugin directory, and a name already
                        // registered (global wins over project) is
                        // skipped. Registering the raw command here
                        // used to mean `./hook.sh` only spawned when
                        // the process cwd happened to be the plugin
                        // dir — dispatch then vetoed every tool call
                        // on the ENOENT error, with no diagnosis.
                        self.register_discovered(&DiscoveredPlugin {
                            dir: plugin_dir,
                            source,
                            name: manifest.name.clone(),
                            manifest: Some(manifest),
                            command,
                            manifest_error: None,
                        });
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

    pub async fn dispatch_session_start(&self, session_id: &str) {
        for e in &self.plugins {
            let _ = e.plugin.on_session_start(session_id).await;
        }
    }

    /// Dispatches `pre_tool_use` to all registered plugins. Any `Ok(false)` vetoes execution.
    /// A plugin that ERRORS also vetoes (fail-closed) — but unlike
    /// before, the failure is logged with the plugin name: a
    /// silently vetoing broken plugin used to block every tool
    /// call with no diagnosis anywhere.
    pub async fn dispatch_pre_tool_use(&self, tool: &str, args: &Value) -> bool {
        for e in &self.plugins {
            match e.plugin.pre_tool_use(tool, args).await {
                Ok(true) => {}
                Ok(false) => return false,
                Err(err) => {
                    tracing::warn!(
                        "Plugin '{}' pre_tool_use failed ({}); vetoing tool call '{}'",
                        e.plugin.manifest().name,
                        err,
                        tool
                    );
                    return false;
                }
            }
        }
        true
    }

    pub async fn dispatch_post_tool_use(&self, tool: &str, result: &Value) {
        for e in &self.plugins {
            let _ = e.plugin.post_tool_use(tool, result).await;
        }
    }

    pub async fn dispatch_session_stop(&self, session_id: &str) {
        for e in &self.plugins {
            let _ = e.plugin.on_session_stop(session_id).await;
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
        // Honesty note (v0.1): network/filesystem capabilities are
        // declarations for the v0.3 OS sandbox only. Only `secrets` is
        // enforced today (env scrubbing); a process plugin runs with full
        // user rights. Warn here (tracing channel, same as the invalid
        // manifest warning) and let callers surface it in the audit log
        // / plugin list via `PluginManager::unenforced_capabilities`.
        if !manifest.capabilities.network.is_empty() || !manifest.capabilities.filesystem.is_empty()
        {
            tracing::warn!(
                "Plugin '{}' declares network/filesystem capabilities, but they are NOT enforced in v0.1: process plugins run with full user rights. Enforcement comes with the v0.3 OS sandbox.",
                manifest.name
            );
        }
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
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = spawn_with_txtbsy_retry(&mut cmd)
            .await
            .map_err(|e| PurrError::Plugin {
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

        let result = if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::new(stdout);
            let mut resp_line = String::new();
            if reader.read_line(&mut resp_line).await.is_ok() {
                serde_json::from_str::<Value>(resp_line.trim())
                    .ok()
                    .and_then(|val| val.get("result").cloned())
            } else {
                None
            }
        } else {
            None
        };
        // The RPC is one-shot: the child is killed (whole process
        // group) as soon as its response is read, so a plugin must
        // finish its side effects BEFORE responding — nothing after
        // the response line is guaranteed to run.
        kill_child_group(&mut child).await;
        Ok(result.unwrap_or(Value::Bool(true)))
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
    let mut network = Vec::new();
    let mut filesystem = Vec::new();

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
        } else if let Some(v) = trimmed.strip_prefix("network:") {
            // Recorded as a declaration for the v0.3 OS sandbox; NOT
            // enforced in v0.1 (see `PluginManager::unenforced_capabilities`).
            let raw_list = v.trim().trim_start_matches('[').trim_end_matches(']');
            for item in raw_list.split(',') {
                let s = item.trim().trim_matches('"').trim_matches('\'');
                if !s.is_empty() {
                    network.push(s.to_string());
                }
            }
        } else if let Some(v) = trimmed.strip_prefix("filesystem:") {
            let raw_list = v.trim().trim_start_matches('[').trim_end_matches(']');
            for item in raw_list.split(',') {
                let s = item.trim().trim_matches('"').trim_matches('\'');
                if !s.is_empty() {
                    filesystem.push(s.to_string());
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
                network,
                filesystem,
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
mod tests;
