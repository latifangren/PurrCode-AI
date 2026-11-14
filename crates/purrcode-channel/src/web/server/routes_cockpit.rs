// SPDX-License-Identifier: GPL-3.0-or-later

//! Control plane REST endpoints and Audit Log handlers for WebUI Cockpit.

use crate::web::server::routes::record_admin;
use crate::web::server::WebState;
use axum::{
    body::Bytes,
    extract::State,
    http::{StatusCode, Uri},
    Json,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub(crate) fn extract_id(path: &str, prefix: &str) -> String {
    let stripped = path.strip_prefix(prefix).unwrap_or(path);
    let id_part = stripped.split('/').next().unwrap_or(stripped);
    id_part.trim_matches('/').to_string()
}

pub(crate) fn parse_body_json(body: &Bytes) -> Value {
    serde_json::from_slice::<Value>(body).unwrap_or(json!({}))
}

pub(crate) fn save_json(path: &Path, val: &Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(val).unwrap_or_default());
}

pub(crate) fn load_audit_entries(purrcode_dir: &Path, limit: usize) -> Vec<Value> {
    let audit_file = purrcode_dir.join("audit.jsonl");
    let mut entries = Vec::new();

    if let Ok(content) = std::fs::read_to_string(&audit_file) {
        for (idx, line) in content.lines().rev().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                let id = v
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("aud-{:04}", idx + 1));
                let timestamp = v
                    .get("timestamp")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Recent")
                    .to_string();
                let actor = v
                    .get("agent")
                    .or_else(|| v.get("actor"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("System")
                    .to_string();
                let operation = v
                    .get("event")
                    .or_else(|| v.get("operation"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("tool_call")
                    .to_string();
                let target = v
                    .get("tool")
                    .or_else(|| v.get("target"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("system")
                    .to_string();
                let decision = v
                    .get("decision")
                    .or_else(|| v.get("outcome"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("allow");

                let outcome = match decision.to_lowercase().as_str() {
                    "allow" | "allowed" | "approve" => "Allowed",
                    "blocked" | "object_blocked" => "Blocked",
                    "deny" | "denied" | "object" => "Denied",
                    _ => "Allowed",
                };
                let detail = v
                    .get("detail")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();

                entries.push(json!({
                    "id": id, "timestamp": timestamp, "actor": actor,
                    "operation": operation, "target": target, "outcome": outcome, "detail": detail
                }));

                if entries.len() >= limit {
                    break;
                }
            }
        }
    }

    if entries.is_empty() {
        entries.push(json!({
            "id": "aud-init-01", "timestamp": "Session start", "actor": "Meowstro",
            "operation": "orchestrator_init", "target": "cockpit", "outcome": "Allowed",
            "detail": "Workspace root initialized with fail-closed security boundary"
        }));
    }
    entries
}

pub(crate) fn load_preferences(purrcode_dir: &Path) -> Value {
    let pref_path = purrcode_dir.join("cockpit_preferences.json");
    if let Ok(raw) = std::fs::read_to_string(&pref_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            return v;
        }
    }
    json!({
        "activeThemeId": "warm-cream",
        "customThemeTokens": { "bgPrimary": "hsl(35, 30%, 96%)", "bgSecondary": "hsl(35, 25%, 92%)", "accent": "hsl(12, 85%, 62%)" },
        "companion": { "enabled": true, "persona": "cat", "scale": 1.0 }
    })
}

pub async fn save_preferences(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let pref_to_save = payload.get("preferences").unwrap_or(&payload).clone();
    let pref_path = state.purrcode_dir.join("cockpit_preferences.json");
    save_json(&pref_path, &pref_to_save);
    record_admin(
        &state,
        "preferences_update",
        "cockpit",
        "allow",
        "preferences saved",
    );
    (StatusCode::OK, Json(json!({ "preferences": pref_to_save })))
}

pub async fn update_routing(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let routing = payload
        .get("routing")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let file = state.purrcode_dir.join("cockpit_routing.json");
    save_json(&file, &json!(routing));
    record_admin(
        &state,
        "routing.update",
        "clowder",
        "allow",
        "updated routing matrix",
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({ "routing": routing, "auditLog": audit_log })),
    )
}

use super::routes_providers::{
    load_cockpit_overrides, load_merged_providers, sanitize_provider_for_client,
    sanitize_providers_for_client, save_cockpit_overrides,
};

fn upsert_provider_override(purrcode_dir: &Path, id: &str, patch: serde_json::Map<String, Value>) {
    let mut entries = load_cockpit_overrides(purrcode_dir);
    if let Some(existing) = entries
        .iter_mut()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
    {
        if let Some(obj) = existing.as_object_mut() {
            for (k, v) in patch {
                obj.insert(k, v);
            }
        }
    } else {
        let mut new_obj = serde_json::Map::new();
        new_obj.insert("id".to_string(), json!(id));
        for (k, v) in patch {
            new_obj.insert(k, v);
        }
        entries.push(Value::Object(new_obj));
    }
    save_cockpit_overrides(purrcode_dir, &entries);
}

pub async fn add_provider(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let name = payload
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .unwrap_or("");
    let base_url = payload
        .get("baseUrl")
        .and_then(|u| u.as_str())
        .map(str::trim)
        .unwrap_or("");
    let default_model = payload
        .get("defaultModel")
        .and_then(|m| m.as_str())
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() || base_url.is_empty() || default_model.is_empty() {
        record_admin(
            &state,
            "provider_add",
            name,
            "deny",
            "missing required fields",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name, baseUrl, and defaultModel are required" })),
        );
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        record_admin(
            &state,
            "provider_add",
            name,
            "deny",
            "invalid baseUrl scheme",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "baseUrl must use http:// or https://" })),
        );
    }
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let short_uid = &uuid::Uuid::new_v4().simple().to_string()[..6];
    let config_key = if slug.is_empty() {
        format!("custom-{short_uid}")
    } else {
        format!("{slug}-{short_uid}")
    };
    let id = format!("prov-{config_key}");

    let mut overrides = load_cockpit_overrides(&state.purrcode_dir);
    let mut new_prov = payload.clone();
    if let Some(obj) = new_prov.as_object_mut() {
        obj.insert("id".to_string(), json!(id));
        obj.insert("configKey".to_string(), json!(config_key));
        obj.insert("source".to_string(), json!("cockpit"));
        obj.insert("enabled".to_string(), json!(true));
        if !obj.contains_key("models") {
            obj.insert("models".to_string(), json!([default_model]));
        }
    }
    overrides.push(new_prov);
    save_cockpit_overrides(&state.purrcode_dir, &overrides);
    record_admin(
        &state,
        "provider_add",
        name,
        "allow",
        &format!("registered provider {id}"),
    );
    let merged = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    let providers = sanitize_providers_for_client(&merged);
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({ "providers": providers, "auditLog": audit_log })),
    )
}

