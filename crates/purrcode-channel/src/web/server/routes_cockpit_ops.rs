// SPDX-License-Identifier: GPL-3.0-or-later

//! Operational cockpit endpoints: skills, MCP servers, cron jobs, approvals, and audit logs.

use crate::web::server::routes::record_admin;
use crate::web::server::routes_cockpit::{
    extract_id, load_audit_entries, parse_body_json, save_json,
};
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
    let mut skills: Vec<Value> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default();

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
            .map(str::trim)
            .unwrap_or("");
        if name.is_empty() {
            record_admin(
                &state,
                "skill_manage",
                "(empty)",
                "deny",
                "missing skill name",
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "skill name is required" })),
            );
        }
        let desc = payload
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("Custom project skill");
        if let Some(reg) = &state.skill_registry {
            let content =
                format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n\n{desc}\n");
            if let Err(e) = reg
                .create_skill(
                    name,
                    desc,
                    &content,
                    purrcode_api::skill::SkillScope::Project,
                )
                .await
            {
                record_admin(&state, "skill_manage", name, "deny", &e.to_string());
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e.to_string() })),
                );
            }
        }
        skills.push(json!({
            "id": format!("sk-{name}"),
            "name": name,
            "scope": "project",
            "description": desc,
            "trigger": format!("When using {name}"),
            "enabled": true
        }));
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
    Vec::new()
}

fn command_exists_on_path(cmd: &str) -> bool {
    let bin = cmd.split_whitespace().next().unwrap_or("");
    if bin.is_empty() {
        return false;
    }
    let p = Path::new(bin);
    if p.is_absolute() || bin.contains('/') {
        return p.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
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
        .map(str::trim)
        .unwrap_or("");
    let endpoint = payload
        .get("endpoint")
        .and_then(|e| e.as_str())
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() || endpoint.is_empty() {
        record_admin(
            &state,
            "mcp_server_add",
            name,
            "deny",
            "missing name or endpoint",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name and endpoint are required" })),
        );
    }
    let id = format!("mcp-{}", uuid::Uuid::new_v4().simple());
    let mut srv = payload.clone();
    if let Some(obj) = srv.as_object_mut() {
        obj.insert("id".to_string(), json!(id));
        obj.insert("status".to_string(), json!("disconnected"));
        obj.insert("latencyMs".to_string(), json!(0));
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
        &format!("registered {id}"),
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
    let Some(idx) = servers
        .iter()
        .position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id))
    else {
        record_admin(&state, "mcp_server_ping", &id, "deny", "server not found");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("MCP server '{id}' not found") })),
        );
    };

    let transport = servers[idx]
        .get("transport")
        .and_then(|v| v.as_str())
        .unwrap_or("stdio")
        .to_string();
    let endpoint = servers[idx]
        .get("endpoint")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let start = Instant::now();
    let (ok, detail) = if transport == "sse" {
        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            (
                false,
                "SSE endpoint must use http:// or https://".to_string(),
            )
        } else if let Ok(c) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(2000))
            .build()
        {
            match c.head(&endpoint).send().await {
                Ok(r) if r.status().is_success() || r.status().as_u16() == 405 => {
                    (true, "endpoint reachable".to_string())
                }
                Ok(r) => (false, format!("HTTP {}", r.status())),
                Err(e) => (false, format!("connection error: {e}")),
            }
        } else {
            (false, "http client error".to_string())
        }
    } else if command_exists_on_path(&endpoint) {
        (true, "stdio executable found".to_string())
    } else {
        (false, format!("command '{endpoint}' not found in PATH"))
    };

    let latency_ms = if ok {
        start.elapsed().as_millis() as u64
    } else {
        0
    };
    if let Some(obj) = servers[idx].as_object_mut() {
        obj.insert(
            "status".to_string(),
            json!(if ok { "connected" } else { "error" }),
        );
        obj.insert("latencyMs".to_string(), json!(latency_ms));
    }
    save_json(
        &state.purrcode_dir.join("cockpit_mcp.json"),
        &json!(servers),
    );
    record_admin(
        &state,
        "mcp_server_ping",
        &id,
        if ok { "allow" } else { "deny" },
        &detail,
    );
    (StatusCode::OK, Json(json!({ "mcpServers": servers })))
}

pub async fn delete_mcp_server(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/mcp/servers/");
    let mut servers = load_mcp_servers(&state.purrcode_dir);
    let before = servers.len();
    servers.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(&id));
    if servers.len() == before {
        record_admin(&state, "mcp_server_delete", &id, "deny", "server not found");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("MCP server '{id}' not found") })),
        );
    }
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
    Vec::new()
}

pub(crate) async fn load_unified_cron_jobs(purrcode_dir: &Path) -> Vec<Value> {
    let raw_cron = crate::web::server::routes::read_cron_jobs(purrcode_dir)
        .await
        .unwrap_or_default();
    let mut mapped: Vec<Value> = raw_cron
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
    for item in load_cron_jobs_state(purrcode_dir) {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !id.is_empty()
            && !mapped
                .iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(id))
        {
            mapped.push(item);
        }
    }
    mapped
}

