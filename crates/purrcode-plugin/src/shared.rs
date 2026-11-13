// SPDX-License-Identifier: GPL-3.0-or-later

//! The shareable plugin manager for the WebUI (v0.4 B4).
//!
//! `PluginManager` itself is a plain `Vec` behind `&mut self` —
//! fine for the CLI binaries, which build it once at startup. The
//! web server needs the same manager mutated at runtime (mount /
//! unmount from the browser) while turns dispatch hooks against it
//! concurrently, so this wrapper holds it behind a tokio `RwLock`
//! and implements both `PluginHookDispatcher` (the loops' view) and
//! `PluginRegistry` (the WebUI's view) over the same instance.
//!
//! Lifecycle note: there is no long-lived plugin process to kill.
//! Each hook RPC spawns a fresh short-lived child (`kill_on_drop`)
//! and dispatch snapshots the plugin list under a read lock, so
//! unmount never tears down a hook already in flight — the
//! in-flight call holds its own `Arc` and finishes, and later
//! dispatches skip the plugin. Mount verifies the command spawns
//! before registering, so a broken plugin cannot be mounted into
//! a state where it would veto every tool call at dispatch.

use crate::{discover_plugins, DiscoveredPlugin, PluginManager};
use purrcode_api::plugin::{
    Plugin, PluginHookDispatcher, PluginInfo, PluginMountError, PluginRegistry,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The live plugin manager shared between the web server's
/// mount/unmount endpoints and the agent loops' hook dispatcher.
#[derive(Clone)]
pub struct SharedPluginManager {
    inner: Arc<RwLock<PluginManager>>,
    global_dir: PathBuf,
    workspace_root: PathBuf,
}

impl SharedPluginManager {
    pub fn new(global_dir: impl Into<PathBuf>, workspace_root: impl Into<PathBuf>) -> Self {
        Self::from_manager(PluginManager::new(), global_dir, workspace_root)
    }

    pub fn from_manager(
        manager: PluginManager,
        global_dir: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(manager)),
            global_dir: global_dir.into(),
            workspace_root: workspace_root.into(),
        }
    }

    /// Startup discovery, same set of directories `discover_all`
    /// scans (global plugins dir, then the workspace
    /// `.purrcode/plugins` dir) — but through the shared
    /// discovery, so the global dir is the one this manager was
    /// constructed with, not whatever `HOME` happens to be.
    /// Broken manifests are skipped with a warning, as in
    /// `PluginManager::discover_all`; unlike `mount`, no spawn
    /// check runs at startup.
    pub async fn discover_all(&self) {
        let discovered = self.discover().await;
        let mut mgr = self.inner.write().await;
        for d in &discovered {
            mgr.register_discovered(d);
        }
    }

    /// Snapshot the currently mounted plugins (cheap `Arc` clones).
    /// Dispatch runs against the snapshot, never under the lock, so
    /// a mount/unmount is never blocked by a slow hook RPC.
    async fn snapshot(&self) -> Vec<Arc<dyn Plugin>> {
        self.inner.read().await.plugins()
    }

    pub async fn is_mounted(&self, name: &str) -> bool {
        self.inner.read().await.is_mounted(name)
    }

    /// Live unenforced-capability report (same shape as
    /// `PluginManager::unenforced_capabilities`); read at session
    /// start so a plugin mounted after startup is reported too.
    pub async fn unenforced_capabilities(&self) -> Vec<(String, Vec<&'static str>)> {
        self.inner.read().await.unenforced_capabilities()
    }

    fn info(discovered: &DiscoveredPlugin, mounted: bool) -> PluginInfo {
        match &discovered.manifest {
            Some(m) => PluginInfo {
                name: m.name.clone(),
                version: m.version.clone(),
                description: m.description.clone(),
                transport: m.transport,
                capabilities: m.capabilities.clone(),
                source: discovered.source,
                path: discovered.dir.display().to_string(),
                mounted,
                manifest_ok: true,
                manifest_error: None,
            },
            None => PluginInfo {
                name: discovered.name.clone(),
                version: String::new(),
                description: None,
                transport: purrcode_api::plugin::PluginTransport::Process,
                capabilities: purrcode_api::plugin::PluginCapabilities {
                    network: Vec::new(),
                    filesystem: Vec::new(),
                    secrets: Vec::new(),
                },
                source: discovered.source,
                path: discovered.dir.display().to_string(),
                mounted,
                manifest_ok: false,
                manifest_error: discovered.manifest_error.clone(),
            },
        }
    }

    async fn discover(&self) -> Vec<DiscoveredPlugin> {
        // Disk reads are blocking syscalls; the scan is small (a
        // handful of dirs), so no spawn_blocking here.
        discover_plugins(&self.global_dir, &self.workspace_root)
    }
}

