// SPDX-License-Identifier: GPL-3.0-or-later

//! Skill management endpoints (v0.4 B5).
//!
//! The curator endpoints (`GET /api/skills`, `/api/skills/approve`,
//! `/api/skills/reject`) stay in `routes.rs`. B5 adds the full
//! skill list and the write side, all routed through the
//! `SkillRegistry` trait — which the CLI implements on top of
//! `manage_skill` — so these handlers never touch a skill file
//! themselves:
//!
//! * `GET /api/skills/all` — every skill the manager knows
//!   (8 embedded + global + project, precedence-resolved).
//! * `GET /api/skills/{name}` — one skill with its full
//!   `SKILL.md` body, for the edit form.
//! * `POST /api/skills/create|update|delete` — add, replace, or
//!   remove a skill in the project or global scope.
//!
//! Without a wired registry every endpoint answers 503, like the
//! B4 plugin endpoints (a 200 with an empty list would hide a
//! miswired server). Failed attempts audit with decision "deny",
//! successes with "allow" — the same rule as plugin mount.

use super::routes::record_admin;
use super::WebState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use purrcode_api::skill::{SkillError, SkillInfo, SkillScope};
use serde_json::{json, Value};
use std::sync::Arc;

fn skill_error_status(e: &SkillError) -> StatusCode {
    match e {
        SkillError::NotFound(_) => StatusCode::NOT_FOUND,
        SkillError::AlreadyExists(_) => StatusCode::CONFLICT,
        SkillError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        SkillError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn no_registry() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "skill registry not configured" })),
    )
}

/// `GET /api/skills/all` — the full skill list.
pub async fn skills_all(State(state): State<Arc<WebState>>) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.skill_registry else {
        return no_registry();
    };
    let skills = reg.list_skills().await;
    (StatusCode::OK, Json(json!({ "skills": skills })))
}

/// `GET /api/skills/{name}` — one skill with its full body.
/// 404 when no skill of that name exists (any scope).
pub async fn skill_detail(
    State(state): State<Arc<WebState>>,
    Path(name): Path<String>,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.skill_registry else {
        return no_registry();
    };
    match reg.get_skill(&name).await {
        Ok(detail) => (StatusCode::OK, Json(json!({ "skill": detail }))),
        Err(e) => (
            skill_error_status(&e),
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

struct SkillWrite {
    name: String,
    description: String,
    content: String,
    scope: SkillScope,
}

/// Hand-parsed body, same style as the other POST endpoints:
/// no `Content-Type` requirement, malformed JSON is a 400.
/// `scope` defaults to "project"; anything else is a 400.
fn skill_write_from_body(
    body: &Bytes,
    need_content: bool,
) -> Result<SkillWrite, (StatusCode, Json<Value>)> {
    let err = |msg: &str| (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })));
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        return Err(err("malformed JSON body"));
    };
    let Some(name) = val.get("name").and_then(|n| n.as_str()) else {
        return Err(err("name must be a string"));
    };
    let scope = match val.get("scope").and_then(|s| s.as_str()) {
        None => SkillScope::Project,
        Some("project") => SkillScope::Project,
        Some("global") => SkillScope::Global,
        Some(other) => {
            return Err(err(&format!("unknown scope '{other}'")));
        }
    };
    let (description, content) = if need_content {
        let Some(description) = val.get("description").and_then(|d| d.as_str()) else {
            return Err(err("description must be a string"));
        };
        let Some(content) = val.get("content").and_then(|c| c.as_str()) else {
            return Err(err("content must be a string"));
        };
        (description.to_string(), content.to_string())
    } else {
        (String::new(), String::new())
    };
    Ok(SkillWrite {
        name: name.to_string(),
        description,
        content,
        scope,
    })
}

