// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the unified state endpoint (`GET /api/state`).

use super::test_support::*;
use crate::web::server::WebServer;
use crate::web::sessions::WebSessionRegistry;
use std::sync::Arc;
use std::time::Duration;

fn body_of(resp: &str) -> &str {
    resp.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or(resp)
}

fn req_raw(port: u16, method: &str, path: &str, body: &str) -> String {
    http_raw(
        port,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_state_endpoint_schema() {
    let dir = test_dir("state-endpoint");
    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    let resp = get(port, "/api/state");
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");

    let body = body_of(&resp);
    let state: serde_json::Value = serde_json::from_str(body).expect("valid json state");

    assert_eq!(state["schemaVersion"], 1);
    assert!(state["projectName"].is_string());
    assert!(state["projectPath"].is_string());
    assert!(state["activeSessionId"].is_string());

    // Sessions array
    let sessions = state["sessions"].as_array().expect("sessions is array");
    assert!(!sessions.is_empty(), "sessions should not be empty");
    assert_eq!(sessions[0]["id"], "purr_web");

    // Providers array: empty by default when purrcode.jsonc has no providers configured
    let providers = state["providers"].as_array().expect("providers is array");
    assert!(
        providers.is_empty(),
        "providers should not fabricate synthetic defaults"
    );

    // Subagent routing
    let routing = state["routing"].as_array().expect("routing is array");
    assert_eq!(routing.len(), 6, "expected 6 subagent roles in routing");
    assert_eq!(routing[0]["role"], "Meowstro");
    assert_eq!(routing[1]["role"], "Tinker");

    // Memory files
    let memory_files = state["memoryFiles"]
        .as_array()
        .expect("memoryFiles is array");
    assert_eq!(memory_files.len(), 3, "expected 3 memory files");
    assert_eq!(memory_files[0]["name"], "MEMORY.md");
    assert_eq!(memory_files[1]["name"], "USER.md");
    assert_eq!(memory_files[2]["name"], "SOUL.md");

    // Sandbox: honest enforcement reporting (Landlock on Linux, false for network)
    assert_eq!(state["sandbox"]["enforced"], cfg!(target_os = "linux"));
    assert_eq!(state["sandbox"]["networkEnforced"], false);
    assert!(state["sandbox"]["paths"].is_array());

    // Skills
    let skills = state["skills"].as_array().expect("skills is array");
    assert!(!skills.is_empty(), "bundled skills should be listed");

    // UI config, workspaceFiles, and presets
    assert!(state["ui"].is_object(), "ui config should be present");
    assert_eq!(state["ui"]["theme"], "warm-paper");
    assert!(state["workspaceFiles"].is_array());
    let presets = state["presets"].as_array().expect("presets is array");
    assert!(!presets.is_empty(), "presets should not be empty");

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_state_with_custom_purrcode_jsonc() {
    let dir = test_dir("state-jsonc");

    // Write a mock purrcode.jsonc inside the workspace dir
    let cfg_content = r#"{
        "preset": "carrier",
        "model": "magpie:antigravity/gemini-3.8-flash-tiered",
        "providers": {
            "magpie": {
                "base_url": "http://127.0.0.1:3425/v1",
                "api": "openai-completions",
                "models": {
                    "antigravity/gemini-3.8-flash-tiered": {
                        "context_window": 1048576
                    }
                }
            }
        }
    }"#;
    std::fs::write(dir.join("purrcode.jsonc"), cfg_content).unwrap();

    // Write a real MEMORY.md
    std::fs::write(dir.join("MEMORY.md"), "# Custom Project Memory\n").unwrap();

    let (port, _reg, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    let resp = get(port, "/api/state");
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");

    let body = body_of(&resp);
    let state: serde_json::Value = serde_json::from_str(body).expect("valid json state");

    // Session preset should reflect carrier
    let sessions = state["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["preset"], "carrier");

    // Memory file content should reflect custom file
    let memory_files = state["memoryFiles"].as_array().unwrap();
    assert_eq!(memory_files[0]["content"], "# Custom Project Memory\n");

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_serves_webui_dist_static_and_spa_fallback() {
    let dir = test_dir("state-dist");
    let dist_dir = dir.join("webui").join("dist");
    std::fs::create_dir_all(&dist_dir).unwrap();

    let html_content = "<!doctype html><html><body>PurrCode WebUI SPA</body></html>";
    std::fs::write(dist_dir.join("index.html"), html_content).unwrap();

    let asset_dir = dist_dir.join("assets");
    std::fs::create_dir_all(&asset_dir).unwrap();
    std::fs::write(asset_dir.join("app.js"), "console.log('purr');").unwrap();

    let port = free_port();
    let registry = Arc::new(WebSessionRegistry::new(dir.clone()));
    let server = WebServer::new(
        "127.0.0.1",
        port,
        None,
        registry.clone(),
        dir.clone(),
        dir.clone(),
    );
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move {
        let _ = server.run(cancel_rx).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Root should serve index.html
    let resp_root = get(port, "/");
    assert!(
        resp_root.starts_with("HTTP/1.1 200 OK"),
        "resp: {resp_root}"
    );
    assert!(resp_root.contains("PurrCode WebUI SPA"));

    // Static asset should serve app.js
    let resp_asset = get(port, "/assets/app.js");
    assert!(
        resp_asset.starts_with("HTTP/1.1 200 OK"),
        "resp: {resp_asset}"
    );
    assert!(resp_asset.contains("console.log('purr');"));

    // SPA fallback: an unknown non-api path serves index.html
    let resp_spa = get(port, "/sessions/custom-session-12");
    assert!(resp_spa.starts_with("HTTP/1.1 200 OK"), "resp: {resp_spa}");
    assert!(resp_spa.contains("PurrCode WebUI SPA"));

    // Unknown API routes must still return 404, never index.html
    let resp_api_missing = get(port, "/api/does_not_exist");
    assert!(
        resp_api_missing.starts_with("HTTP/1.1 404"),
        "resp: {resp_api_missing}"
    );

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cockpit_sessions_mutations_and_audit_logs() {
    let dir = test_dir("cockpit-mutations");
    let port = free_port();
    let registry = Arc::new(WebSessionRegistry::new(dir.clone()));
    let s1 = registry.create_session().unwrap();

    let server = WebServer::new(
        "127.0.0.1",
        port,
        None,
        registry.clone(),
        dir.clone(),
        dir.clone(),
    );
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move {
        let _ = server.run(cancel_rx).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // PATCH session
    let patch_body = r#"{"activate":true}"#;
    let patch_resp = req_raw(
        port,
        "PATCH",
        &format!("/api/sessions/{}", s1.id),
        patch_body,
    );
    assert!(
        patch_resp.starts_with("HTTP/1.1 200 OK"),
        "patch failed: {patch_resp}"
    );

    // DELETE session
    let del_resp = req_raw(port, "DELETE", &format!("/api/sessions/{}", s1.id), "");
    assert!(
        del_resp.starts_with("HTTP/1.1 200 OK"),
        "delete failed: {del_resp}"
    );
    let del_json: serde_json::Value =
        serde_json::from_str(body_of(&del_resp)).expect("json delete body");
    assert!(del_json["sessions"].is_array());
    assert!(!del_json["activeSessionId"].as_str().unwrap().is_empty());

    // GET /api/logs & /api/audit
    let logs_resp = get(port, "/api/logs");
    assert!(logs_resp.starts_with("HTTP/1.1 200 OK"));
    let logs_json: serde_json::Value =
        serde_json::from_str(body_of(&logs_resp)).expect("json logs body");
    assert!(logs_json["entries"].is_array());

    // PUT /api/preferences
    let pref_body = r#"{"theme":"catppuccin-mocha","density":"comfortable"}"#;
    let pref_resp = req_raw(port, "PUT", "/api/preferences", pref_body);
    assert!(pref_resp.starts_with("HTTP/1.1 200 OK"));

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn cockpit_security_and_session_regression_guards() {
    let dir = test_dir("cockpit-security");
    let (port, registry, cancel_tx, handle) = start_server(None, dir.clone(), None).await;

    // 1. Disallowed memory filename must return 400 and never write outside allowlist.
    let bad_mem_body = serde_json::json!({
        "name": "../evil.md",
        "content": "pwned"
    })
    .to_string();
    let resp = req_raw(port, "PUT", "/api/memory/files", &bad_mem_body);
    assert!(resp.starts_with("HTTP/1.1 400"), "resp: {resp}");
    assert!(!dir.parent().unwrap().join("evil.md").exists());

    // Valid memory file persists cleanly.
    let ok_mem_body = serde_json::json!({
        "name": "MEMORY.md",
        "content": "# Notes\nHello"
    })
    .to_string();
    let resp = req_raw(port, "PUT", "/api/memory/files", &ok_mem_body);
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");
    assert_eq!(
        std::fs::read_to_string(dir.join("MEMORY.md")).unwrap(),
        "# Notes\nHello"
    );

    // 2. Sandbox path validation rejects relative/nonexistent paths and invalid modes.
    let bad_sb = serde_json::json!({ "path": "relative/path", "mode": "rw" }).to_string();
    let resp = req_raw(port, "POST", "/api/sandbox/paths", &bad_sb);
    assert!(resp.starts_with("HTTP/1.1 400"), "resp: {resp}");

    let bad_mode = serde_json::json!({
        "path": dir.to_string_lossy(),
        "mode": "exec"
    })
    .to_string();
    let resp = req_raw(port, "POST", "/api/sandbox/paths", &bad_mode);
    assert!(resp.starts_with("HTTP/1.1 400"), "resp: {resp}");

    let ok_sb = serde_json::json!({
        "path": dir.to_string_lossy(),
        "mode": "ro"
    })
    .to_string();
    let resp = req_raw(port, "POST", "/api/sandbox/paths", &ok_sb);
    assert!(resp.starts_with("HTTP/1.1 201"), "resp: {resp}");

    // 3. PATCH invalid id returns 400, unknown valid-format session returns 404; known session persists preset/model/title into state.
    let resp = req_raw(
        port,
        "PATCH",
        "/api/sessions/does-not-exist",
        r#"{"title":"Nope"}"#,
    );
    assert!(resp.starts_with("HTTP/1.1 400"), "resp: {resp}");

    let resp = req_raw(
        port,
        "PATCH",
        "/api/sessions/purr_web_000000000000",
        r#"{"title":"Nope"}"#,
    );
    assert!(resp.starts_with("HTTP/1.1 404"), "resp: {resp}");

    let resp = req_raw(port, "POST", "/api/sessions", "{}");
    assert!(resp.starts_with("HTTP/1.1 201"), "resp: {resp}");
    let created: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    let sid = created["session"]["id"].as_str().unwrap().to_string();

    let patch_body = serde_json::json!({
        "title": "Custom Audit Chat",
        "preset": "catwalk",
        "model": "openai/gpt-4o"
    })
    .to_string();
    let resp = req_raw(port, "PATCH", &format!("/api/sessions/{sid}"), &patch_body);
    assert!(resp.starts_with("HTTP/1.1 200 OK"), "resp: {resp}");

    let state_resp = get(port, "/api/state");
    assert!(state_resp.starts_with("HTTP/1.1 200 OK"));
    let state_json: serde_json::Value = serde_json::from_str(body_of(&state_resp)).unwrap();
    let sessions = state_json["sessions"].as_array().unwrap();
    let patched = sessions.iter().find(|s| s["id"] == sid).unwrap();
    assert_eq!(patched["title"], "Custom Audit Chat");
    assert_eq!(patched["preset"], "catwalk");
    assert_eq!(patched["model"], "openai/gpt-4o");

    // 4. Live approval resolution routes through WebChannel and does not return fake sessions.
    let session = registry.ensure_session_if_exists(&sid).unwrap();
    let chan = session.channel.clone();
    let req_task = tokio::spawn(async move {
        use purrcode_api::channel::Channel;
        chan.request_approval("call-1", "bash", "echo hi", None)
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let ap_body = serde_json::json!({
        "session_id": sid,
        "decision": "approved"
    })
    .to_string();
    let ap_resp = req_raw(port, "POST", "/api/approvals/call-1", &ap_body);
    assert!(ap_resp.starts_with("HTTP/1.1 200 OK"), "resp: {ap_resp}");
    let ap_json: serde_json::Value = serde_json::from_str(body_of(&ap_resp)).unwrap();
    assert_eq!(ap_json["resolved"], true);
    assert!(ap_json.get("sessions").is_none());
    assert!(req_task.await.unwrap());

    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(dir);
}