pub async fn update_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let merged = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    if !merged
        .iter()
        .any(|p| p.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("provider '{id}' not found") })),
        );
    }

    let payload = parse_body_json(&body);
    let mut patch = serde_json::Map::new();

    if let Some(enabled) = payload.get("enabled").and_then(|b| b.as_bool()) {
        patch.insert("enabled".to_string(), json!(enabled));
    }
    if let Some(name) = payload.get("name").and_then(|s| s.as_str()) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            patch.insert("name".to_string(), json!(trimmed));
        }
    }
    if let Some(protocol) = payload.get("protocol").and_then(|s| s.as_str()) {
        let trimmed = protocol.trim();
        if !trimmed.is_empty() {
            patch.insert("protocol".to_string(), json!(trimmed));
        }
    }
    if let Some(base_url) = payload.get("baseUrl").and_then(|s| s.as_str()) {
        let trimmed = base_url.trim();
        if !trimmed.is_empty() {
            if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "baseUrl must use http:// or https://" })),
                );
            }
            patch.insert("baseUrl".to_string(), json!(trimmed));
        }
    }
    if let Some(cw) = payload.get("contextWindow").and_then(|n| n.as_u64()) {
        if cw >= 1024 {
            patch.insert("contextWindow".to_string(), json!(cw));
        }
    }
    if let Some(models) = payload.get("models").and_then(|m| m.as_array()) {
        let cleaned: Vec<String> = models
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        patch.insert("models".to_string(), json!(cleaned));
    }
    if let Some(default_model) = payload.get("defaultModel").and_then(|s| s.as_str()) {
        let trimmed = default_model.trim();
        if !trimmed.is_empty() {
            patch.insert("defaultModel".to_string(), json!(trimmed));
        }
    }
    if payload
        .get("clearApiKey")
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
    {
        patch.insert("apiKey".to_string(), json!(""));
    } else if let Some(api_key) = payload.get("apiKey").and_then(|s| s.as_str()) {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            patch.insert("apiKey".to_string(), json!(trimmed));
        }
    }

    if !patch.is_empty() {
        upsert_provider_override(&state.purrcode_dir, &id, patch);
    }

    record_admin(
        &state,
        "provider_update",
        &id,
        "allow",
        "updated provider settings",
    );
    let updated_merged = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    let providers = sanitize_providers_for_client(&updated_merged);
    (StatusCode::OK, Json(json!({ "providers": providers })))
}

