// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for the plugin manager (extracted from lib.rs in
//! the v0.4 B4 fix pass: lib.rs had grown past the ADR-011 800
//! line limit).

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

struct MockPlugin {
    manifest: PluginManifest,
    should_veto: bool,
}

#[async_trait]
impl Plugin for MockPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn on_session_start(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn pre_tool_use(&self, _tool: &str, _arguments: &Value) -> Result<bool> {
        Ok(!self.should_veto)
    }

    async fn post_tool_use(&self, _tool: &str, _result: &Value) -> Result<()> {
        Ok(())
    }

    async fn on_session_stop(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_plugin_manager_dispatch_pre_tool_use_veto() {
    let manifest = PluginManifest {
        name: "veto-guard".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        transport: PluginTransport::Process,
        capabilities: PluginCapabilities {
            network: Vec::new(),
            filesystem: Vec::new(),
            secrets: Vec::new(),
        },
    };

    let mut mgr = PluginManager::new();
    mgr.register(Arc::new(MockPlugin {
        manifest,
        should_veto: true,
    }));

    let allowed = mgr
        .dispatch_pre_tool_use("bash", &serde_json::json!({}))
        .await;
    assert!(
        !allowed,
        "Plugin must veto tool execution when returning false"
    );
}

#[test]
fn test_parse_plugin_yaml_with_declared_secrets() {
    let yaml = r#"
name: secret-reader
version: 1.0.0
transport: process
command: ./runner
secrets: ["OPENAI_API_KEY", "DEEPSEEK_API_KEY"]
"#;
    let (manifest, cmd) = parse_plugin_yaml_str(yaml).unwrap();
    assert_eq!(cmd.as_deref(), Some("./runner"));
    let p = ProcessPlugin::new(manifest, "./runner");
    assert!(p.is_secret_allowed("OPENAI_API_KEY"));
    assert!(p.is_secret_allowed("DEEPSEEK_API_KEY"));
    assert!(!p.is_secret_allowed("ANTHROPIC_API_KEY"));
}

#[test]
fn test_parse_plugin_yaml_missing_name_rejected() {
    let yaml = r#"
version: 1.0.0
transport: process
command: ./runner
"#;
    let res = parse_plugin_yaml_str(yaml);
    assert!(res.is_err());
}

#[test]
fn test_parse_plugin_yaml_records_network_and_filesystem() {
    // network/filesystem are declarations for the v0.3 OS sandbox:
    // parsed and recorded, but NOT enforced in v0.1.
    let yaml = r#"
name: net-plugin
version: 1.0.0
transport: process
command: ./runner
network: ["api.example.com", "*.cdn.example"]
filesystem: ["/tmp", "/home/user/data"]
"#;
    let (manifest, _) = parse_plugin_yaml_str(yaml).unwrap();
    assert_eq!(manifest.capabilities.network.len(), 2);
    assert_eq!(manifest.capabilities.filesystem.len(), 2);
    assert!(manifest.capabilities.secrets.is_empty());
}

#[test]
fn test_unenforced_capabilities_reports_declared_only() {
    let mut mgr = PluginManager::new();
    let declared = PluginManifest {
        name: "net-plugin".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        transport: PluginTransport::Process,
        capabilities: PluginCapabilities {
            network: vec!["api.example.com".to_string()],
            filesystem: vec!["/tmp".to_string()],
            secrets: Vec::new(),
        },
    };
    mgr.register(Arc::new(ProcessPlugin::new(declared, "./runner")));
    let secrets_only = PluginManifest {
        name: "secret-reader".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        transport: PluginTransport::Process,
        capabilities: PluginCapabilities {
            network: Vec::new(),
            filesystem: Vec::new(),
            secrets: vec!["OPENAI_API_KEY".to_string()],
        },
    };
    mgr.register(Arc::new(ProcessPlugin::new(secrets_only, "./runner")));
    let reports = mgr.unenforced_capabilities();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, "net-plugin");
    assert_eq!(reports[0].1, vec!["network", "filesystem"]);
}

#[tokio::test]
async fn test_cli_discover_all_resolves_relative_command() {
    // Fix pass P1-1: the CLI/gateway/cron startup path
    // (`PluginManager::discover_all`, used by main/gateway/
    // one_shot/tui/cron) used to register the raw manifest
    // command, so `./hook.sh` only spawned when the process
    // cwd happened to be the plugin dir — otherwise dispatch
    // vetoed every tool call on the ENOENT error, with no
    // diagnosis. It must go through the same
    // `register_discovered` (resolve + dedupe) as the web path.
    // The project dir is fully controlled here; the global dir
    // comes from HOME, so the assertion targets the one plugin
    // by name instead of the whole dispatch (a stray global
    // plugin must not fail this test).
    use std::io::Write;
    let root = std::env::temp_dir().join(format!(
        "purrcode-plugin-test-{}-cli-relative",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root
        .join("ws")
        .join(".purrcode")
        .join("plugins")
        .join("b4-fix-allow-hook");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.yaml"),
        "name: b4-fix-allow-hook\nversion: 0.1.0\ntransport: process\ncommand: ./hook.sh\n",
    )
    .unwrap();
    let marker = dir.join("marker.txt");
    let script = format!(
        "#!/bin/sh\nread line\necho x >> {}\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":true}}'\n",
        marker.display()
    );
    let path = dir.join("hook.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Close the file before any spawn: an open-for-write
    // executable fails exec with ETXTBSY, and no retry window
    // covers a handle held for the whole test.
    drop(f);
    let mut mgr = PluginManager::new();
    mgr.discover_all(&root.join("ws"));
    let found = mgr
        .plugins()
        .into_iter()
        .find(|p| p.manifest().name == "b4-fix-allow-hook")
        .expect("startup discovery must register the project plugin");
    assert!(
        found
            .pre_tool_use("bash", &serde_json::json!({}))
            .await
            .unwrap(),
        "relative-command plugin discovered at startup must allow tool use"
    );
    assert!(marker.exists(), "hook script must have run (resolved path)");
    let _ = std::fs::remove_dir_all(&root);
}
