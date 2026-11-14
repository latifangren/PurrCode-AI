// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP handlers for the WebUI (v0.4 B2, Axum).
//!
//! The route table lives in `server/mod.rs`; this module owns the
//! plain JSON handlers. Every handler preserves the hand-rolled
//! server's observable contract (R3 + B1):
//!
//! * `POST` bodies are read as raw bytes and parsed as JSON by hand
//!   — no `Content-Type: application/json` requirement, and a
//!   malformed body is a 400 (the status is as before; the body is
//!   now a JSON `{"error": ...}` where the old server sent an empty
//!   body).
//! * `POST /api/prompt` answers 200 `{"status":"dispatched"}` at
//!   once and runs the turn in a spawned task; a missing dispatcher
//!   is a 503.
//! * `POST /api/skills/approve|reject` are resolve-only: the WebUI
//!   never promotes a draft by moving files itself.
//! * `GET /api/memory` picks the file by substring (`SOUL.md` /
//!   `USER.md`, else `MEMORY.md`); a missing file answers with a
//!   template instead of a 404.

use crate::web::channel::SKILL_DRAFT_KEY_PREFIX;
use crate::web::html::web_index_html;
use crate::web::server::{approval_session, WebState};
use crate::web::sessions::WebSessionRegistry;
use axum::{
    body::Bytes,
    extract::State,
    http::{StatusCode, Uri},
    response::{Html, IntoResponse, Json},
};
use purrcode_api::plugin::PluginMountError;
use purrcode_config::jsonc::clean_jsonc;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// `GET /` and `GET /index.html` — the single-page WebUI.
pub async fn index() -> Html<String> {
    Html(web_index_html())
}

/// `GET /health`, `/api/status`, `/api/health`.
pub async fn status(State(state): State<Arc<WebState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "channel": "web",
        "workspace": state.workspace_root.display().to_string(),
        "purrcode_dir": state.purrcode_dir.display().to_string()
    }))
}

/// Shared reader for the cron store: the plain job table (`GET
/// /api/cron`, v0.4 B6) and the overview's next-job pick (v0.4 B7)
/// project the same file, so they share one parse — the store
/// stays the single source of truth and the two handlers cannot
/// drift apart. A missing file means no jobs yet (`Ok([])`); a
/// present-but-unparsable file is `Err` carrying the exact message
/// the cron table reports as a 500 (B6 r1 P3-4: never hide a
/// corrupt store behind an empty list). The overview degrades
/// that `Err` to a warning instead of failing the whole response.
pub(crate) async fn read_cron_jobs(purrcode_dir: &Path) -> Result<Vec<Value>, String> {
    let jobs_path = purrcode_dir.join("cron").join("jobs.jsonc");
    let Ok(raw) = tokio::fs::read_to_string(&jobs_path).await else {
        return Ok(Vec::new());
    };
    let cleaned = clean_jsonc(&raw);
    let Ok(val) = serde_json::from_str::<Value>(&cleaned) else {
        return Err("cron jobs file is invalid JSON".to_string());
    };
    let Some(list) = val.get("jobs").and_then(|j| j.as_array()) else {
        return Err("cron jobs file has no jobs array".to_string());
    };
    Ok(list
        .iter()
        .map(|j| {
            let schedule = j
                .get("schedule")
                .and_then(|s| s.get("display"))
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let tools = j.get("tools").cloned().unwrap_or_else(|| json!([]));
            json!({
                "id": j.get("id").cloned().unwrap_or(Value::Null),
                "name": j.get("name").cloned().unwrap_or(Value::Null),
                "schedule": schedule,
                "prompt": j.get("prompt").cloned().unwrap_or(Value::Null),
                "allowed_tools": tools,
                "next_run": j.get("next_run").cloned().unwrap_or(Value::Null),
                "enabled": j.get("enabled").cloned().unwrap_or(Value::Null),
            })
        })
        .collect())
}

