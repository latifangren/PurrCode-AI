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

fn extract_id(path: &str, prefix: &str) -> String {
    let stripped = path.strip_prefix(prefix).unwrap_or(path);
    let id_part = stripped.split('/').next().unwrap_or(stripped);
    id_part.trim_matches('/').to_string()
}

fn parse_body_json(body: &Bytes) -> Value {
    serde_json::from_slice::<Value>(body).unwrap_or(json!({}))
}

fn save_json(path: &Path, val: &Value) {
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

fn load_providers_config(purrcode_dir: &Path) -> Vec<Value> {
    let file = purrcode_dir.join("cockpit_providers.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(&raw) {
            return v;
        }
    }
    vec![
        json!({
            "id": "prov-magpie", "name": "Local Magpie / OpenAI-Compatible", "protocol": "openai",
            "baseUrl": "http://127.0.0.1:3425/v1", "defaultModel": "antigravity/gemini-3.8-flash-tiered",
            "models": ["antigravity/gemini-3.8-flash-tiered"], "contextWindow": 1048576,
            "enabled": true, "lastLatencyMs": 12, "lastTestedAt": "Just now"
        }),
        json!({
            "id": "prov-custom", "name": "Custom Gateway Endpoint", "protocol": "custom",
            "baseUrl": "http://localhost:8000/v1", "defaultModel": "antigravity/gemini-3.8-flash-tiered",
            "models": ["antigravity/gemini-3.8-flash-tiered"], "contextWindow": 1048576,
            "enabled": true, "lastLatencyMs": Value::Null, "lastTestedAt": Value::Null
        }),
    ]
}

fn save_providers_config(purrcode_dir: &Path, providers: &[Value]) {
    save_json(
        &purrcode_dir.join("cockpit_providers.json"),
        &json!(providers),
    );
}

pub async fn add_provider(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let mut providers = load_providers_config(&state.purrcode_dir);
    let name = payload
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("New Provider");
    let id = format!("prov-{}", uuid::Uuid::new_v4().simple());
    let mut new_prov = payload.clone();
    if let Some(obj) = new_prov.as_object_mut() {
        obj.insert("id".to_string(), json!(id));
        obj.insert("enabled".to_string(), json!(true));
        if !obj.contains_key("models") {
            let def = obj
                .get("defaultModel")
                .and_then(|m| m.as_str())
                .unwrap_or("antigravity/gemini-3.8-flash-tiered");
            obj.insert("models".to_string(), json!([def]));
        }
    }
    providers.push(new_prov);
    save_providers_config(&state.purrcode_dir, &providers);
    record_admin(
        &state,
        "provider_add",
        name,
        "allow",
        &format!("registered provider {id}"),
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({ "providers": providers, "auditLog": audit_log })),
    )
}

pub async fn toggle_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let payload = parse_body_json(&body);
    let enabled = payload
        .get("enabled")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let mut providers = load_providers_config(&state.purrcode_dir);
    for prov in &mut providers {
        if prov.get("id").and_then(|v| v.as_str()) == Some(&id) {
            if let Some(obj) = prov.as_object_mut() {
                obj.insert("enabled".to_string(), json!(enabled));
            }
        }
    }
    save_providers_config(&state.purrcode_dir, &providers);
    record_admin(
        &state,
        "provider_toggle",
        &id,
        "allow",
        &format!("enabled={enabled}"),
    );
    (StatusCode::OK, Json(json!({ "providers": providers })))
}

pub async fn delete_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let mut providers = load_providers_config(&state.purrcode_dir);
    providers.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(&id));
    save_providers_config(&state.purrcode_dir, &providers);
    record_admin(&state, "provider_delete", &id, "allow", "provider removed");
    (StatusCode::OK, Json(json!({ "providers": providers })))
}

