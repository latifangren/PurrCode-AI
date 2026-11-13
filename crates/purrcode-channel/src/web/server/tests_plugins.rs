// SPDX-License-Identifier: GPL-3.0-or-later

//! Mount/unmount endpoint tests (v0.4 B4).
//!
//! These test the HTTP layer against a small fake `PluginRegistry`
//! that follows the trait contract (unknown → 404, double mount →
//! 409, broken manifest → 400). The real `SharedPluginManager` is
//! covered by its own unit tests in `purrcode-plugin` (including
//! mount → dispatch veto end-to-end), and the E2E harness exercises
//! the real manager over HTTP against the release binary.

use super::test_support::*;
use purrcode_api::plugin::{
    PluginCapabilities, PluginInfo, PluginMountError, PluginRegistry, PluginSource, PluginTransport,
};
use purrcode_api::tool::AuditSink;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn info(name: &str, mounted: bool, manifest_ok: bool) -> PluginInfo {
    PluginInfo {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: Some(format!("{name} plugin")),
        transport: PluginTransport::Process,
        capabilities: PluginCapabilities {
            network: Vec::new(),
            filesystem: Vec::new(),
            secrets: Vec::new(),
        },
        source: PluginSource::Global,
        path: format!("/plugins/{name}"),
        mounted,
        manifest_ok,
        manifest_error: if manifest_ok {
            None
        } else {
            Some("missing name".to_string())
        },
    }
}

/// Contract-faithful fake registry: the HTTP tests assert status
/// mapping and JSON shape, not the manager's spawn checks.
struct FakeRegistry {
    plugins: Mutex<HashMap<String, PluginInfo>>,
}

impl FakeRegistry {
    fn new(entries: Vec<PluginInfo>) -> Self {
        Self {
            plugins: Mutex::new(entries.into_iter().map(|p| (p.name.clone(), p)).collect()),
        }
    }
}

#[async_trait::async_trait]
impl PluginRegistry for FakeRegistry {
    async fn list_plugins(&self) -> Vec<PluginInfo> {
        let mut out: Vec<PluginInfo> = self.plugins.lock().unwrap().values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    async fn mount_plugin(&self, name: &str) -> std::result::Result<PluginInfo, PluginMountError> {
        let mut map = self.plugins.lock().unwrap();
        let entry = map
            .get_mut(name)
            .ok_or_else(|| PluginMountError::NotFound(name.to_string()))?;
        if !entry.manifest_ok {
            return Err(PluginMountError::InvalidManifest(
                entry.manifest_error.clone().unwrap_or_default(),
            ));
        }
        if entry.mounted {
            return Err(PluginMountError::AlreadyMounted(name.to_string()));
        }
        entry.mounted = true;
        Ok(entry.clone())
    }

    async fn unmount_plugin(
        &self,
        name: &str,
    ) -> std::result::Result<PluginInfo, PluginMountError> {
        let mut map = self.plugins.lock().unwrap();
        let entry = map
            .get_mut(name)
            .ok_or_else(|| PluginMountError::NotFound(name.to_string()))?;
        if !entry.mounted {
            return Err(PluginMountError::NotMounted(name.to_string()));
        }
        entry.mounted = false;
        Ok(entry.clone())
    }
}

type AuditEvents = Arc<Mutex<Vec<(String, String, String, String)>>>;

struct RecordingAuditSink {
    events: AuditEvents,
}

impl RecordingAuditSink {
    fn new() -> (Self, AuditEvents) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                events: events.clone(),
            },
            events,
        )
    }
}