/// `GET /api/cron` — dev-muse stores jobs in `cron/jobs.jsonc` as
/// `{"jobs": [...]}` (JSONC — comments allowed); the WebUI table
/// wants a plain array with display fields. A missing file means
/// no jobs yet (200 `[]`); a present-but-unparsable file is a
/// server-side state problem (500, not a silent `[]` — fix pass
/// B6 r1 P3-4: the old code hid a corrupt store behind an empty
/// list while the POSTs 400'd on the same file).
pub async fn cron(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    match read_cron_jobs(&state.purrcode_dir).await {
        Ok(jobs) => (StatusCode::OK, Json(Value::Array(jobs))),
        Err(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        ),
    }
}

/// `GET /api/skills` and `/api/skills/candidates`.
///
/// Staged curator drafts live in `skills/.drafts/` as
/// `<name>.<session>.md` files (F20) — NOT in a `candidates/`
/// directory (that was the dev-claude layout). Name and description
/// come from the draft's SKILL.md frontmatter; the filename is only
/// a fallback.
pub async fn skills(State(state): State<Arc<WebState>>) -> Json<Value> {
    let drafts_dir = state.purrcode_dir.join("skills").join(".drafts");
    let mut candidates = Vec::new();
    if drafts_dir.exists() {
        if let Ok(mut rd) = tokio::fs::read_dir(&drafts_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
                    let (name, desc) = draft_frontmatter(&content);
                    let name = name.unwrap_or_else(|| {
                        path.file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default()
                            .split('.')
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    });
                    candidates.push(json!({
                        "name": name,
                        "description": desc.unwrap_or_else(|| "Draft skill".to_string()),
                        "path": path.display().to_string()
                    }));
                }
            }
        }
    }

    let mut active_skills = Vec::new();
    let skills_dir = state.purrcode_dir.join("skills");
    if skills_dir.exists() {
        if let Ok(mut rd) = tokio::fs::read_dir(&skills_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string();
                    if name != "candidates" && name != "archive" && name != ".drafts" {
                        let skill_file = path.join("SKILL.md");
                        let desc = if skill_file.exists() {
                            tokio::fs::read_to_string(&skill_file)
                                .await
                                .ok()
                                .and_then(|c| {
                                    c.lines()
                                        .find(|l| l.trim().starts_with("description:"))
                                        .map(|l| {
                                            l.trim()
                                                .trim_start_matches("description:")
                                                .trim()
                                                .to_string()
                                        })
                                })
                                .unwrap_or_else(|| "Active custom skill".to_string())
                        } else {
                            "Active custom skill".to_string()
                        };
                        active_skills.push(json!({
                            "name": name,
                            "description": desc,
                            "path": path.display().to_string()
                        }));
                    }
                }
            }
        }
    }

    Json(json!({
        "candidates": candidates,
        "skills": active_skills
    }))
}

/// Resolve-only (R3): the WebUI never promotes a draft by moving
/// files itself — that would bypass the curator's validation and
/// the Tinker save turn. Approving resolves the pending
/// `skill-draft:<name>` approval the WebSkillDraftRequester
/// registered; the curator hook in the dispatcher performs the real
/// save/discard after. `resolved: false` means no draft approval
/// was waiting (stale tab, or the 120s card already expired).
/// v0.4 B1: the approval lives on one session's channel (the
/// body's optional `session_id`, default `purr_web`).
async fn resolve_skill_draft(
    registry: &Arc<WebSessionRegistry>,
    body: Bytes,
    approve: bool,
) -> Json<Value> {
    let mut resolved = false;
    if let Ok(val) = serde_json::from_slice::<Value>(&body) {
        if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
            let key = format!("{}{}", SKILL_DRAFT_KEY_PREFIX, name);
            if let Some(session) = approval_session(registry, &val) {
                resolved = session.channel.resolve_approval(&key, approve).await;
            }
        }
    }
    Json(json!({ "resolved": resolved }))
}