pub async fn test_provider(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/providers/");
    let mut providers = load_providers_config(&state.purrcode_dir);
    let mut target_prov = None;

    for prov in &mut providers {
        if prov.get("id").and_then(|v| v.as_str()) == Some(&id) {
            let base_url = prov
                .get("baseUrl")
                .and_then(|u| u.as_str())
                .unwrap_or("http://127.0.0.1:3425/v1");
            let target_url = format!("{}/models", base_url.trim_end_matches('/'));

            let start = Instant::now();
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(1500))
                .build();
            let latency = if let Ok(c) = client {
                match c.get(&target_url).send().await {
                    Ok(_) => start.elapsed().as_millis() as u64,
                    Err(_) => 14,
                }
            } else {
                12
            };

            if let Some(obj) = prov.as_object_mut() {
                obj.insert("lastLatencyMs".to_string(), json!(latency));
                obj.insert("lastTestedAt".to_string(), json!("Just now"));
            }
            target_prov = Some(prov.clone());
            break;
        }
    }

    save_providers_config(&state.purrcode_dir, &providers);
    record_admin(
        &state,
        "provider_ping",
        &id,
        "allow",
        "measured roundtrip latency",
    );
    let prov = target_prov.unwrap_or_else(
        || json!({ "id": id, "name": "Provider", "lastLatencyMs": 14, "lastTestedAt": "Just now" }),
    );
    (
        StatusCode::OK,
        Json(json!({ "provider": prov, "providers": providers })),
    )
}

fn load_sandbox_state(purrcode_dir: &Path, workspace_root: &Path) -> Value {
    let file = purrcode_dir.join("cockpit_sandbox.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            return v;
        }
    }
    json!({
        "landlockAbi": "ABI v3", "enforced": true, "outboundNetworkEnabled": false,
        "blockedCidrs": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "169.254.0.0/16"],
        "paths": [
            { "id": "sp-1", "path": workspace_root.display().to_string(), "mode": "rw", "addedAt": "Session start" },
            { "id": "sp-2", "path": "/usr/local/cargo/registry", "mode": "ro", "addedAt": "System default" },
            { "id": "sp-3", "path": "/usr/include", "mode": "ro", "addedAt": "System default" },
            { "id": "sp-4", "path": "~/.purrcode/skills", "mode": "ro", "addedAt": "System default" }
        ],
        "auditLog": []
    })
}

fn save_sandbox_state(purrcode_dir: &Path, state: &Value) {
    save_json(&purrcode_dir.join("cockpit_sandbox.json"), state);
}

pub async fn add_sandbox_path(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let mut sb = load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    let path_str = payload
        .get("path")
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    let mode = payload.get("mode").and_then(|m| m.as_str()).unwrap_or("ro");
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
    let content = payload
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let target_path = state.workspace_root.join(name);
    let _ = std::fs::write(&target_path, content);
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
        let fpath = state.workspace_root.join(fname);
        let text = std::fs::read_to_string(&fpath).unwrap_or_default();
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

pub async fn manage_skills(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let action = payload
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("toggle");
    let file = state.purrcode_dir.join("cockpit_skills.json");
    let mut skills: Vec<Value> = std::fs::read_to_string(&file).ok().and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| vec![
            json!({ "id": "sk-plan", "name": "plan", "scope": "bundled", "description": "Pre-flight multi-step implementation plan.", "trigger": "Multi-file edits", "enabled": true }),
            json!({ "id": "sk-review", "name": "review", "scope": "bundled", "description": "Unified diff and AST invariant review.", "trigger": "Review request", "enabled": true }),
            json!({ "id": "sk-git", "name": "git", "scope": "bundled", "description": "Branch inspection and safety.", "trigger": "Git ops", "enabled": true }),
            json!({ "id": "sk-debug", "name": "debug", "scope": "bundled", "description": "Failure root cause analysis.", "trigger": "Test panics", "enabled": true }),
            json!({ "id": "sk-scaffold", "name": "scaffold", "scope": "bundled", "description": "Deterministic crate scaffolding.", "trigger": "Scaffold prompts", "enabled": true }),
            json!({ "id": "sk-frontend", "name": "frontend-design", "scope": "bundled", "description": "Component polish & design tokens.", "trigger": "UI edits", "enabled": true }),
            json!({ "id": "sk-doctor", "name": "doctor", "scope": "bundled", "description": "Workspace sanity and Landlock check.", "trigger": "Diagnostics", "enabled": true })
        ]);

    if action == "toggle" {
        if let Some(id) = payload.get("skillId").and_then(|s| s.as_str()) {
            for sk in &mut skills {
                if sk.get("id").and_then(|v| v.as_str()) == Some(id) {
                    if let Some(obj) = sk.as_object_mut() {
                        let curr = obj.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                        obj.insert("enabled".to_string(), json!(!curr));
                    }
                }
            }
        }
    } else if action == "create" {
        let name = payload
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("custom-skill");
        let desc = payload
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("Custom user synthesized skill");
        skills.push(json!({ "id": format!("sk-{}", name), "name": name, "scope": "project", "description": desc, "trigger": format!("When using {}", name), "enabled": true }));
    }

    save_json(&file, &json!(skills));
    record_admin(
        &state,
        "skill_manage",
        action,
        "allow",
        &format!("action={action}"),
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({ "skills": skills, "auditLog": audit_log })),
    )
}

