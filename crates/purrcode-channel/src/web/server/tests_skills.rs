// SPDX-License-Identifier: GPL-3.0-or-later

//! Memory + skill endpoint tests (v0.4 B5).
//!
//! These test the HTTP layer against a small fake `SkillRegistry`
//! that follows the trait contract (unknown → 404, same-scope
//! double create → 409, bad name → 400). The real
//! `WebSkillRegistry` is covered by its own unit tests in
//! `purrcode-cli` (create → list → get → update → delete through
//! the real `manage_skill` tool), and the E2E harness exercises
//! the real registry over HTTP against the release binary.
//! Memory saves write the test `purrcode_dir` directly — no
//! registry is involved, so they are tested here for real.

use super::test_support::*;
use purrcode_api::skill::{
    SkillDetail, SkillError, SkillInfo, SkillRegistry, SkillScope, SkillSource,
};
use purrcode_api::tool::AuditSink;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn info(name: &str, source: SkillSource, content: &str) -> (SkillInfo, String) {
    (
        SkillInfo {
            name: name.to_string(),
            description: format!("{name} skill"),
            source,
            path: match source {
                SkillSource::Embedded => None,
                _ => Some(format!("/skills/{name}/SKILL.md")),
            },
        },
        content.to_string(),
    )
}

/// Contract-faithful fake registry: the HTTP tests assert status
/// mapping, JSON shape, and audit events, not the manager's
/// name validation or file writes.
struct FakeRegistry {
    skills: Mutex<HashMap<String, (SkillInfo, String)>>,
}

impl FakeRegistry {
    fn new(entries: Vec<(SkillInfo, String)>) -> Self {
        Self {
            skills: Mutex::new(
                entries
                    .into_iter()
                    .map(|(info, content)| (info.name.clone(), (info, content)))
                    .collect(),
            ),
        }
    }
}