pub async fn add_cron_job(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let payload = parse_body_json(&body);
    let natural = payload
        .get("naturalPrompt")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .unwrap_or("");
    if natural.is_empty() {
        record_admin(&state, "cron_job_add", "(empty)", "deny", "empty schedule");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "naturalPrompt is required" })),
        );
    }
    let agent = payload
        .get("targetAgent")
        .and_then(|a| a.as_str())
        .unwrap_or("Tuxeshadow");

    if let Some(reg) = &state.cron_registry {
        let input = purrcode_api::cron::CronAddInput {
            name: payload
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            schedule: natural.to_string(),
            prompt: natural.to_string(),
            allow: None,
            preset: None,
        };
        match reg.add_job(input).await {
            Ok(_created) => {
                record_admin(
                    &state,
                    "cron_job_add",
                    natural,
                    "allow",
                    &format!("target={agent}"),
                );
                let audit_log = load_audit_entries(&state.purrcode_dir, 50);
                let jobs = load_unified_cron_jobs(&state.purrcode_dir).await;
                return (
                    StatusCode::CREATED,
                    Json(json!({ "cronJobs": jobs, "auditLog": audit_log })),
                );
            }
            Err(e) => {
                record_admin(&state, "cron_job_add", natural, "deny", &e.to_string());
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e.to_string() })),
                );
            }
        }
    }

    let expr = payload
        .get("expression")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.split_whitespace().count() == 5)
        .map(str::to_string);
    let Some(expression) = expr else {
        record_admin(
            &state,
            "cron_job_add",
            natural,
            "deny",
            "cron registry unavailable and no 5-field expression provided",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "valid 5-field cron expression required when cron registry is not configured" }),
            ),
        );
    };
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    let id = format!("cron-{}", uuid::Uuid::new_v4().simple());
    jobs.push(json!({
        "id": id, "naturalPrompt": natural, "expression": expression,
        "summary": format!("Runs on schedule: {natural}"), "nextRun": "Scheduled",
        "targetAgent": agent, "enabled": true
    }));
    save_json(&state.purrcode_dir.join("cockpit_cron.json"), &json!(jobs));
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
    if let Some(reg) = &state.cron_registry {
        let existing = load_unified_cron_jobs(&state.purrcode_dir).await;
        if let Some(job) = existing
            .iter()
            .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(&id))
        {
            let curr = job.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
            if reg.set_enabled(&id, !curr).await.is_ok() {
                record_admin(&state, "cron_job_toggle", &id, "allow", "toggled status");
                let jobs = load_unified_cron_jobs(&state.purrcode_dir).await;
                return (StatusCode::OK, Json(json!({ "cronJobs": jobs })));
            }
        }
    }
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    let mut found = false;
    for j in &mut jobs {
        if j.get("id").and_then(|v| v.as_str()) == Some(&id) {
            if let Some(obj) = j.as_object_mut() {
                let curr = obj.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                obj.insert("enabled".to_string(), json!(!curr));
                found = true;
            }
        }
    }
    if !found {
        record_admin(&state, "cron_job_toggle", &id, "deny", "job not found");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("cron job '{id}' not found") })),
        );
    }
    save_json(&state.purrcode_dir.join("cockpit_cron.json"), &json!(jobs));
    record_admin(&state, "cron_job_toggle", &id, "allow", "toggled status");
    let unified = load_unified_cron_jobs(&state.purrcode_dir).await;
    (StatusCode::OK, Json(json!({ "cronJobs": unified })))
}

pub async fn delete_cron_job(
    State(state): State<Arc<WebState>>,
    uri: Uri,
) -> (StatusCode, Json<Value>) {
    let id = extract_id(uri.path(), "/api/cron/jobs/");
    if let Some(reg) = &state.cron_registry {
        if reg.remove_job(&id).await.is_ok() {
            record_admin(&state, "cron_job_delete", &id, "allow", "removed schedule");
            let jobs = load_unified_cron_jobs(&state.purrcode_dir).await;
            return (StatusCode::OK, Json(json!({ "cronJobs": jobs })));
        }
    }
    let mut jobs = load_cron_jobs_state(&state.purrcode_dir);
    let before = jobs.len();
    jobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(&id));
    if jobs.len() == before {
        record_admin(&state, "cron_job_delete", &id, "deny", "job not found");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("cron job '{id}' not found") })),
        );
    }
    save_json(&state.purrcode_dir.join("cockpit_cron.json"), &json!(jobs));
    record_admin(&state, "cron_job_delete", &id, "allow", "removed schedule");
    let unified = load_unified_cron_jobs(&state.purrcode_dir).await;
    (StatusCode::OK, Json(json!({ "cronJobs": unified })))
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
    let approved = decision == "approved";
    let resolved =
        if let Some(session) = crate::web::server::approval_session(&state.registry, &payload) {
            session.channel.resolve_approval(&id, approved).await
        } else {
            false
        };
    record_admin(
        &state,
        "approval_decision",
        &id,
        if approved && resolved {
            "allow"
        } else {
            "deny"
        },
        &format!("decision={decision} resolved={resolved}"),
    );
    let audit_log = load_audit_entries(&state.purrcode_dir, 50);
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "id": id,
            "decision": decision,
            "resolved": resolved,
            "auditLog": audit_log
        })),
    )
}

pub async fn get_audit_logs(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    let entries = load_audit_entries(&state.purrcode_dir, 100);
    (StatusCode::OK, Json(json!({ "entries": entries })))
}
