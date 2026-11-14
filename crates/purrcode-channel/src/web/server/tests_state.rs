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

    // Providers array
    let providers = state["providers"].as_array().expect("providers is array");
    assert!(!providers.is_empty(), "providers should not be empty");
    assert!(providers.iter().any(|p| p["id"] == "prov-magpie"));

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

    // Sandbox
    assert_eq!(state["sandbox"]["enforced"], true);
    assert!(state["sandbox"]["paths"].is_array());

    // Skills
    let skills = state["skills"].as_array().expect("skills is array");
    assert!(!skills.is_empty(), "bundled skills should be listed");

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