/// `POST /api/skills/approve`.
/// v0.4 B8, D9: resolving a draft card is an audited web action —
/// `skill_approve` allow when a pending draft approval was
/// resolved, deny when nothing was waiting.
pub async fn skills_approve(State(state): State<Arc<WebState>>, body: Bytes) -> Json<Value> {
    let name = draft_name_from_body(&body);
    let out = resolve_skill_draft(&state.registry, body, true).await;
    let resolved = out
        .get("resolved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    record_admin(
        &state,
        "skill_approve",
        &name,
        if resolved { "allow" } else { "deny" },
        &format!("resolved={resolved}"),
    );
    out
}

/// `POST /api/skills/reject`.
/// v0.4 B8, D9: same audit as approve, event `skill_reject`.
pub async fn skills_reject(State(state): State<Arc<WebState>>, body: Bytes) -> Json<Value> {
    let name = draft_name_from_body(&body);
    let out = resolve_skill_draft(&state.registry, body, false).await;
    let resolved = out
        .get("resolved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    record_admin(
        &state,
        "skill_reject",
        &name,
        if resolved { "allow" } else { "deny" },
        &format!("resolved={resolved}"),
    );
    out
}

/// Draft name for the audit record: the body's string `name`, or
/// "(unknown)" when the body is malformed or nameless.
fn draft_name_from_body(body: &Bytes) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .unwrap_or_else(|| "(unknown)".to_string())
}

/// `GET /api/plugins` (v0.4 B4) — every discovered plugin with
/// its live mounted state, from the shared registry. The R3 shape
/// (one entry per directory, raw `manifest` YAML string, name =
/// directory name) is replaced by structured `PluginInfo` entries
/// (`name` is now the manifest name — the mount key). Without a
/// wired registry the server answers 503, like mount/unmount do:
/// a 200 with an empty list would be indistinguishable from "no
/// plugins installed" and would hide a miswired server.
pub async fn plugins(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.plugin_registry else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "plugin registry not configured" })),
        );
    };
    (
        StatusCode::OK,
        Json(json!({ "plugins": reg.list_plugins().await })),
    )
}

fn plugin_mount_status(err: &PluginMountError) -> StatusCode {
    match err {
        PluginMountError::NotFound(_) => StatusCode::NOT_FOUND,
        PluginMountError::AlreadyMounted(_) | PluginMountError::NotMounted(_) => {
            StatusCode::CONFLICT
        }
        PluginMountError::InvalidManifest(_)
        | PluginMountError::NoCommand(_)
        | PluginMountError::SpawnFailed(_) => StatusCode::BAD_REQUEST,
    }
}

/// Read the `{"name": ...}` body of a mount/unmount request, in
/// the same hand-rolled style as `/api/prompt`. The error carries
/// an optional audit name: a malformed body or a non-string
/// `name` audits as deny with "(unknown)" (fix pass B8 r1 P2-1 —
/// the same rule the B8 prompt/approval handlers apply). A named
/// `type` alias (not a nested tuple spelled out in the
/// signature) keeps clippy's `type_complexity` lint quiet — the
/// same trap as the B6 r3 P1 fix in `routes_cron.rs`.
type PluginNameError = (Option<String>, (StatusCode, Json<Value>));

fn plugin_name_from_body(body: &Bytes) -> std::result::Result<String, PluginNameError> {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        return Err((
            Some("(unknown)".to_string()),
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "malformed JSON body" })),
            ),
        ));
    };
    let Some(name) = val.get("name").and_then(|v| v.as_str()) else {
        return Err((
            Some("(unknown)".to_string()),
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "name must be a string" })),
            ),
        ));
    };
    Ok(name.to_string())
}

/// One admin action in the audit log (session/agent "web").
/// Shared by the plugin (B4), memory and skill (B5) endpoints:
/// the event names the action (`plugin_mount`, `memory_save`,
/// `skill_create`, ...), `name` is the item, `decision` is
/// "allow" on success and "deny" on a refused attempt.
pub(crate) fn record_admin(
    state: &WebState,
    event: &str,
    name: &str,
    decision: &str,
    detail: &str,
) {
    if let Some(sink) = &state.audit_sink {
        sink.record_event("web", "web", event, Some(name), decision, detail);
    }
}

