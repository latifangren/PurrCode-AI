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

fn extract_ui_config(config: Option<&Value>) -> purrcode_config::UiConfig {
    config
        .and_then(|c| c.get("ui"))
        .and_then(|u| serde_json::from_value::<purrcode_config::UiConfig>(u.clone()).ok())
        .unwrap_or_default()
}

fn serialize_ui_config(ui: &purrcode_config::UiConfig) -> Value {
    json!({
        "theme": ui.theme,
        "density": ui.density,
        "showReasoning": ui.show_reasoning,
        "reasoningCollapsed": ui.reasoning_collapsed,
        "showToolArgs": ui.show_tool_args,
        "toolPreviewLines": ui.tool_preview_lines,
        "streamingMode": ui.streaming_mode,
        "showTimestamps": ui.show_timestamps,
        "showTokenUsage": ui.show_token_usage,
        "companionName": ui.companion_name,
        "companionPersonality": ui.companion_personality,
        "statuslineFields": ui.statusline_fields,
        "pinnedFiles": ui.pinned_files,
        "quickCommands": ui.quick_commands.iter().map(|q| json!({
            "name": q.name,
            "description": q.description,
            "template": q.template,
        })).collect::<Vec<_>>(),
        "autoCompactThreshold": ui.auto_compact_threshold,
        "customThemeTokens": ui.custom_theme_tokens,
    })
}

fn collect_workspace_files(workspace_root: &Path, pinned: &[String]) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for p in pinned {
        if !p.trim().is_empty() && !files.contains(p) {
            files.push(p.clone());
        }
    }
    fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        if out.len() >= 48 || depth > 2 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            if out.len() >= 48 {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "dist"
            {
                continue;
            }
            let path = entry.path();
            if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base) {
                    let s = rel.to_string_lossy().replace('\\', "/");
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            } else if path.is_dir() {
                walk(base, &path, depth + 1, out);
            }
        }
    }
    walk(workspace_root, workspace_root, 0, &mut files);
    files
}

fn discover_presets(workspace_root: &Path, purrcode_dir: &Path) -> Vec<Value> {
    let mut presets: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let dirs = [
        workspace_root.join(".purrcode").join("presets"),
        workspace_root.join("presets"),
        purrcode_dir.join("presets"),
    ];
    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "jsonc" && ext != "json" {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !seen.insert(stem.to_string()) {
                continue;
            }
            let desc = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&clean_jsonc(&raw)).ok())
                .and_then(|v| {
                    v.get("description")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            presets.push(json!({
                "id": stem,
                "label": stem,
                "description": desc,
            }));
        }
    }
    if presets.is_empty() {
        for (id, desc) in [
            ("tabby", "Supervised mode (ask on sensitive writes)"),
            ("carrier", "Focused autonomous edits in workspace"),
            ("oyen", "High-autonomy mode"),
        ] {
            presets.push(json!({ "id": id, "label": id, "description": desc }));
        }
    }
    presets
}

fn map_tool_kind(name: &str) -> &'static str {
    match name {
        "read" => "read",
        "grep" | "glob" | "find" => "grep",
        "edit" | "write" => "edit",
        "bash" => "bash",
        _ => "mcp",
    }
}