pub async fn delete_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let merged = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    if let Some(found) = merged
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        if found.get("source").and_then(|v| v.as_str()) == Some("config") {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Provider is defined in purrcode.yaml/purrcode.jsonc; toggle it disabled instead or remove it from the config file."
                })),
            );
        }
    }
    let mut overrides = load_cockpit_overrides(&state.purrcode_dir);
    overrides.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(&id));
    save_cockpit_overrides(&state.purrcode_dir, &overrides);
    record_admin(&state, "provider_delete", &id, "allow", "provider removed");
    let updated = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    let providers = sanitize_providers_for_client(&updated);
    (StatusCode::OK, Json(json!({ "providers": providers })))
}

pub async fn test_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let merged = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    let entry = merged
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&id));

    let base_url_opt = entry
        .and_then(|p| p.get("baseUrl").and_then(|u| u.as_str()))
        .map(str::to_string);
    let api_key_opt = entry
        .and_then(|p| p.get("apiKey").and_then(|k| k.as_str()))
        .map(str::to_string)
        .or_else(|| {
            let clean = id.strip_prefix("prov-").unwrap_or(&id);
            purrcode_config::ConfigLoader::load_effective()
                .ok()
                .and_then(|cfg| cfg.providers.get(clean).and_then(|p| p.api_key.clone()))
        });

    let Some(base_url) = base_url_opt else {
        record_admin(&state, "provider_ping", &id, "deny", "provider not found");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("provider '{id}' not found") })),
        );
    };
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        record_admin(
            &state,
            "provider_ping",
            &id,
            "deny",
            "invalid baseUrl scheme",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "provider baseUrl must use http:// or https://" })),
        );
    }

    let target_url = format!("{}/models", base_url.trim_end_matches('/'));
    let start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(3000))
        .build();
    let probe = match client {
        Ok(c) => {
            let mut req = c.get(&target_url);
            if let Some(ref key) = api_key_opt {
                if !key.trim().is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", key.trim()));
                }
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => Ok(start.elapsed().as_millis() as u64),
                Ok(resp) => Err(format!("HTTP {}", resp.status())),
                Err(e) => Err(format!("connection failed: {e}")),
            }
        }
        Err(e) => Err(format!("http client error: {e}")),
    };

    let (status_code, latency_val, detail, ok) = match probe {
        Ok(ms) => (
            StatusCode::OK,
            json!(ms),
            "endpoint reachable".to_string(),
            true,
        ),
        Err(err) => (StatusCode::BAD_GATEWAY, Value::Null, err, false),
    };

    let mut patch = serde_json::Map::new();
    patch.insert("lastLatencyMs".to_string(), latency_val.clone());
    patch.insert(
        "lastTestedAt".to_string(),
        json!(if ok { "Just now" } else { "Failed" }),
    );
    upsert_provider_override(&state.purrcode_dir, &id, patch);

    record_admin(
        &state,
        "provider_ping",
        &id,
        if ok { "allow" } else { "deny" },
        &detail,
    );

    let updated = load_merged_providers(&state.workspace_root, &state.purrcode_dir);
    let providers = sanitize_providers_for_client(&updated);
    let prov = providers
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&id))
        .map(sanitize_provider_for_client)
        .unwrap_or_else(|| {
            json!({ "id": id, "lastLatencyMs": latency_val, "lastTestedAt": if ok { "Just now" } else { "Failed" } })
        });
    if ok {
        (
            status_code,
            Json(json!({ "provider": prov, "providers": providers })),
        )
    } else {
        (
            status_code,
            Json(json!({ "error": detail, "provider": prov, "providers": providers })),
        )
    }
}