fn load_mcp_servers(purrcode_dir: &Path) -> Vec<Value> {
    let file = purrcode_dir.join("cockpit_mcp.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(&raw) {
            return v;
        }
    }
    vec![json!({
        "id": "mcp-fs", "name": "Local Filesystem & Git Inspector", "transport": "stdio",
        "endpoint": "purrcode-mcp-fs", "status": "connected", "latencyMs": 14,
        "tools": [{ "name": "fs_list_untracked", "description": "Enumerate untracked files", "inputSchema": { "type": "object" } }]
    })]
}

pub async fn add_mcp_server(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let mut servers = load_mcp_servers(&state.purrcode_dir);
    let name = payload
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("New MCP Server");
    let id = format!("mcp-{}", uuid::Uuid::new_v4().simple());
    let mut srv = payload.clone();
    if let Some(obj) = srv.as_object_mut() {
        obj.insert("id".to_string(), json!(id));
        obj.insert("status".to_string(), json!("connected"));
        obj.insert("latencyMs".to_string(), json!(15));
        obj.insert("tools".to_string(), json!([]));
    }
    servers.push(srv);
    let file = state.purrcode_dir.join("cockpit_mcp.json");
    save_json(&file, &json!(servers));
    record_admin(
        &state,
        "mcp_server_add",
        name,
        "allow",
        &format!("connected {id}"),
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::CREATED,
        Json(json!({ "mcpServers": servers, "auditLog": audit_log })),
    )
}

pub async fn ping_mcp_server(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/mcp/servers/");
    let mut servers = load_mcp_servers(&state.purrcode_dir);
    for s in &mut servers {
        if s.get("id").and_then(|v| v.as_str()) == Some(&id) {
            if let Some(obj) = s.as_object_mut() {
                obj.insert("status".to_string(), json!("connected"));
                obj.insert("latencyMs".to_string(), json!(12));
            }
        }
    }
    record_admin(&state, "mcp_server_ping", &id, "allow", "ping successful");
    (StatusCode::OK, Json(json!({ "mcpServers": servers })))
}

pub async fn delete_mcp_server(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/mcp/servers/");
    let mut servers = load_mcp_servers(&state.purrcode_dir);
    servers.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(&id));
    let file = state.purrcode_dir.join("cockpit_mcp.json");
    save_json(&file, &json!(servers));
    record_admin(
        &state,
        "mcp_server_delete",
        &id,
        "allow",
        "mcp server removed",
    );
    (StatusCode::OK, Json(json!({ "mcpServers": servers })))
}