/// Audit a rejected skill write when the request names the
/// skill (fix pass B5 r1 P3-3): a refusal the client can
/// attribute to a name belongs in the audit log; only a
/// nameless or malformed body has nothing to attribute.
fn audit_rejected_parse(
    state: &Arc<WebState>,
    event: &str,
    body: &Bytes,
    err: &(StatusCode, Json<Value>),
) {
    let detail = err
        .1
         .0
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("rejected request body");
    if let Ok(val) = serde_json::from_slice::<Value>(body) {
        if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
            record_admin(state, event, name, "deny", detail);
        }
    }
}
/// `POST /api/skills/create {"name", "description", "content",
/// "scope"?}` — 409 when a skill of that name already exists in
/// the scope; 400 when `manage_skill` rejects the name,
/// description, or content. The response echoes the skill as
/// stored on disk (re-read after the write), not the form
/// values — a body whose frontmatter declares another
/// description must not make the API claim the form's (fix pass
/// B5 r1 P2-5).
pub async fn skills_create(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.skill_registry else {
        return no_registry();
    };
    let fields = match skill_write_from_body(&body, true) {
        Ok(f) => f,
        Err(e) => {
            audit_rejected_parse(&state, "skill_create", &body, &e);
            return e;
        }
    };
    match reg
        .create_skill(
            &fields.name,
            &fields.description,
            &fields.content,
            fields.scope,
        )
        .await
    {
        Ok(info) => {
            record_admin(
                &state,
                "skill_create",
                &fields.name,
                "allow",
                &format!(
                    "scope={:?} path={}",
                    fields.scope,
                    info.path.as_deref().unwrap_or("embedded")
                ),
            );
            // Report the skill as stored, not as requested:
            // the stored description is authoritative when the
            // body's frontmatter carries its own.
            let stored = reg.get_skill(&fields.name).await.ok().map(|d| SkillInfo {
                name: d.name,
                description: d.description,
                source: d.source,
                path: d.path,
            });
            (
                StatusCode::OK,
                Json(json!({ "created": true, "skill": stored.unwrap_or(info) })),
            )
        }
        Err(e) => {
            record_admin(&state, "skill_create", &fields.name, "deny", &e.to_string());
            (
                skill_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

/// `POST /api/skills/update {"name", "description", "content",
/// "scope"?}` — replaces the copy in the scope (via
/// `manage_skill` create, which overwrites). 404 when the copy
/// does not exist there — embedded skills cannot be updated;
/// create a project/global copy of the same name to shadow one.
pub async fn skills_update(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.skill_registry else {
        return no_registry();
    };
    let fields = match skill_write_from_body(&body, true) {
        Ok(f) => f,
        Err(e) => {
            audit_rejected_parse(&state, "skill_update", &body, &e);
            return e;
        }
    };
    match reg
        .update_skill(
            &fields.name,
            &fields.description,
            &fields.content,
            fields.scope,
        )
        .await
    {
        Ok(info) => {
            record_admin(
                &state,
                "skill_update",
                &fields.name,
                "allow",
                &format!(
                    "scope={:?} path={}",
                    fields.scope,
                    info.path.as_deref().unwrap_or("embedded")
                ),
            );
            // Report the skill as stored, not as requested (see
            // skills_create).
            let stored = reg.get_skill(&fields.name).await.ok().map(|d| SkillInfo {
                name: d.name,
                description: d.description,
                source: d.source,
                path: d.path,
            });
            (
                StatusCode::OK,
                Json(json!({ "updated": true, "skill": stored.unwrap_or(info) })),
            )
        }
        Err(e) => {
            record_admin(&state, "skill_update", &fields.name, "deny", &e.to_string());
            (
                skill_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}

/// `POST /api/skills/delete {"name", "scope"?}` — 404 when the
/// copy does not exist in the scope; embedded skills cannot be
/// deleted (there is no file to remove).
pub async fn skills_delete(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Some(reg) = &state.skill_registry else {
        return no_registry();
    };
    let fields = match skill_write_from_body(&body, false) {
        Ok(f) => f,
        Err(e) => {
            audit_rejected_parse(&state, "skill_delete", &body, &e);
            return e;
        }
    };
    match reg.delete_skill(&fields.name, fields.scope).await {
        Ok(()) => {
            record_admin(
                &state,
                "skill_delete",
                &fields.name,
                "allow",
                &format!("scope={:?}", fields.scope),
            );
            (
                StatusCode::OK,
                Json(json!({ "deleted": true, "name": fields.name })),
            )
        }
        Err(e) => {
            record_admin(&state, "skill_delete", &fields.name, "deny", &e.to_string());
            (
                skill_error_status(&e),
                Json(json!({ "error": e.to_string() })),
            )
        }
    }
}