#[async_trait::async_trait]
impl PluginRegistry for SharedPluginManager {
    async fn list_plugins(&self) -> Vec<PluginInfo> {
        let discovered = self.discover().await;
        // Match a discovered entry against (name, dir), not the
        // name alone: a global and a project plugin can share a
        // manifest name, and only the mounted copy's directory
        // tells which row is actually live.
        let mounted = self.inner.read().await.mounted_entries();
        discovered
            .iter()
            .map(|d| {
                let is_mounted = d
                    .manifest
                    .as_ref()
                    .map(|m| mounted.iter().any(|(n, dir)| n == &m.name && dir == &d.dir))
                    .unwrap_or(false);
                Self::info(d, is_mounted)
            })
            .collect()
    }

    async fn mount_plugin(&self, name: &str) -> std::result::Result<PluginInfo, PluginMountError> {
        let discovered = self.discover().await;
        let found = discovered
            .iter()
            .find(|d| d.manifest.as_ref().map(|m| m.name.as_str()) == Some(name))
            .ok_or_else(|| PluginMountError::NotFound(name.to_string()))?;
        // Advisory fast path: an already-mounted name is rejected
        // before the ~50 ms spawn check runs.
        if self.is_mounted(name).await {
            return Err(PluginMountError::AlreadyMounted(name.to_string()));
        }
        // The spawn check runs with NO lock held (fix pass P3-4):
        // holding the write lock across it stalled every dispatch
        // snapshot and list call for its duration.
        let manifest = PluginManager::verify_mount_candidate(found).await?;
        let mut mgr = self.inner.write().await;
        // Authoritative re-check: another mount may have won the
        // race while the spawn check was running.
        if mgr.is_mounted(&manifest.name) {
            return Err(PluginMountError::AlreadyMounted(manifest.name));
        }
        mgr.register_discovered(found);
        Ok(Self::info(found, true))
    }

    async fn unmount_plugin(
        &self,
        name: &str,
    ) -> std::result::Result<PluginInfo, PluginMountError> {
        // Find the disk entry first: after the unmount the entry
        // still lists (mounted false) unless the directory vanished
        // in between, in which case the manifest itself describes it.
        let discovered = self.discover().await;
        let found = discovered
            .iter()
            .find(|d| d.manifest.as_ref().map(|m| m.name.as_str()) == Some(name));
        let mut mgr = self.inner.write().await;
        // A name that exists nowhere is 404, not 409: only a
        // discovered-but-not-mounted plugin is "not mounted".
        if !mgr.is_mounted(name) {
            return Err(if found.is_some() {
                PluginMountError::NotMounted(name.to_string())
            } else {
                PluginMountError::NotFound(name.to_string())
            });
        }
        let manifest = mgr.unmount(name)?;
        Ok(match found {
            Some(d) => Self::info(d, false),
            None => PluginInfo {
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                transport: manifest.transport,
                capabilities: manifest.capabilities,
                source: purrcode_api::plugin::PluginSource::Global,
                path: String::new(),
                mounted: false,
                manifest_ok: true,
                manifest_error: None,
            },
        })
    }
}

#[async_trait::async_trait]
impl PluginHookDispatcher for SharedPluginManager {
    async fn dispatch_session_start(&self, session_id: &str) {
        for p in self.snapshot().await {
            let _ = p.on_session_start(session_id).await;
        }
    }

    async fn dispatch_pre_tool_use(&self, tool: &str, args: &Value) -> bool {
        for p in self.snapshot().await {
            match p.pre_tool_use(tool, args).await {
                Ok(true) => {}
                Ok(false) | Err(_) => return false,
            }
        }
        true
    }

    async fn dispatch_post_tool_use(&self, tool: &str, result: &Value) {
        for p in self.snapshot().await {
            let _ = p.post_tool_use(tool, result).await;
        }
    }

