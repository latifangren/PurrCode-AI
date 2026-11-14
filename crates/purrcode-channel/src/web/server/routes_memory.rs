// SPDX-License-Identifier: GPL-3.0-or-later

//! Memory endpoints (v0.4 B5).
//!
//! `GET /api/memory` (read-only, in `routes.rs`) stays as-is.
//! B5 adds the write side and the backend facts:
//!
//! * `GET /api/memory/info` — which backend is active and the
//!   files it reads, with existence + byte sizes. The active
//!   backend is always `markdown` today: `MarkdownMemory` is the
//!   only `MemoryBackend` implementation (ARCHITECTURE §4);
//!   richer backends arrive as plugins behind the trait, never
//!   hardcoded into core.
//! * `POST /api/memory` — save one of the three global identity
//!   files (`MEMORY.md` / `USER.md` / `SOUL.md`) from the WebUI.
//!   The save is the user's own explicit action in an
//!   authenticated session (the same authorization as a plugin
//!   mount), validated and audited like every other admin write:
//!   success audits `memory_save` with decision "allow", a
//!   refused save audits the same event with "deny".

use super::routes::record_admin;
use super::WebState;
use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::Arc;

/// Maximum memory content the WebUI accepts in one save — the
/// same 100K cap `manage_skill` applies to skill bodies. A
/// memory file is a curated note, not a dump; anything bigger
/// belongs in the editor, not in a POST.
const MAX_MEMORY_CHARS: usize = 100_000;

/// The three global identity files the WebUI may save. The
/// project-local `.purrcode/MEMORY.md` is read by the agent but
/// is NOT writable from the WebUI: it belongs to the project,
/// and the web session's write scope is the global identity.
fn memory_file_name(val: &Value) -> Result<&str, (StatusCode, Json<Value>)> {
    match val.get("file") {
        // Absent means the default file.
        None => Ok("MEMORY.md"),
        Some(Value::String(name)) => match name.as_str() {
            "MEMORY.md" | "USER.md" | "SOUL.md" => Ok(name.as_str()),
            other => Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown memory file '{other}'") })),
            )),
        },
        // Present but not a string must not silently fall back
        // to the default file (fix pass B5 r1 P3-4).
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "file must be a string" })),
        )),
    }
}

fn file_facts(name: &str, path: &std::path::Path) -> Value {
    let (exists, bytes) = match std::fs::metadata(path) {
        Ok(m) => (true, m.len()),
        Err(_) => (false, 0),
    };
    json!({
        "name": name,
        "path": path.display().to_string(),
        "exists": exists,
        "bytes": bytes,
    })
}

/// `GET /api/memory/info` — backend facts: the active backend
/// kind, the global root, and every file the agent loads memory
/// from (three global files + the project-local one).
pub async fn memory_info(State(state): State<Arc<WebState>>) -> Json<Value> {
    let global = &state.purrcode_dir;
    let project = state.workspace_root.join(".purrcode").join("MEMORY.md");
    Json(json!({
        "backend": "markdown",
        "root": global.display().to_string(),
        "files": [
            file_facts("MEMORY.md", &global.join("MEMORY.md")),
            file_facts("USER.md", &global.join("USER.md")),
            file_facts("SOUL.md", &global.join("SOUL.md")),
            file_facts(".purrcode/MEMORY.md", &project),
        ],
    }))
}

/// `POST /api/memory {"file": "MEMORY.md", "content": "..."}` —
/// validate, then atomically replace the file (tmp + rename in
/// the same directory, like the cron store) so a half-written
/// memory can never reach the next turn. 400 on an unknown file,
/// oversize content, or NUL bytes; every refusal is audited
/// with decision "deny" — including an unknown file name, which
/// IS attributable (it came in the request body).
pub async fn memory_save(
    State(state): State<Arc<WebState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        // Fix pass B8 r1 P2-1: a malformed body audits as deny
        // with "(unknown)", like the B8 prompt/approval handlers.
        record_admin(
            &state,
            "memory_save",
            "(unknown)",
            "deny",
            "malformed JSON body",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "malformed JSON body" })),
        );
    };
    let file = match memory_file_name(&val) {
        Ok(f) => f,
        Err(e) => {
            // An unknown file name is attributable — audit the
            // refusal (fix pass B5 r1 P3-3). A non-string `file`
            // has no name to attribute, so it audits nothing.
            if let Some(name) = val.get("file").and_then(|f| f.as_str()) {
                record_admin(&state, "memory_save", name, "deny", "unknown memory file");
            }
            return e;
        }
    };
    let Some(content) = val.get("content").and_then(|c| c.as_str()) else {
        record_admin(&state, "memory_save", file, "deny", "missing content field");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a string" })),
        );
    };
    if content.chars().count() > MAX_MEMORY_CHARS {
        record_admin(
            &state,
            "memory_save",
            file,
            "deny",
            &format!("content exceeds {MAX_MEMORY_CHARS} characters"),
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("content exceeds {MAX_MEMORY_CHARS} characters") })),
        );
    }
    if content.contains('\0') {
        record_admin(
            &state,
            "memory_save",
            file,
            "deny",
            "content contains NUL bytes",
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content contains NUL bytes" })),
        );
    }
    let target = state.purrcode_dir.join(file);
    // A tmp name unique per save (pid + monotonic counter):
    // concurrent saves used to share one deterministic tmp, so
    // all but one failed to rename and the reported success did
    // not match the file that actually won (fix pass B5 r1
    // P3-2). With unique tmps every save succeeds and the last
    // rename wins, as documented.
    static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = target.with_extension(format!("md.tmp-{}-{tmp_n}", std::process::id()));
    let saved: std::io::Result<()> = async {
        tokio::fs::create_dir_all(&state.purrcode_dir).await?;
        tokio::fs::write(&tmp, content.as_bytes()).await?;
        tokio::fs::rename(&tmp, &target).await?;
        Ok(())
    }
    .await;
    if let Err(e) = saved {
        let _ = tokio::fs::remove_file(&tmp).await;
        record_admin(
            &state,
            "memory_save",
            file,
            "deny",
            &format!("write failed: {e}"),
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to save {file}: {e}") })),
        );
    }
    record_admin(
        &state,
        "memory_save",
        file,
        "allow",
        &format!("bytes={}", content.len()),
    );
    (
        StatusCode::OK,
        Json(json!({ "saved": true, "file": file, "bytes": content.len() })),
    )
}