pub(crate) fn load_sandbox_state(purrcode_dir: &Path, workspace_root: &Path) -> Value {
    let file = purrcode_dir.join("cockpit_sandbox.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(mut v) = serde_json::from_str::<Value>(&raw) {
            if let Some(obj) = v.as_object_mut() {
                // Seccomp network enforcement is not yet wired; keep network status honest.
                obj.insert("networkEnforced".to_string(), json!(false));
            }
            return v;
        }
    }
    let landlock_supported = cfg!(target_os = "linux");
    json!({
        "landlockAbi": if landlock_supported { "ABI v8 (Linux Landlock)" } else { "Unavailable (non-Linux)" },
        "enforced": landlock_supported,
        "networkEnforced": false,
        "outboundNetworkEnabled": true,
        "blockedCidrs": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "169.254.0.0/16"],
        "paths": [
            { "id": "sp-1", "path": workspace_root.display().to_string(), "mode": "rw", "addedAt": "Session start" },
            { "id": "sp-2", "path": "/usr", "mode": "ro", "addedAt": "System default" }
        ],
        "auditLog": []
    })
}

fn save_sandbox_state(purrcode_dir: &Path, state: &Value) {
    save_json(&purrcode_dir.join("cockpit_sandbox.json"), state);
}

fn validate_sandbox_path(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err("path must not be empty or contain NUL bytes");
    }
    if trimmed.split(['/', '\\']).any(|seg| seg == "..") {
        return Err("path traversal ('..') is forbidden");
    }
    if !trimmed.starts_with('/') && !trimmed.starts_with("~/") && !trimmed.starts_with("./") {
        return Err("path must start with '/', '~/', or './'");
    }
    let normalized = trimmed.trim_end_matches('/').to_string();
    if normalized.is_empty() || normalized == "~" {
        return Err("root or bare home directory cannot be mounted directly");
    }
    const BLOCKED_PREFIXES: &[&str] = &[
        "/etc",
        "/root",
        "/var/run",
        "/proc",
        "/sys",
        "/dev",
        "/boot",
        "/usr/sbin",
        "/sbin",
    ];
    if BLOCKED_PREFIXES
        .iter()
        .any(|p| normalized == *p || normalized.starts_with(&format!("{p}/")))
    {
        return Err("sensitive system directory is blocked by policy");
    }
    const BLOCKED_FRAGMENTS: &[&str] = &[".ssh", ".gnupg", ".aws", ".kube", "id_rsa", "id_ed25519"];
    let lower = normalized.to_ascii_lowercase();
    if BLOCKED_FRAGMENTS.iter().any(|f| lower.contains(f)) {
        return Err("credential paths cannot be exposed in sandbox rules");
    }
    Ok(normalized)
}