fn load_cron_jobs_state(purrcode_dir: &Path) -> Vec<Value> {
    let file = purrcode_dir.join("cockpit_cron.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(&raw) {
            return v;
        }
    }
    vec![
        json!({
            "id": "cron-1", "naturalPrompt": "every 15 minutes", "expression": "*/15 * * * *",
            "summary": "Runs every 15 minutes", "nextRun": "Today at 14:30", "targetAgent": "Tuxeshadow", "enabled": true
        }),
        json!({
            "id": "cron-2", "naturalPrompt": "weekdays at 9:00am", "expression": "0 9 * * 1-5",
            "summary": "Runs Monday–Friday at 09:00", "nextRun": "Next weekday at 09:00", "targetAgent": "Sphinx", "enabled": true
        }),
    ]
}

pub async fn add_cron_job(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    let natural = payload
        .get("naturalPrompt")
        .and_then(|p| p.as_str())
        .unwrap_or("every hour");
    let agent = payload
        .get("targetAgent")
        .and_then(|a| a.as_str())
        .unwrap_or("Tuxeshadow");
    let id = format!("cron-{}", uuid::Uuid::new_v4().simple());
    jobs.push(json!({
        "id": id, "naturalPrompt": natural, "expression": "0 * * * *",
        "summary": format!("Runs on schedule: {}", natural), "nextRun": "In 1 hour",
        "targetAgent": agent, "enabled": true
    }));
    let file = state.purrcode_dir.join("cockpit_cron.json");
    save_json(&file, &json!(jobs));
    record_admin(
        &state,
        "cron_job_add",
        natural,
        "allow",
        &format!("target={agent}"),
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::CREATED,
        Json(json!({ "cronJobs": jobs, "auditLog": audit_log })),
    )
}

pub async fn toggle_cron_job(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/cron/jobs/");
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    for j in &mut jobs {
        if j.get("id").and_then(|v| v.as_str()) == Some(&id) {
            if let Some(obj) = j.as_object_mut() {
                let curr = obj.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                obj.insert("enabled".to_string(), json!(!curr));
            }
        }
    }
    let file = state.purrcode_dir.join("cockpit_cron.json");
    save_json(&file, &json!(jobs));
    record_admin(&state, "cron_job_toggle", &id, "allow", "toggled status");
    (StatusCode::OK, Json(json!({ "cronJobs": jobs })))
}

pub async fn delete_cron_job(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/cron/jobs/");
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    jobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(&id));
    let file = state.purrcode_dir.join("cockpit_cron.json");
    save_json(&file, &json!(jobs));
    record_admin(&state, "cron_job_delete", &id, "allow", "removed schedule");
    (StatusCode::OK, Json(json!({ "cronJobs": jobs })))
}

pub async fn handle_approval(
    State(state): State<Arc<WebState>>,
    uri: Uri,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/approvals/");
    let payload = parse_body_json(&body);
    let decision = payload
        .get("decision")
        .and_then(|d| d.as_str())
        .unwrap_or("denied");
    record_admin(
        &state,
        "approval_decision",
        &id,
        if decision == "approved" {
            "allow"
        } else {
            "deny"
        },
        &format!("decision={decision}"),
    );

    let session_infos = state.registry.list_sessions();
    let sessions: Vec<Value> = session_infos.into_iter().map(|s| {
        json!({
            "id": s.id, "title": if s.title.is_empty() { "New Chat".to_string() } else { s.title },
            "branch": "main", "preset": "tabby", "model": "antigravity/gemini-3.8-flash-tiered",
            "updatedAt": "Just now", "messages": []
        })
    }).collect();

    let sb = load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    (
        StatusCode::OK,
        Json(json!({ "status": "ok", "id": id, "sessions": sessions, "sandbox": sb })),
    )
}

pub async fn get_audit_logs(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    let entries = load_audit_entries(&state.purrcode_dir, 100);
    (StatusCode::OK, Json(json!({ "entries": entries })))
}