/// `POST /api/plugins/mount {"name": "..."}` (v0.4 B4) — verify
/// the command spawns and register the plugin in the live manager.
/// 404 unknown name, 409 already mounted, 400 manifest / command
/// / spawn failure (message in `error`).
pub async fn plugins_mount(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.plugin_registry else {
        // Fix pass B8 r1 P3-2: a refused POST leaves a trace.
        record_admin(
            &state,
            "plugin_mount",
            "(unknown)",
            "deny",
            "plugin registry not configured",
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "plugin registry not configured" })),
        );
    };
    let name = match plugin_name_from_body(&body) {
        Ok(n) => n,
        Err((audit, e)) => {
            if let Some(name) = audit {
                let detail =
                    e.1 .0
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rejected request body");
                record_admin(&state, "plugin_mount", &name, "deny", detail);
            }
            return e;
        }
    };
    match reg.mount_plugin(&name).await {
        Ok(info) => {
            record_admin(
                &state,
                "plugin_mount",
                &name,
                "allow",
                &format!("path={} transport={:?}", info.path, info.transport),
            );
            (
                StatusCode::OK,
                Json(json!({ "mounted": true, "plugin": info })),
            )
        }
        Err(e) => {
            // Failed attempts are audited too (decision `deny`):
            // a 404/409/400 that leaves no trace is invisible in
            // the log a user reads after a mount that "did nothing".
            record_admin(&state, "plugin_mount", &name, "deny", &e.to_string());
            (
                plugin_mount_status(&e),
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

/// `POST /api/plugins/unmount {"name": "..."}` (v0.4 B4) — remove
/// the plugin from the live manager. 404 unknown name, 409 not
/// mounted. The plugin stays listed (it is still on disk).
pub async fn plugins_unmount(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.plugin_registry else {
        // Fix pass B8 r1 P3-2: a refused POST leaves a trace.
        record_admin(
            &state,
            "plugin_unmount",
            "(unknown)",
            "deny",
            "plugin registry not configured",
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "plugin registry not configured" })),
        );
    };
    let name = match plugin_name_from_body(&body) {
        Ok(n) => n,
        Err((audit, e)) => {
            if let Some(name) = audit {
                let detail =
                    e.1 .0
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("rejected request body");
                record_admin(&state, "plugin_unmount", &name, "deny", detail);
            }
            return e;
        }
    };
    match reg.unmount_plugin(&name).await {
        Ok(info) => {
            record_admin(
                &state,
                "plugin_unmount",
                &name,
                "allow",
                &format!("path={} transport={:?}", info.path, info.transport),
            );
            (
                StatusCode::OK,
                Json(json!({ "mounted": false, "plugin": info })),
            )
        }
        Err(e) => {
            record_admin(&state, "plugin_unmount", &name, "deny", &e.to_string());
            (
                plugin_mount_status(&e),
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

/// `POST /api/prompt`.
///
/// The prompt goes to one session (v0.4 B1). No `session_id` means
/// the legacy default `purr_web` (R3 clients send none). An
/// explicit id is materialized on demand when the session exists on
/// disk — a resumed chat after restart has a file but no live entry
/// yet (re-checker P2-1); a valid id with no file is a 404, and a
/// present-but-non-string id is a 400 (P3-3) — neither silently
/// falls back to the default.
pub async fn prompt(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // v0.4 B8, D9: a dispatched prompt is already audited at the
    // turn level (`web_turn_start` in web_cli); the handler-level
    // rejections below were unaudited, so they are recorded here
    // as `web_prompt` deny. A correct dispatch answers 200 at
    // once and runs the turn in a spawned task.
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        record_admin(
            &state,
            "web_prompt",
            "(unknown)",
            "deny",
            "malformed JSON body",
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed JSON body" })),
        ));
    };
    let Some(prompt) = val.get("prompt").and_then(|p| p.as_str()) else {
        record_admin(
            &state,
            "web_prompt",
            "(unknown)",
            "deny",
            "prompt must be a string",
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "prompt must be a string" })),
        ));
    };
    let session = match val.get("session_id") {
        Some(v) => {
            let Some(id) = v.as_str() else {
                record_admin(
                    &state,
                    "web_prompt",
                    "(unknown)",
                    "deny",
                    "session_id must be a string",
                );
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "session_id must be a string" })),
                ));
            };
            state.registry.ensure_session_if_exists(id).ok()
        }
        None => state.registry.default_session().ok(),
    };
    let Some(session) = session else {
        record_admin(
            &state,
            "web_prompt",
            "(unknown)",
            "deny",
            "session not found",
        );
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        ));
    };
    if let Some(ref disp) = state.dispatcher {
        let disp = disp.clone();
        let p = prompt.to_string();
        tokio::spawn(async move {
            if let Err(e) = disp.dispatch(session, p).await {
                tracing::error!("Web prompt execution error: {}", e);
            }
        });
        Ok(Json(json!({ "status": "dispatched" })))
    } else {
        record_admin(
            &state,
            "web_prompt",
            &session.id,
            "deny",
            "no prompt dispatcher registered",
        );
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "No prompt dispatcher registered on web server" })),
        ))
    }
}