fn build_session_messages(
    session_id: &str,
    session_model: &str,
    transcript: Vec<purrcode_api::provider::ChatMessage>,
) -> Vec<Value> {
    let mut tool_outputs = std::collections::HashMap::<String, String>::new();
    for msg in &transcript {
        if msg.role == purrcode_api::provider::MessageRole::Tool {
            if let Some(id) = &msg.tool_call_id {
                tool_outputs.insert(id.clone(), msg.content.clone());
            }
        }
    }

    let mut out = Vec::new();
    for (idx, msg) in transcript.into_iter().enumerate() {
        let is_user = msg.role == purrcode_api::provider::MessageRole::User;
        let is_assistant = msg.role == purrcode_api::provider::MessageRole::Assistant;
        if !is_user && !is_assistant {
            continue;
        }
        let tool_steps: Vec<Value> = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let output = tool_outputs.get(&tc.id).cloned().unwrap_or_default();
                let preview = if output.trim().is_empty() {
                    tc.arguments.clone()
                } else {
                    output.chars().take(240).collect()
                };
                json!({
                    "id": tc.id,
                    "tool": map_tool_kind(&tc.name),
                    "target": tc.name,
                    "summary": preview,
                    "durationMs": 0,
                    "status": "completed",
                })
            })
            .collect();
        if msg.content.trim().is_empty() && tool_steps.is_empty() {
            continue;
        }
        let mut obj = json!({
            "id": format!("msg-{}-{}", session_id, idx + 1),
            "sender": if is_user { "user" } else { "agent" },
            "agentRole": if is_user { Value::Null } else { json!("Meowstro") },
            "agentTitle": if is_user { Value::Null } else { json!("Orchestrator") },
            "modelUsed": if is_user { Value::Null } else { json!(session_model) },
            "timestamp": "Recent",
            "content": msg.content,
        });
        if !tool_steps.is_empty() {
            if let Some(map) = obj.as_object_mut() {
                map.insert("toolSteps".to_string(), json!(tool_steps));
            }
        }
        out.push(obj);
    }
    out
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