#[async_trait::async_trait]
impl SkillRegistry for FakeRegistry {
    async fn list_skills(&self) -> Vec<SkillInfo> {
        let mut out: Vec<SkillInfo> = self
            .skills
            .lock()
            .unwrap()
            .values()
            .map(|(info, _)| info.clone())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    async fn get_skill(&self, name: &str) -> Result<SkillDetail, SkillError> {
        let map = self.skills.lock().unwrap();
        let (info, content) = map
            .get(name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
        Ok(SkillDetail {
            name: info.name.clone(),
            description: info.description.clone(),
            source: info.source,
            path: info.path.clone(),
            content: content.clone(),
        })
    }

    async fn create_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError> {
        if name.is_empty() || name.contains(' ') {
            return Err(SkillError::InvalidInput(format!(
                "invalid skill name '{name}'"
            )));
        }
        let mut map = self.skills.lock().unwrap();
        if map.contains_key(name) {
            return Err(SkillError::AlreadyExists(name.to_string()));
        }
        let info = SkillInfo {
            name: name.to_string(),
            description: description.to_string(),
            source: match scope {
                SkillScope::Project => SkillSource::Project,
                SkillScope::Global => SkillSource::Global,
            },
            path: Some(format!("/skills/{name}/SKILL.md")),
        };
        map.insert(name.to_string(), (info.clone(), content.to_string()));
        Ok(info)
    }

    async fn update_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError> {
        let mut map = self.skills.lock().unwrap();
        let (info, body) = map
            .get_mut(name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
        if info.source == SkillSource::Embedded {
            return Err(SkillError::NotFound(name.to_string()));
        }
        info.description = description.to_string();
        info.source = match scope {
            SkillScope::Project => SkillSource::Project,
            SkillScope::Global => SkillSource::Global,
        };
        *body = content.to_string();
        Ok(info.clone())
    }

    async fn delete_skill(&self, name: &str, _scope: SkillScope) -> Result<(), SkillError> {
        let mut map = self.skills.lock().unwrap();
        let (info, _) = map
            .get(name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
        if info.source == SkillSource::Embedded {
            return Err(SkillError::NotFound(name.to_string()));
        }
        map.remove(name);
        Ok(())
    }
}

type AuditEvents = Arc<Mutex<Vec<(String, String, String, String)>>>;

struct RecordingAuditSink {
    events: AuditEvents,
}

impl RecordingAuditSink {
    fn new() -> (Self, AuditEvents) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                events: events.clone(),
            },
            events,
        )
    }
}

impl AuditSink for RecordingAuditSink {
    fn record_event(
        &self,
        _session_id: &str,
        _agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    ) {
        self.events.lock().unwrap().push((
            event.to_string(),
            tool.unwrap_or_default().to_string(),
            decision.to_string(),
            detail.to_string(),
        ));
    }
}

fn seed_registry() -> Arc<FakeRegistry> {
    Arc::new(FakeRegistry::new(vec![
        info("plan", SkillSource::Embedded, "# Plan\n"),
        info("git", SkillSource::Global, "# Git\n"),
    ]))
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skills_all_lists_every_source() {
    let dir = test_dir("skills-all");
    let (port, _ch, cancel_tx, handle) = start_server_with_skill_registry(
        None,
        dir.clone(),
        None,
        None,
        Some(seed_registry()),
        None,
    )
    .await;
    let resp = get(port, "/api/skills/all");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    let skills = val["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0]["name"], "git");
    assert_eq!(skills[0]["source"], "global");
    assert_eq!(skills[1]["name"], "plan");
    assert_eq!(skills[1]["source"], "embedded");
    assert!(skills[1]["path"].is_null());
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skill_detail_and_404() {
    let dir = test_dir("skills-detail");
    let (port, _ch, cancel_tx, handle) = start_server_with_skill_registry(
        None,
        dir.clone(),
        None,
        None,
        Some(seed_registry()),
        None,
    )
    .await;
    let resp = get(port, "/api/skills/git");
    assert!(resp.contains("200 OK"), "resp: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["skill"]["name"], "git");
    assert_eq!(val["skill"]["content"], "# Git\n");
    let resp = get(port, "/api/skills/ghost");
    assert!(resp.contains("404 Not Found"), "resp: {resp}");
    assert!(resp.contains("no skill named"), "resp: {resp}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skills_crud_statuses_and_audit() {
    let dir = test_dir("skills-crud");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _ch, cancel_tx, handle) = start_server_with_skill_registry(
        None,
        dir.clone(),
        None,
        None,
        Some(seed_registry()),
        Some(Arc::new(sink)),
    )
    .await;
    // Create: 200, then the same name is a 409.
    let resp = post(
        port,
        "/api/skills/create",
        &serde_json::json!({"name": "notes", "description": "Notes", "content": "# N\n", "scope": "project"}).to_string(),
    );
    assert!(resp.contains("200 OK"), "resp: {resp}");
    assert!(resp.contains("\"created\":true"), "resp: {resp}");
    let resp = post(
        port,
        "/api/skills/create",
        &serde_json::json!({"name": "notes", "description": "Again", "content": "# N\n"})
            .to_string(),
    );
    assert!(resp.contains("409 Conflict"), "resp: {resp}");
    // Bad name is a 400 from the registry contract.
    let resp = post(
        port,
        "/api/skills/create",
        r#"{"name": "bad name", "description": "d", "content": "c"}"#,
    );
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    // Bad scope is a 400 from the body parser.
    let resp = post(
        port,
        "/api/skills/create",
        r#"{"name": "x", "description": "d", "content": "c", "scope": "sideways"}"#,
    );
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    // Update: 200; embedded is a 404; ghost is a 404.
    let resp = post(
        port,
        "/api/skills/update",
        &serde_json::json!({"name": "notes", "description": "Notes v2", "content": "# N2\n"})
            .to_string(),
    );
    assert!(resp.contains("200 OK"), "resp: {resp}");
    let resp = get(port, "/api/skills/notes");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["skill"]["content"], "# N2\n");
    let resp = post(
        port,
        "/api/skills/update",
        r#"{"name": "plan", "description": "d", "content": "c"}"#,
    );
    assert!(resp.contains("404 Not Found"), "resp: {resp}");
    // Delete: 200; second delete is a 404.
    let resp = post(port, "/api/skills/delete", r#"{"name": "notes"}"#);
    assert!(resp.contains("200 OK"), "resp: {resp}");
    let resp = post(port, "/api/skills/delete", r#"{"name": "notes"}"#);
    assert!(resp.contains("404 Not Found"), "resp: {resp}");
    // Audit: allows and denies both recorded, in order. The
    // bad-scope 400 is a parse rejection WITH a name, so it
    // audits a deny too (fix pass B5 r1 P3-3).
    let got = events.lock().unwrap().clone();
    let kinds: Vec<(String, String)> = got
        .iter()
        .map(|(e, _n, d, _)| (e.clone(), d.clone()))
        .collect();
    assert_eq!(kinds[0], ("skill_create".to_string(), "allow".to_string()));
    assert_eq!(kinds[1], ("skill_create".to_string(), "deny".to_string()));
    assert_eq!(kinds[2], ("skill_create".to_string(), "deny".to_string()));
    assert_eq!(kinds[3], ("skill_create".to_string(), "deny".to_string()));
    assert_eq!(kinds[4], ("skill_update".to_string(), "allow".to_string()));
    assert_eq!(kinds[5], ("skill_update".to_string(), "deny".to_string()));
    assert_eq!(kinds[6], ("skill_delete".to_string(), "allow".to_string()));
    assert_eq!(kinds[7], ("skill_delete".to_string(), "deny".to_string()));
    assert_eq!(kinds.len(), 8);
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skills_endpoints_503_without_registry() {
    let dir = test_dir("skills-503");
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    for (method, path) in [
        ("GET", "/api/skills/all"),
        ("GET", "/api/skills/plan"),
        ("POST", "/api/skills/create"),
        ("POST", "/api/skills/update"),
        ("POST", "/api/skills/delete"),
    ] {
        let resp = if method == "GET" {
            get(port, path)
        } else {
            post(port, path, r#"{"name": "x"}"#)
        };
        assert!(
            resp.contains("503 Service Unavailable"),
            "{method} {path}: {resp}"
        );
        assert!(
            resp.contains("skill registry not configured"),
            "{method} {path}: {resp}"
        );
    }
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_info_shape() {
    let dir = test_dir("memory-info");
    std::fs::write(dir.join("MEMORY.md"), "# Mem\n").unwrap();
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let resp = get(port, "/api/memory/info");
    assert!(resp.contains("200 OK"), "resp: {resp}");
    let val: serde_json::Value = serde_json::from_str(body_of(&resp)).unwrap();
    assert_eq!(val["backend"], "markdown");
    let files = val["files"].as_array().unwrap();
    assert_eq!(files.len(), 4);
    let mem = files.iter().find(|f| f["name"] == "MEMORY.md").unwrap();
    assert_eq!(mem["exists"], true);
    assert_eq!(mem["bytes"], 6);
    let soul = files.iter().find(|f| f["name"] == "SOUL.md").unwrap();
    assert_eq!(soul["exists"], false);
    assert_eq!(soul["bytes"], 0);
    let proj = files
        .iter()
        .find(|f| f["name"] == ".purrcode/MEMORY.md")
        .unwrap();
    assert_eq!(proj["exists"], false);
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_save_roundtrip_and_validation() {
    let dir = test_dir("memory-save");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _ch, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
    // Save, then GET shows the new content; no tmp file left over.
    let resp = post(
        port,
        "/api/memory",
        &serde_json::json!({"file": "MEMORY.md", "content": "# Saved memory\n"}).to_string(),
    );
    assert!(resp.contains("200 OK"), "resp: {resp}");
    assert!(resp.contains("\"saved\":true"), "resp: {resp}");
    assert!(!dir.join("MEMORY.md.tmp").exists());
    let resp = get(port, "/api/memory?file=MEMORY.md");
    assert!(resp.contains("# Saved memory"), "resp: {resp}");
    // Unknown file, missing content, NUL bytes, oversize: all 400.
    let resp = post(
        port,
        "/api/memory",
        r#"{"file": "SECRETS.md", "content": "x"}"#,
    );
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    let resp = post(port, "/api/memory", r#"{"file": "USER.md"}"#);
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    let resp = post(
        port,
        "/api/memory",
        "{\"file\": \"USER.md\", \"content\": \"a\\u0000b\"}",
    );
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    let big = "x".repeat(100_001);
    let body = serde_json::json!({"file": "USER.md", "content": big}).to_string();
    let resp = post(port, "/api/memory", &body);
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    // Audit: one allow, then denies for the refused saves —
    // including the unknown file, whose name came in the request
    // body and is therefore attributable (fix pass B5 r1 P3-3).
    let got = events.lock().unwrap().clone();
    let kinds: Vec<(String, String, String)> = got
        .iter()
        .map(|(e, n, d, _)| (e.clone(), n.clone(), d.clone()))
        .collect();
    assert_eq!(kinds[0].0, "memory_save");
    assert_eq!(kinds[0].2, "allow");
    assert!(kinds
        .iter()
        .skip(1)
        .all(|k| k.0 == "memory_save" && k.2 == "deny"));
    assert_eq!(kinds.len(), 5);
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_save_parallel_all_succeed_and_no_tmp_left() {
    // Fix pass B5 r1 P3-2: concurrent saves used to share one
    // deterministic tmp, so all but one failed to rename — and
    // the file that won belonged to a save that reported 500.
    // With a unique tmp per save, every save succeeds and the
    // last rename wins.
    let dir = test_dir("memory-save-parallel");
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let mut tasks = Vec::new();
    for i in 0..5 {
        let body = serde_json::json!({"file": "MEMORY.md", "content": format!("payload-{i}\n")})
            .to_string();
        tasks.push(tokio::task::spawn_blocking(move || {
            post(port, "/api/memory", &body)
        }));
    }
    for t in tasks {
        let resp = t.await.unwrap();
        assert!(resp.contains("200 OK"), "resp: {resp}");
        assert!(resp.contains("\"saved\":true"), "resp: {resp}");
    }
    let saved = std::fs::read_to_string(dir.join("MEMORY.md")).unwrap();
    assert!(
        (0..5).any(|i| saved == format!("payload-{i}\n")),
        "saved: {saved:?}"
    );
    // No tmp of any save left behind (unique names now).
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_save_nonstring_file_is_400() {
    // Fix pass B5 r1 P3-4: a non-string `file` used to fall
    // through to the default and save MEMORY.md.
    let dir = test_dir("memory-save-nonstring");
    let (port, _ch, cancel_tx, handle) = start_server(None, dir.clone(), None).await;
    let resp = post(port, "/api/memory", r#"{"file": 123, "content": "x"}"#);
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    assert!(!dir.join("MEMORY.md").exists());
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_save_malformed_body_is_audited() {
    // Fix pass B8 r1 P2-1: a malformed JSON body audits as deny
    // with "(unknown)", like the B8 prompt/approval handlers.
    let dir = test_dir("memory-save-malformed");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _ch, cancel_tx, handle) =
        start_server_with_skill_registry(None, dir.clone(), None, None, None, Some(Arc::new(sink)))
            .await;
    let resp = post(port, "/api/memory", "{not json");
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    let got = events.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "events: {got:?}");
    assert_eq!(got[0].0, "memory_save");
    assert_eq!(got[0].1, "(unknown)");
    assert_eq!(got[0].2, "deny");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_skill_create_malformed_body_is_audited() {
    // Fix pass B8 r1 P2-1: same rule for skill writes — a
    // malformed body audits as deny with "(unknown)".
    let dir = test_dir("skill-create-malformed");
    let (sink, events) = RecordingAuditSink::new();
    let (port, _ch, cancel_tx, handle) = start_server_with_skill_registry(
        None,
        dir.clone(),
        None,
        None,
        Some(seed_registry()),
        Some(Arc::new(sink)),
    )
    .await;
    let resp = post(port, "/api/skills/create", "{not json");
    assert!(resp.contains("400 Bad Request"), "resp: {resp}");
    let got = events.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "events: {got:?}");
    assert_eq!(got[0].0, "skill_create");
    assert_eq!(got[0].1, "(unknown)");
    assert_eq!(got[0].2, "deny");
    let _ = cancel_tx.send(true);
    handle.abort();
    let _ = std::fs::remove_dir_all(dir);
}