/// `GET /api/memory` — GET-only (file chosen by substring
/// `SOUL.md` / `USER.md`, else `MEMORY.md`); a missing file
/// answers with a template instead of a 404.
pub async fn memory(State(state): State<Arc<WebState>>, uri: Uri) -> impl IntoResponse {
    let raw_uri = uri.to_string();
    let file_name = if raw_uri.contains("SOUL.md") {
        "SOUL.md"
    } else if raw_uri.contains("USER.md") {
        "USER.md"
    } else {
        "MEMORY.md"
    };
    let mem_file = state.purrcode_dir.join(file_name);
    let body = if mem_file.exists() {
        tokio::fs::read_to_string(&mem_file)
            .await
            .unwrap_or_default()
    } else {
        format!("# {}\n\nNo memory record created yet.", file_name)
    };
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
}

/// `POST /api/approval`.
/// v0.4 B8, D9: resolving (or failing to resolve) a card is an
/// audited web action — `web_approval` allow when a pending card
/// was resolved, deny when nothing was pending (or the body was
/// malformed). The `name` is the call id; the detail records
/// whether the user approved or rejected.
pub async fn approval(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        record_admin(
            &state,
            "web_approval",
            "(unknown)",
            "deny",
            "malformed JSON body",
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed JSON body" })),
        ));
    };
    let call_id = val
        .get("call_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let approved = val
        .get("approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let resolved = match approval_session(&state.registry, &val) {
        Some(session) => {
            // v0.4 B8, D8: the default path reads the live entry
            // without ensuring, so touch explicitly — resolving a
            // card is activity.
            session.touch();
            session.channel.resolve_approval(call_id, approved).await
        }
        None => false,
    };
    record_admin(
        &state,
        "web_approval",
        call_id,
        if resolved { "allow" } else { "deny" },
        &format!("approved={approved} resolved={resolved}"),
    );
    Ok(Json(json!({ "resolved": resolved })))
}

/// Read `name:` / `description:` from a SKILL.md frontmatter block
/// (between the leading `---` lines). Each is `None` when the key is
/// absent; the caller falls back to the staged filename / a default.
pub fn draft_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut desc = None;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if name.is_none() {
            if let Some(v) = line.strip_prefix("name:") {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    name = Some(v.to_string());
                }
            }
        }
        if desc.is_none() {
            if let Some(v) = line.strip_prefix("description:") {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    desc = Some(v.to_string());
                }
            }
        }
    }
    (name, desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draft_frontmatter_parsing() {
        let content = "---\nname: git-helper\ndescription: Helps with git\n---\n\n# Skill\n";
        assert_eq!(
            draft_frontmatter(content),
            (
                Some("git-helper".to_string()),
                Some("Helps with git".to_string())
            )
        );
        assert_eq!(draft_frontmatter("no frontmatter"), (None, None));
        assert_eq!(
            draft_frontmatter("---\nname: only-name\n---\n"),
            (Some("only-name".to_string()), None)
        );
    }
}