pub async fn add_sandbox_path(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let raw_path = payload.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let mode = payload.get("mode").and_then(|m| m.as_str()).unwrap_or("ro");
    if mode != "ro" && mode != "rw" {
        record_admin(&state, "sandbox_path_add", raw_path, "deny", "invalid mode");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "mode must be 'ro' or 'rw'" })),
        );
    }
    let path_str = match validate_sandbox_path(raw_path) {
        Ok(p) => p,
        Err(reason) => {
            record_admin(&state, "sandbox_path_add", raw_path, "deny", reason);
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": reason })));
        }
    };
    let mut sb = load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    let rule_id = format!("sp-{}", uuid::Uuid::new_v4().simple());
    if let Some(paths) = sb.get_mut("paths").and_then(|p| p.as_array_mut()) {
        paths.push(json!({ "id": rule_id, "path": path_str, "mode": mode, "addedAt": "Just now" }));
    }
    save_sandbox_state(&state.purrcode_dir, &sb);
    record_admin(
        &state,
        "sandbox_path_add",
        &path_str,
        "allow",
        &format!("mode={mode}"),
    );
    (StatusCode::CREATED, Json(json!({ "sandbox": sb })))
}

pub async fn remove_sandbox_path(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/sandbox/paths/");
    let mut sb = load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    if let Some(paths) = sb.get_mut("paths").and_then(|p| p.as_array_mut()) {
        paths.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(&id));
    }
    save_sandbox_state(&state.purrcode_dir, &sb);
    record_admin(
        &state,
        "sandbox_path_remove",
        &id,
        "allow",
        "path rule removed",
    );
    (StatusCode::OK, Json(json!({ "sandbox": sb })))
}

pub async fn toggle_sandbox_network(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let mut sb = load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    let enabled = payload
        .get("outboundNetworkEnabled")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    if let Some(obj) = sb.as_object_mut() {
        obj.insert("outboundNetworkEnabled".to_string(), json!(enabled));
    }
    save_sandbox_state(&state.purrcode_dir, &sb);
    record_admin(
        &state,
        "sandbox_network_toggle",
        "outbound",
        if enabled { "allow" } else { "deny" },
        &format!("outbound={enabled}"),
    );
    (StatusCode::OK, Json(json!({ "sandbox": sb })))
}

pub async fn save_memory_files(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let name = payload
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("MEMORY.md");
    if !matches!(name, "MEMORY.md" | "USER.md" | "SOUL.md") {
        record_admin(
            &state,
            "memory_file_save",
            name,
            "deny",
            "invalid memory file name",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name must be MEMORY.md, USER.md, or SOUL.md" })),
        );
    }
    let Some(content) = payload.get("content").and_then(|c| c.as_str()) else {
        record_admin(
            &state,
            "memory_file_save",
            name,
            "deny",
            "content must be a string",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a string" })),
        );
    };
    if content.contains('\0') {
        record_admin(
            &state,
            "memory_file_save",
            name,
            "deny",
            "NUL byte rejected",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must not contain NUL bytes" })),
        );
    }

    let ws_target = state.workspace_root.join(name);
    let target_path = if ws_target.exists() {
        ws_target
    } else {
        state.purrcode_dir.join(name)
    };
    if let Some(parent) = target_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            record_admin(&state, "memory_file_save", name, "deny", &e.to_string());
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to prepare directory: {e}") })),
            );
        }
    }
    if let Err(e) = std::fs::write(&target_path, content) {
        record_admin(&state, "memory_file_save", name, "deny", &e.to_string());
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to save {name}: {e}") })),
        );
    }
    record_admin(
        &state,
        "memory_file_save",
        name,
        "allow",
        &format!("saved {} bytes", content.len()),
    );

    let names = ["MEMORY.md", "USER.md", "SOUL.md"];
    let mut memory_files = Vec::new();
    for fname in names {
        let ws_path = state.workspace_root.join(fname);
        let read_path = if ws_path.exists() {
            ws_path
        } else {
            state.purrcode_dir.join(fname)
        };
        let text = std::fs::read_to_string(&read_path).unwrap_or_default();
        let desc = match fname {
            "MEMORY.md" => "Persistent architectural facts and invariants.",
            "USER.md" => "Operator preferences, coding style, and workflow defaults.",
            "SOUL.md" => "Core persona, reasoning discipline, and Clowder rules.",
            _ => "Project memory file",
        };
        memory_files.push(
            json!({ "name": fname, "description": desc, "content": text, "updatedAt": "Just now" }),
        );
    }
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({ "memoryFiles": memory_files, "auditLog": audit_log })),
    )
}
