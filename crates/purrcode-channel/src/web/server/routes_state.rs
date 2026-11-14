// SPDX-License-Identifier: GPL-3.0-or-later

//! Endpoint for the unified cockpit state (`GET /api/state`).
//!
//! Provides the complete real-time runtime state for the WebUI:
//! - Workspace metadata (name, path, git branch)
//! - Configured providers and model routing from `purrcode.jsonc`
//! - Active web sessions from `WebSessionRegistry`
//! - Real identity/memory files (`MEMORY.md`, `USER.md`, `SOUL.md`)
//! - Registered skills and MCP servers
//! - Scheduled cron jobs from `jobs.jsonc`
//! - Sandbox security state

use crate::web::server::routes::read_cron_jobs;
use crate::web::server::WebState;
use axum::{extract::State, Json};
use purrcode_config::jsonc::clean_jsonc;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Resolves the current git branch name from the `.git/HEAD` file if present.
fn detect_git_branch(workspace_root: &Path) -> String {
    let head_path = workspace_root.join(".git").join("HEAD");
    if let Ok(content) = std::fs::read_to_string(&head_path) {
        let trimmed = content.trim();
        if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
            return branch.to_string();
        }
    }
    "main".to_string()
}

/// Reads memory file content from workspace root or purrcode dir, or provides a clean default template.
async fn read_memory_file(
    workspace_root: &Path,
    purrcode_dir: &Path,
    name: &str,
    description: &str,
) -> Value {
    let ws_path = workspace_root.join(name);
    let purr_path = purrcode_dir.join(name);

    let (content, updated_at) = if ws_path.exists() {
        let text = tokio::fs::read_to_string(&ws_path)
            .await
            .unwrap_or_default();
        let mtime = std::fs::metadata(&ws_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.format("%Y-%m-%d %H:%M").to_string()
            })
            .unwrap_or_else(|| "Saved".to_string());
        (text, mtime)
    } else if purr_path.exists() {
        let text = tokio::fs::read_to_string(&purr_path)
            .await
            .unwrap_or_default();
        let mtime = std::fs::metadata(&purr_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.format("%Y-%m-%d %H:%M").to_string()
            })
            .unwrap_or_else(|| "Saved".to_string());
        (text, mtime)
    } else {
        let default_content = match name {
            "MEMORY.md" => "# PurrCode-AI Project Memory\n\n## Architectural Invariants\n- ADR-011 Modularity: Keep every source module focused and under 800 lines.\n- Supervised Mode (Tabby): All destructive filesystem mutations require operator confirmation.\n".to_string(),
            "USER.md" => "# User Preferences\n\n- Language: Indonesian for chat, English for code/comments.\n- Style: Clean, idiomatic Rust with full test coverage.\n".to_string(),
            "SOUL.md" => "# PurrCode Soul\n\nYou are PurrCode AI, a modular, lightweight, high-performance feline AI engineer.\n".to_string(),
            _ => format!("# {}\n\nNo memory record created yet.\n", name),
        };
        (default_content, "Template".to_string())
    };

    json!({
        "name": name,
        "description": description,
        "content": content,
        "updatedAt": updated_at,
    })
}