/// Builds subagent routing specifications by merging saved overrides from
/// `cockpit_routing.json` onto the canonical 6 Clowder roles so partial or
/// legacy override files never drop required UI/runtime fields.
fn build_routing(primary_model: &str, purrcode_dir: &Path) -> Vec<Value> {
    let mut defaults = vec![
        json!({
            "role": "Meowstro",
            "responsibility": "Lead Orchestrator & Task Planner",
            "primaryModel": primary_model,
            "fallbackModel": "",
            "reasoningEffort": "auto",
            "temperature": Value::Null,
            "maxTokens": 8192,
            "status": "active"
        }),
        json!({
            "role": "Tinker",
            "responsibility": "Code Implementer & Surgical Patch Fixer",
            "primaryModel": "",
            "fallbackModel": "",
            "reasoningEffort": "high",
            "temperature": Value::Null,
            "maxTokens": 16384,
            "status": "active"
        }),
        json!({
            "role": "Whisker",
            "responsibility": "Codebase Explorer & Fast Grep/AST Recon",
            "primaryModel": "",
            "fallbackModel": "",
            "reasoningEffort": "low",
            "temperature": Value::Null,
            "maxTokens": 4096,
            "status": "idle"
        }),
        json!({
            "role": "Sphinx",
            "responsibility": "Architecture Oracle & Security Reviewer",
            "primaryModel": "",
            "fallbackModel": "",
            "reasoningEffort": "high",
            "temperature": Value::Null,
            "maxTokens": 8192,
            "status": "idle"
        }),
        json!({
            "role": "Calico",
            "responsibility": "Editorial UI Designer & Component Architect",
            "primaryModel": "",
            "fallbackModel": "",
            "reasoningEffort": "medium",
            "temperature": Value::Null,
            "maxTokens": 8192,
            "status": "idle"
        }),
        json!({
            "role": "Tuxeshadow",
            "responsibility": "Background Verification & Test Harness Runner",
            "primaryModel": "",
            "fallbackModel": "",
            "reasoningEffort": "high",
            "temperature": Value::Null,
            "maxTokens": 4096,
            "status": "idle"
        }),
    ];

    let routing_file = purrcode_dir.join("cockpit_routing.json");
    if let Ok(raw) = std::fs::read_to_string(&routing_file) {
        if let Ok(saved) = serde_json::from_str::<Vec<Value>>(&raw) {
            for item in saved {
                let Some(role) = item.get("role").and_then(|v| v.as_str()) else {
                    continue;
                };
                if let Some(target) = defaults.iter_mut().find(|d| {
                    d.get("role")
                        .and_then(|v| v.as_str())
                        .map(|r| r.eq_ignore_ascii_case(role))
                        .unwrap_or(false)
                }) {
                    if let (Some(dst), Some(src)) = (target.as_object_mut(), item.as_object()) {
                        for key in [
                            "responsibility",
                            "primaryModel",
                            "fallbackModel",
                            "reasoningEffort",
                            "temperature",
                            "maxTokens",
                            "status",
                        ] {
                            if let Some(val) = src.get(key) {
                                if !val.is_null() || key == "temperature" {
                                    dst.insert(key.to_string(), val.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    defaults
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

/// Builds MCP servers list from configuration and cockpit_mcp.json.
fn extract_mcp_servers(config: Option<&Value>, purrcode_dir: &Path) -> Vec<Value> {
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
                    "status": "disconnected",
                    "latencyMs": 0,
                    "tools": []
                }));
            }
        }
    }
    let cockpit_file = purrcode_dir.join("cockpit_mcp.json");
    if let Ok(raw) = std::fs::read_to_string(&cockpit_file) {
        if let Ok(saved) = serde_json::from_str::<Vec<Value>>(&raw) {
            for item in saved {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if !id.is_empty()
                    && !servers
                        .iter()
                        .any(|s| s.get("id").and_then(|v| v.as_str()) == Some(id))
                {
                    servers.push(item);
                }
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

    let (raw_providers, preset, primary_model) =
        super::routes_providers::extract_providers_merged(config.as_ref(), &state.purrcode_dir);
    let providers = super::routes_providers::sanitize_providers_for_client(&raw_providers);
    let routing = build_routing(&primary_model, &state.purrcode_dir);

    // Sessions from WebSessionRegistry
    let session_infos = state.registry.list_sessions();
    let mut sessions = Vec::new();

    for info in session_infos {
        let meta = state.registry.load_session_meta(&info.id);
        let session_model = meta
            .model
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| primary_model.clone());
        let session_preset = meta
            .preset
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| preset.clone());
        let session_title = meta.title.filter(|t| !t.is_empty()).unwrap_or_else(|| {
            if info.title.is_empty() {
                info.id.clone()
            } else {
                info.title
            }
        });

        let transcript = state.registry.transcript(&info.id).unwrap_or_default();
        let messages = build_session_messages(&info.id, &session_model, transcript);

        sessions.push(json!({
            "id": info.id,
            "title": session_title,
            "branch": branch.clone(),
            "preset": session_preset,
            "model": session_model,
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

    // Cron jobs (unified from cron store + cockpit_cron fallback)
    let cron_jobs = super::routes_cockpit_ops::load_unified_cron_jobs(&state.purrcode_dir).await;

    let skills = load_skills(&state.purrcode_dir).await;
    let mcp_servers = extract_mcp_servers(config.as_ref(), &state.purrcode_dir);

    // Load audit logs, preferences, UI config, workspace files, presets, and honest sandbox state
    let audit_entries = super::routes_cockpit::load_audit_entries(&state.purrcode_dir, 50);
    let preferences = super::routes_cockpit::load_preferences(&state.purrcode_dir);
    let ui_cfg = extract_ui_config(config.as_ref());
    let workspace_files = collect_workspace_files(&state.workspace_root, &ui_cfg.pinned_files);
    let presets = discover_presets(&state.workspace_root, &state.purrcode_dir);

    let mut sandbox =
        super::routes_cockpit::load_sandbox_state(&state.purrcode_dir, &state.workspace_root);
    if let Some(obj) = sandbox.as_object_mut() {
        obj.insert("auditLog".to_string(), json!(audit_entries));
    }

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
        "ui": serialize_ui_config(&ui_cfg),
        "workspaceFiles": workspace_files,
        "presets": presets,
    }))
}