    async fn dispatch_session_stop(&self, session_id: &str) {
        for p in self.snapshot().await {
            let _ = p.on_session_stop(session_id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "purrcode-plugin-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        dir
    }

    fn write_plugin(dir: &Path, manifest: &str, script: Option<(&str, &str)>) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.yaml"), manifest).unwrap();
        if let Some((name, body)) = script {
            let path = dir.join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(body.as_bytes()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    fn manager_for(root: &Path) -> SharedPluginManager {
        SharedPluginManager::new(root.join("plugins"), root.join("ws"))
    }

    #[tokio::test]
    async fn test_mount_unmount_lifecycle() {
        let root = temp_root("lifecycle");
        write_plugin(
            &root.join("plugins").join("echo-hook"),
            "name: echo-hook\nversion: 0.1.0\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        let mgr = manager_for(&root);
        assert!(mgr.list_plugins().await.iter().all(|p| !p.mounted));

        let mounted = mgr.mount_plugin("echo-hook").await.unwrap();
        assert!(mounted.mounted);
        assert_eq!(mounted.name, "echo-hook");
        assert!(mgr.is_mounted("echo-hook").await);

        let err = mgr.mount_plugin("echo-hook").await.unwrap_err();
        assert_eq!(
            err,
            PluginMountError::AlreadyMounted("echo-hook".to_string())
        );

        let unmounted = mgr.unmount_plugin("echo-hook").await.unwrap();
        assert!(!unmounted.mounted);
        assert!(!mgr.is_mounted("echo-hook").await);

        let err = mgr.unmount_plugin("echo-hook").await.unwrap_err();
        assert_eq!(err, PluginMountError::NotMounted("echo-hook".to_string()));
    }

    #[tokio::test]
    async fn test_mount_unknown_broken_and_no_command() {
        let root = temp_root("errors");
        write_plugin(
            &root.join("plugins").join("broken"),
            "version: 1.0.0\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        write_plugin(
            &root.join("plugins").join("builtin-only"),
            "name: builtin-only\nversion: 0.1.0\ntransport: builtin\n",
            None,
        );
        write_plugin(
            &root.join("plugins").join("bad-cmd"),
            "name: bad-cmd\nversion: 0.1.0\ntransport: process\ncommand: /nonexistent/nope\n",
            None,
        );
        let mgr = manager_for(&root);

        let err = mgr.mount_plugin("no-such-plugin").await.unwrap_err();
        assert_eq!(
            err,
            PluginMountError::NotFound("no-such-plugin".to_string())
        );

        // A broken manifest is listed (manifest_ok false) but has no
        // mountable name.
        let listed = mgr.list_plugins().await;
        let broken = listed.iter().find(|p| p.path.ends_with("broken")).unwrap();
        assert!(!broken.manifest_ok);
        assert!(broken.manifest_error.is_some());
        let err = mgr.mount_plugin("broken").await.unwrap_err();
        assert_eq!(err, PluginMountError::NotFound("broken".to_string()));

        let err = mgr.mount_plugin("builtin-only").await.unwrap_err();
        assert_eq!(err, PluginMountError::NoCommand("builtin-only".to_string()));

        let err = mgr.mount_plugin("bad-cmd").await.unwrap_err();
        assert!(matches!(err, PluginMountError::SpawnFailed(_)));
        assert!(!mgr.is_mounted("bad-cmd").await);
    }

    #[tokio::test]
    async fn test_mount_resolves_relative_command_and_dispatches() {
        // The hook script speaks one line of JSON-RPC: it reads the
        // request line, answers `false` to pre_tool_use (veto), and
        // appends a marker so we can count spawns. The marker is
        // written BEFORE the response on purpose: `call_rpc` kills
        // the child as soon as the response line is read, so a
        // post-response write races the kill and made this test
        // flaky (~30% failures). The marker path is absolute: a
        // plugin child inherits the server's cwd, not the plugin
        // directory (unchanged dispatch semantics).
        let root = temp_root("relative");
        let dir = root.join("plugins").join("veto-hook");
        let marker = dir.join("marker.txt");
        let script = format!(
            "#!/bin/sh\nread line\necho x >> {}\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":false}}'\n",
            marker.display()
        );
        write_plugin(
            &dir,
            "name: veto-hook\nversion: 0.1.0\ntransport: process\ncommand: ./hook.sh\n",
            Some(("hook.sh", &script)),
        );
        let mgr = manager_for(&root);
        mgr.mount_plugin("veto-hook").await.unwrap();
        // The mount spawn check ran the script once (marker exists).
        assert!(dir.join("marker.txt").exists());
        // Dispatch spawns it again and honors the veto.
        let allowed = mgr
            .dispatch_pre_tool_use("bash", &serde_json::json!({}))
            .await;
        assert!(!allowed, "mounted veto plugin must veto tool use");
        assert_eq!(
            std::fs::read_to_string(dir.join("marker.txt"))
                .unwrap()
                .lines()
                .count(),
            2,
            "mount check + one dispatch = two spawns"
        );
        // Unmount stops the veto: dispatch allows again.
        mgr.unmount_plugin("veto-hook").await.unwrap();
        assert!(
            mgr.dispatch_pre_tool_use("bash", &serde_json::json!({}))
                .await
        );
    }

    #[tokio::test]
    async fn test_unmount_unknown_is_not_found() {
        // Fix pass P2-1: a name that exists nowhere is NotFound
        // (HTTP 404), not NotMounted (409) — only a discovered
        // but not mounted plugin is "not mounted".
        let root = temp_root("unmount-unknown");
        write_plugin(
            &root.join("plugins").join("echo-hook"),
            "name: echo-hook\nversion: 0.1.0\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        let mgr = manager_for(&root);
        let err = mgr.unmount_plugin("ghost").await.unwrap_err();
        assert_eq!(err, PluginMountError::NotFound("ghost".to_string()));
        let err = mgr.unmount_plugin("echo-hook").await.unwrap_err();
        assert_eq!(err, PluginMountError::NotMounted("echo-hook".to_string()));
    }

    #[tokio::test]
    async fn test_list_duplicate_name_marks_only_mounted_copy() {
        // Fix pass P2-2: a global and a project plugin sharing a
        // manifest name must not both list as mounted — only the
        // copy that is actually registered (global wins the mount)
        // carries `mounted: true`.
        let root = temp_root("dup");
        write_plugin(
            &root.join("plugins").join("dup"),
            "name: dup\nversion: 9.9.9\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        write_plugin(
            &root
                .join("ws")
                .join(".purrcode")
                .join("plugins")
                .join("dup"),
            "name: dup\nversion: 1.0.0\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        let mgr = manager_for(&root);
        let mounted = mgr.mount_plugin("dup").await.unwrap();
        assert_eq!(mounted.version, "9.9.9");
        let listed = mgr.list_plugins().await;
        assert_eq!(listed.len(), 2);
        let global = listed
            .iter()
            .find(|p| p.source == purrcode_api::plugin::PluginSource::Global)
            .unwrap();
        let project = listed
            .iter()
            .find(|p| p.source == purrcode_api::plugin::PluginSource::Project)
            .unwrap();
        assert!(global.mounted);
        assert_eq!(global.version, "9.9.9");
        assert!(!project.mounted);
        assert_eq!(project.version, "1.0.0");
        // Unmount removes the global copy; re-mount mounts it again
        // (first name match wins).
        mgr.unmount_plugin("dup").await.unwrap();
        assert!(mgr.list_plugins().await.iter().all(|p| !p.mounted));
        let mounted = mgr.mount_plugin("dup").await.unwrap();
        assert_eq!(mounted.version, "9.9.9");
    }

    #[tokio::test]
    async fn test_discover_all_resolves_relative_command() {
        // Fix pass P1-1: the startup discovery path
        // (`SharedPluginManager::discover_all`, same
        // `register_discovered` as the CLI `discover_all`) must
        // resolve `./hook.sh` against the plugin dir — the old
        // CLI `scan_dir` registered the raw command, so the
        // script only spawned when the process cwd happened to be
        // the plugin dir and dispatch otherwise vetoed every tool
        // call on the ENOENT error.
        let root = temp_root("startup-relative");
        let dir = root.join("plugins").join("allow-hook");
        let marker = dir.join("marker.txt");
        let script = format!(
            "#!/bin/sh\nread line\necho x >> {}\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":true}}'\n",
            marker.display()
        );
        write_plugin(
            &dir,
            "name: allow-hook\nversion: 0.1.0\ntransport: process\ncommand: ./hook.sh\n",
            Some(("hook.sh", &script)),
        );
        let mgr = manager_for(&root);
        mgr.discover_all().await;
        assert!(mgr.is_mounted("allow-hook").await);
        assert!(
            mgr.dispatch_pre_tool_use("bash", &serde_json::json!({}))
                .await,
            "startup-discovered relative-command plugin must allow tool use"
        );
        assert!(marker.exists(), "hook script must have run (resolved path)");
    }

    #[tokio::test]
    async fn test_spawn_check_kills_backgrounded_grandchildren() {
        // Fix pass P3-1: the mount spawn check used to kill only
        // the direct child — a script that backgrounds a writer
        // leaked it, and the grandchild wrote its marker ~0.5 s
        // after mount returned.
        let root = temp_root("grandchild");
        let dir = root.join("plugins").join("leaky-hook");
        let marker = dir.join("grandchild.txt");
        let script = format!(
            "#!/bin/sh\n(sleep 0.5; echo x >> {}) &\nsleep 5\n",
            marker.display()
        );
        write_plugin(
            &dir,
            "name: leaky-hook\nversion: 0.1.0\ntransport: process\ncommand: ./hook.sh\n",
            Some(("hook.sh", &script)),
        );
        let mgr = manager_for(&root);
        mgr.mount_plugin("leaky-hook").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert!(
            !marker.exists(),
            "backgrounded grandchild survived the mount spawn check"
        );
    }

    #[tokio::test]
    async fn test_spawn_check_kills_fast_exiting_grandchildren() {
        // Fix pass #2 (round-2 re-check P2): the first group-kill
        // fix only killed the group when the child was STILL ALIVE
        // at the end of the 50 ms window. A script that backgrounds
        // a writer and exits immediately leaked the grandchild —
        // the group survives its leader, so the kill must run in
        // every outcome of the spawn check, including the
        // already-exited arms.
        let root = temp_root("grandchild-fast");
        let dir = root.join("plugins").join("fast-leaky-hook");
        let marker = dir.join("grandchild.txt");
        let script = format!(
            "#!/bin/sh\n(sleep 0.5; echo x >> {}) &\nexit 0\n",
            marker.display()
        );
        write_plugin(
            &dir,
            "name: fast-leaky-hook\nversion: 0.1.0\ntransport: process\ncommand: ./hook.sh\n",
            Some(("hook.sh", &script)),
        );
        let mgr = manager_for(&root);
        mgr.mount_plugin("fast-leaky-hook").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert!(
            !marker.exists(),
            "backgrounded grandchild of a fast-exiting child survived the mount spawn check"
        );
    }

    #[tokio::test]
    async fn test_mount_rejects_immediately_dying_command() {
        let root = temp_root("dying");
        write_plugin(
            &root.join("plugins").join("dying"),
            "name: dying\nversion: 0.1.0\ntransport: process\ncommand: /bin/false\n",
            None,
        );
        let mgr = manager_for(&root);
        let err = mgr.mount_plugin("dying").await.unwrap_err();
        assert!(matches!(err, PluginMountError::SpawnFailed(_)));
    }

    #[tokio::test]
    async fn test_list_shows_startup_discovered_as_mounted() {
        let root = temp_root("discovered");
        write_plugin(
            &root.join("plugins").join("echo-hook"),
            "name: echo-hook\nversion: 0.2.0\ntransport: process\ncommand: /bin/true\n",
            None,
        );
        let mgr = manager_for(&root);
        mgr.discover_all().await;
        let listed = mgr.list_plugins().await;
        assert_eq!(listed.len(), 1);
        assert!(listed[0].mounted);
        assert_eq!(listed[0].version, "0.2.0");
        // Unmount then re-mount round-trips through the API.
        mgr.unmount_plugin("echo-hook").await.unwrap();
        assert!(!mgr.list_plugins().await[0].mounted);
        mgr.mount_plugin("echo-hook").await.unwrap();
        assert!(mgr.list_plugins().await[0].mounted);
    }

    #[tokio::test]
    async fn test_unenforced_capabilities_live_after_mount() {
        let root = temp_root("caps");
        write_plugin(
            &root.join("plugins").join("net-hook"),
            "name: net-hook\nversion: 0.1.0\ntransport: process\ncommand: /bin/true\nnetwork: [\"api.example.com\"]\n",
            None,
        );
        let mgr = manager_for(&root);
        assert!(mgr.unenforced_capabilities().await.is_empty());
        mgr.mount_plugin("net-hook").await.unwrap();
        let caps = mgr.unenforced_capabilities().await;
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].0, "net-hook");
        assert_eq!(caps[0].1, vec!["network"]);
    }
}