/// Reads configuration from `purrcode.yaml` / `purrcode.yml` or `purrcode.jsonc` / `purrcode.json`
/// in workspace root or purrcode dir (YAML prioritized).
async fn load_config_json(workspace_root: &Path, purrcode_dir: &Path) -> Option<Value> {
    let candidates = [
        workspace_root.join(".purrcode").join("purrcode.yaml"),
        workspace_root.join(".purrcode").join("purrcode.yml"),
        workspace_root.join(".purrcode").join("purrcode.jsonc"),
        workspace_root.join(".purrcode").join("purrcode.json"),
        workspace_root.join("purrcode.yaml"),
        workspace_root.join("purrcode.yml"),
        workspace_root.join("purrcode.jsonc"),
        workspace_root.join("purrcode.json"),
        purrcode_dir.join("purrcode.yaml"),
        purrcode_dir.join("purrcode.yml"),
        purrcode_dir.join("purrcode.jsonc"),
        purrcode_dir.join("purrcode.json"),
    ];

    for path in &candidates {
        if path.is_file() {
            if let Ok(raw) = tokio::fs::read_to_string(path).await {
                let is_yaml = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
                    .unwrap_or(false);

                if is_yaml {
                    if let Ok(val) = serde_yaml::from_str::<Value>(&raw) {
                        return Some(val);
                    }
                } else {
                    let cleaned = clean_jsonc(&raw);
                    if let Ok(val) = serde_json::from_str::<Value>(&cleaned) {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

/// Builds provider list from configuration or default fallback.
fn extract_providers(config: Option<&Value>) -> (Vec<Value>, String, String) {
    let mut providers = Vec::new();
    let mut active_preset = "tabby".to_string();
    let mut primary_model = "antigravity/gemini-3.8-flash-tiered".to_string();

    if let Some(cfg) = config {
        if let Some(p) = cfg.get("preset").and_then(|v| v.as_str()) {
            active_preset = p.to_string();
        }
        if let Some(m) = cfg.get("model").and_then(|v| v.as_str()) {
            primary_model = m.split_once(':').map(|(_, b)| b).unwrap_or(m).to_string();
        }

        if let Some(prov_map) = cfg.get("providers").and_then(|v| v.as_object()) {
            for (prov_id, prov_val) in prov_map {
                let api_type = prov_val
                    .get("api")
                    .and_then(|v| v.as_str())
                    .unwrap_or("openai");
                let protocol = if api_type.contains("gemini") {
                    "gemini"
                } else if api_type.contains("anthropic") {
                    "anthropic"
                } else if api_type.contains("deepseek") {
                    "deepseek"
                } else if api_type.contains("ollama") {
                    "ollama"
                } else if prov_id == "custom" {
                    "custom"
                } else {
                    "openai"
                };

                let base_url = prov_val
                    .get("base_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http://127.0.0.1:3425/v1")
                    .to_string();

                let mut models = Vec::new();
                let mut max_context = 1048576;
                if let Some(models_obj) = prov_val.get("models").and_then(|v| v.as_object()) {
                    for (m_name, m_cfg) in models_obj {
                        models.push(m_name.clone());
                        if let Some(cw) = m_cfg.get("context_window").and_then(|v| v.as_u64()) {
                            if cw > max_context {
                                max_context = cw;
                            }
                        }
                    }
                }
                if models.is_empty() {
                    models.push(primary_model.clone());
                }

                let default_model = if models.contains(&primary_model) {
                    primary_model.clone()
                } else {
                    models
                        .first()
                        .cloned()
                        .unwrap_or_else(|| primary_model.clone())
                };

                let prov_name = match prov_id.as_str() {
                    "magpie" => "Local Magpie / OpenAI-Compatible".to_string(),
                    "custom" => "Custom Gateway Endpoint".to_string(),
                    _ => format!("{prov_id} Gateway"),
                };

                providers.push(json!({
                    "id": format!("prov-{prov_id}"),
                    "name": prov_name,
                    "protocol": protocol,
                    "baseUrl": base_url,
                    "defaultModel": default_model,
                    "models": models,
                    "contextWindow": max_context,
                    "enabled": true,
                    "lastLatencyMs": 12,
                    "lastTestedAt": "Active",
                }));
            }
        }
    }

    if providers.is_empty() {
        providers.push(json!({
            "id": "prov-magpie",
            "name": "Local Magpie / OpenAI-Compatible",
            "protocol": "openai",
            "baseUrl": "http://127.0.0.1:3425/v1",
            "defaultModel": primary_model,
            "models": [primary_model],
            "contextWindow": 1048576,
            "enabled": true,
            "lastLatencyMs": 12,
            "lastTestedAt": "Active",
        }));
    }

    (providers, active_preset, primary_model)
}

/// Builds subagent routing specifications using the active model.
fn build_routing(primary_model: &str) -> Vec<Value> {
    vec![
        json!({
            "role": "Meowstro",
            "responsibility": "Lead Orchestrator & Task Planner",
            "primaryModel": primary_model,
            "fallbackModel": "deepseek-v3",
            "temperature": 0.2,
            "maxTokens": 8192,
            "status": "active"
        }),
        json!({
            "role": "Tinker",
            "responsibility": "Code Implementer & Surgical Patch Fixer",
            "primaryModel": primary_model,
            "fallbackModel": "qwen-2.5-coder-32b",
            "temperature": 0.1,
            "maxTokens": 16384,
            "status": "active"
        }),
        json!({
            "role": "Whisker",
            "responsibility": "Codebase Explorer & Fast Grep/AST Recon",
            "primaryModel": primary_model,
            "fallbackModel": "qwen-2.5-coder-32b",
            "temperature": 0.0,
            "maxTokens": 4096,
            "status": "idle"
        }),
        json!({
            "role": "Sphinx",
            "responsibility": "Architecture Oracle & Security Reviewer",
            "primaryModel": primary_model,
            "fallbackModel": "deepseek-r1",
            "temperature": 0.15,
            "maxTokens": 8192,
            "status": "idle"
        }),
        json!({
            "role": "Calico",
            "responsibility": "Editorial UI Designer & Component Architect",
            "primaryModel": primary_model,
            "fallbackModel": "claude-sonnet-4",
            "temperature": 0.4,
            "maxTokens": 8192,
            "status": "idle"
        }),
        json!({
            "role": "Tuxeshadow",
            "responsibility": "Background Verification & Test Harness Runner",
            "primaryModel": primary_model,
            "fallbackModel": "ollama/llama3.3",
            "temperature": 0.0,
            "maxTokens": 4096,
            "status": "idle"
        }),
    ]
}

/// Builds bundled and custom skills list.
async fn load_skills(purrcode_dir: &Path) -> Vec<Value> {
    let mut skills = vec![
        json!({
            "id": "skill-plan",
            "name": "plan",
            "scope": "bundled",
            "description": "Plan complex multi-step engineering tasks before implementation.",
            "trigger": "when planning architecture or features",
            "enabled": true
        }),
        json!({
            "id": "skill-review",
            "name": "review",
            "scope": "bundled",
            "description": "Code review and security inspection against ADR invariants.",
            "trigger": "when auditing code quality or diffs",
            "enabled": true
        }),
        json!({
            "id": "skill-git",
            "name": "git",
            "scope": "bundled",
            "description": "Safe git branch status, staging inspection, and diff verification.",
            "trigger": "when inspecting git repository state",
            "enabled": true
        }),
        json!({
            "id": "skill-debug",
            "name": "debug",
            "scope": "bundled",
            "description": "Root cause investigation, failure isolation, and diagnostics.",
            "trigger": "when diagnosing failing tests or panics",
            "enabled": true
        }),
        json!({
            "id": "skill-scaffold",
            "name": "scaffold",
            "scope": "bundled",
            "description": "Crate and module scaffolding under ADR-011 file size constraints.",
            "trigger": "when creating new modules or crates",
            "enabled": true
        }),
        json!({
            "id": "skill-frontend-design",
            "name": "frontend-design",
            "scope": "bundled",
            "description": "Frontend design systems, typographic scale, and theme polish.",
            "trigger": "when polishing UI/TUI components",
            "enabled": true
        }),
        json!({
            "id": "skill-webapp-testing",
            "name": "webapp-testing",
            "scope": "bundled",
            "description": "Web application end-to-end tests and DOM assertions.",
            "trigger": "when testing frontend components",
            "enabled": true
        }),
        json!({
            "id": "skill-doctor",
            "name": "doctor",
            "scope": "bundled",
            "description": "Workspace, toolchain, and environment diagnostic health checks.",
            "trigger": "when troubleshooting runtime environment",
            "enabled": true
        }),
    ];

    let custom_dir = purrcode_dir.join("skills");
    if custom_dir.is_dir() {
        if let Ok(mut rd) = tokio::fs::read_dir(&custom_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                    let file_stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("custom");
                    skills.push(json!({
                        "id": format!("skill-custom-{file_stem}"),
                        "name": file_stem,
                        "scope": "user",
                        "description": format!("Custom user skill {file_stem}"),
                        "trigger": format!("when using {file_stem}"),
                        "enabled": true
                    }));
                }
            }
        }
    }

    skills
}

/// Builds MCP servers list from configuration or default inspector.
fn extract_mcp_servers(config: Option<&Value>) -> Vec<Value> {
    let mut servers = Vec::new();
    if let Some(cfg) = config {
        if let Some(mcp) = cfg
            .get("mcpServers")
            .or_else(|| cfg.get("mcp_servers"))
            .and_then(|v| v.as_object())
        {
            for (id, val) in mcp {
                let name = val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let transport = val
                    .get("transport")
                    .and_then(|v| v.as_str())
                    .unwrap_or("stdio")
                    .to_string();
                let endpoint = val
                    .get("endpoint")
                    .or_else(|| val.get("command"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("local")
                    .to_string();

                servers.push(json!({
                    "id": format!("mcp-{id}"),
                    "name": name,
                    "transport": transport,
                    "endpoint": endpoint,
                    "status": "connected",
                    "latencyMs": 15,
                    "tools": []
                }));
            }
        }
    }
    servers
}

/// Handler for `GET /api/state`.
pub async fn state_handler(State(state): State<Arc<WebState>>) -> Json<Value> {
    let project_path = state.workspace_root.display().to_string();
    let project_name = state
        .workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("PurrCode-AI")
        .to_string();

    let branch = detect_git_branch(&state.workspace_root);
    let config = load_config_json(&state.workspace_root, &state.purrcode_dir).await;

    let (providers, preset, primary_model) = extract_providers(config.as_ref());
    let routing = build_routing(&primary_model);

    // Sessions from WebSessionRegistry
    let session_infos = state.registry.list_sessions();
    let mut sessions = Vec::new();

    for info in session_infos {
        let transcript = state.registry.transcript(&info.id).unwrap_or_default();
        let messages: Vec<Value> = transcript
            .into_iter()
            .enumerate()
            .map(|(idx, msg)| {
                let is_user = msg.role == purrcode_api::provider::MessageRole::User;
                json!({
                    "id": format!("msg-{}-{}", info.id, idx + 1),
                    "sender": if is_user { "user" } else { "agent" },
                    "agentRole": if is_user { Value::Null } else { json!("Meowstro") },
                    "agentTitle": if is_user { Value::Null } else { json!("Orchestrator") },
                    "modelUsed": if is_user { Value::Null } else { json!(primary_model) },
                    "timestamp": "Recent",
                    "content": msg.content,
                })
            })
            .collect();

        sessions.push(json!({
            "id": info.id,
            "title": if info.title.is_empty() { info.id.clone() } else { info.title },
            "branch": branch.clone(),
            "preset": preset.clone(),
            "model": primary_model.clone(),
            "updatedAt": "Recent",
            "messages": messages,
        }));
    }

    if sessions.is_empty() {
        sessions.push(json!({
            "id": "purr_web",
            "title": "New Chat",
            "branch": branch.clone(),
            "preset": preset.clone(),
            "model": primary_model.clone(),
            "updatedAt": "Just now",
            "messages": [],
        }));
    }

    let active_session_id = sessions
        .first()
        .and_then(|s| s.get("id").and_then(|v| v.as_str()))
        .unwrap_or("purr_web")
        .to_string();

    // Memory files
    let memory_files = vec![
        read_memory_file(
            &state.workspace_root,
            &state.purrcode_dir,
            "MEMORY.md",
            "Persistent architectural facts, verified patterns, and project conventions.",
        )
        .await,
        read_memory_file(
            &state.workspace_root,
            &state.purrcode_dir,
            "USER.md",
            "Operator preferences, coding style, and workflow defaults.",
        )
        .await,
        read_memory_file(
            &state.workspace_root,
            &state.purrcode_dir,
            "SOUL.md",
            "Core personality, behavioral boundaries, and tone directives.",
        )
        .await,
    ];

    // Cron jobs
    let raw_cron = read_cron_jobs(&state.purrcode_dir)
        .await
        .unwrap_or_default();
    let cron_jobs: Vec<Value> = raw_cron
        .into_iter()
        .map(|j| {
            let id = j
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("cron-job")
                .to_string();
            let prompt = j
                .get("prompt")
                .or_else(|| j.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Scheduled Task")
                .to_string();
            let schedule = j
                .get("schedule")
                .and_then(|v| v.as_str())
                .unwrap_or("@hourly")
                .to_string();
            let next_run = j
                .get("next_run")
                .and_then(|v| v.as_str())
                .unwrap_or("Scheduled")
                .to_string();
            let enabled = j.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

            json!({
                "id": id,
                "naturalPrompt": prompt,
                "expression": schedule,
                "summary": prompt,
                "nextRun": next_run,
                "targetAgent": "Tuxeshadow",
                "enabled": enabled,
            })
        })
        .collect();

    let skills = load_skills(&state.purrcode_dir).await;
    let mcp_servers = extract_mcp_servers(config.as_ref());

    // Load audit logs and preferences
    let audit_entries = super::routes_cockpit::load_audit_entries(&state.purrcode_dir, 50);
    let preferences = super::routes_cockpit::load_preferences(&state.purrcode_dir);

    // Sandbox state
    let sandbox = json!({
        "landlockAbi": "v3",
        "enforced": true,
        "outboundNetworkEnabled": true,
        "blockedCidrs": [
            "169.254.169.254/32",
            "127.0.0.0/8",
            "100.64.0.0/10"
        ],
        "paths": [
            {
                "path": project_path,
                "access": "read-write",
                "reason": "Workspace root"
            },
            {
                "path": "/tmp",
                "access": "read-write",
                "reason": "Scratch builds & temp"
            },
            {
                "path": "/usr",
                "access": "read-only",
                "reason": "System binaries and toolchains"
            }
        ],
        "auditLog": audit_entries
    });

    Json(json!({
        "schemaVersion": 1,
        "projectName": project_name,
        "projectPath": project_path,
        "activeSessionId": active_session_id,
        "sessions": sessions,
        "routing": routing,
        "providers": providers,
        "sandbox": sandbox,
        "memoryFiles": memory_files,
        "skills": skills,
        "mcpServers": mcp_servers,
        "cronJobs": cron_jobs,
        "preferences": preferences,
    }))
}