impl AuditSink for RecordingAuditSink {
    fn record_event(
        &self,
        _session_id: &str,
        _agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    ) {
        self.events.lock().unwrap().push((
            event.to_string(),
            tool.unwrap_or_default().to_string(),
            decision.to_string(),
            detail.to_string(),
        ));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_plugins_list_shows_mounted_state() {
    let dir = test_dir("plugins-list");
    let registry: Arc<dyn PluginRegistry> = Arc::new(FakeRegistry::new(vec![
        info("echo-hook", false, true),
        info("veto-hook", true, true),
        info("broken", false, false),
    ]));
    let (port, _reg, cancel_tx, handle) =
        start_server_with_registry(None, dir.clone(), None, Some(registry), None).await;
    let val: serde_json::Value = serde_json::from_str(body_of(&get(port, "/api/plugins"))).unwrap();
    let plugins = val["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 3);
    let echo = plugins.iter().find(|p| p["name"] == "echo-hook").unwrap();
    assert_eq!(echo["mounted"], false);
    assert_eq!(echo["version"], "0.1.0");
    assert_eq!(echo["source"], "global");
    assert_eq!(echo["manifest_ok"], true);
    let veto = plugins.iter().find(|p| p["name"] == "veto-hook").unwrap();
    assert_eq!(veto["mounted"], true);
    let broken = plugins.iter().find(|p| p["name"] == "broken").unwrap();
    assert_eq!(broken["manifest_ok"], false);
    assert_eq!(broken["manifest_error"], "missing name");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_plugins_mount_unmount_roundtrip_and_audit() {
    let dir = test_dir("plugins-roundtrip");
    let registry: Arc<dyn PluginRegistry> =
        Arc::new(FakeRegistry::new(vec![info("echo-hook", false, true)]));
    let (sink, events) = RecordingAuditSink::new();
    let (port, _reg, cancel_tx, handle) = start_server_with_registry(
        None,
        dir.clone(),
        None,
        Some(registry),
        Some(Arc::new(sink)),
    )
    .await;

    let resp = post(port, "/api/plugins/mount", r#"{"name":"echo-hook"}"#);
    assert!(resp.contains("200 OK"), "mount: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["mounted"], true);
    assert_eq!(val["plugin"]["name"], "echo-hook");

    let val: serde_json::Value = serde_json::from_str(body_of(&get(port, "/api/plugins"))).unwrap();
    assert_eq!(val["plugins"][0]["mounted"], true);

    let resp = post(port, "/api/plugins/mount", r#"{"name":"echo-hook"}"#);
    assert!(resp.contains("409 Conflict"), "double mount: {resp}");

    let resp = post(port, "/api/plugins/unmount", r#"{"name":"echo-hook"}"#);
    assert!(resp.contains("200 OK"), "unmount: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["mounted"], false);

    let resp = post(port, "/api/plugins/unmount", r#"{"name":"echo-hook"}"#);
    assert!(resp.contains("409 Conflict"), "double unmount: {resp}");

    let logged = events.lock().unwrap();
    assert_eq!(logged.len(), 2);
    assert_eq!(logged[0].0, "plugin_mount");
    assert_eq!(logged[0].1, "echo-hook");
    assert_eq!(logged[1].0, "plugin_unmount");
    assert_eq!(logged[1].1, "echo-hook");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_plugins_mount_errors() {
    let dir = test_dir("plugins-errors");
    let registry: Arc<dyn PluginRegistry> = Arc::new(FakeRegistry::new(vec![
        info("echo-hook", false, true),
        info("broken", false, false),
    ]));
    let (port, _reg, cancel_tx, handle) =
        start_server_with_registry(None, dir.clone(), None, Some(registry), None).await;

    let resp = post(port, "/api/plugins/mount", r#"{"name":"no-such"}"#);
    assert!(resp.contains("404 Not Found"), "unknown: {resp}");

    let resp = post(port, "/api/plugins/mount", r#"{"name":"broken"}"#);
    assert!(resp.contains("400 Bad Request"), "broken manifest: {resp}");
    assert!(
        body_of(&resp).contains("invalid plugin manifest"),
        "error message: {resp}"
    );

    let resp = post(port, "/api/plugins/mount", r#"{"name":42}"#);
    assert!(resp.contains("400 Bad Request"), "non-string name: {resp}");

    let resp = post(port, "/api/plugins/mount", r#"{"name":"echo-hook"#);
    assert!(resp.contains("400 Bad Request"), "malformed JSON: {resp}");

    let resp = post(port, "/api/plugins/unmount", r#"{"name":"no-such"}"#);
    assert!(resp.contains("404 Not Found"), "unknown unmount: {resp}");

    let resp = post(port, "/api/plugins/unmount", r#"{"name":"echo-hook"}"#);
    assert!(resp.contains("409 Conflict"), "unmount not mounted: {resp}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_plugins_without_registry() {
    let dir = test_dir("plugins-none");
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let val: serde_json::Value = serde_json::from_str(body_of(&get(port, "/api/plugins"))).unwrap();
    assert_eq!(val["plugins"], serde_json::json!([]));
    let resp = post(port, "/api/plugins/mount", r#"{"name":"x"}"#);
    assert!(resp.contains("503 Service Unavailable"), "mount: {resp}");
    let resp = post(port, "/api/plugins/unmount", r#"{"name":"x"}"#);
    assert!(resp.contains("503 Service Unavailable"), "unmount: {resp}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}
