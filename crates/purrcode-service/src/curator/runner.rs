// SPDX-License-Identifier: GPL-3.0-or-later

//! The post-turn curator (v0.3 F20, ADR-014 item 7): after a turn
//! that made at least [`CURATOR_THRESHOLD`] tool calls, one tool-less
//! model call drafts a skill from the turn's log. The draft is
//! validated (same rules as `manage_skill`, plus the secret scan) and
//! staged outside the live skill dirs; the user approves it on the
//! TUI card or the Telegram buttons, and only then does Tinker save
//! it with `manage_skill action=create`.
//!
//! The runner is deliberately read-only by construction: the curator
//! step is a single `stream_completion` with `tools: None` — it has
//! no tools to call, so it cannot write, delegate, or trigger another
//! curator step. The save turn is a normal turn (its tool-call count
//! is 1 — one `task` delegation — far below the threshold), so no
//! extra recursion flag is needed. Heartbeat and cron turns never
//! reach this runner: they run through `run_job`/`run_once`, not the
//! TUI or gateway turn paths that call it.

use crate::curator::{
    counter::build_tool_log,
    draft::{parse_draft_response, validate_draft, DraftMeta},
    stage::stage_draft,
};
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{ChatMessage, CompletionRequest, MessageRole, ModelProvider};
use purrcode_approval::{AuditEntry, AuditLogger};
use purrcode_tools::skill::SkillManager;
use std::path::PathBuf;
use std::sync::Arc;

/// Tool calls in one turn before the curator even looks at it (PRD
/// F20: "post-turn trigger ≥ 5 tool calls (counter, not a judgment)").
pub const CURATOR_THRESHOLD: usize = 5;

/// What the post-turn check produced. The caller (TUI / gateway) only
/// has work to do for [`CuratorOutcome::DraftStaged`]: ask the user.
#[derive(Debug)]
pub enum CuratorOutcome {
    /// Fewer than the threshold calls since the last check; nothing
    /// drafted, the marker was still written.
    BelowThreshold { count: usize },
    /// The curator model call failed (provider error, empty stream).
    /// Fail-closed: no draft, the marker was still written.
    DecideFailed { reason: String },
    /// The model answered NO_SKILL: nothing repeatable in this turn.
    NoDraft,
    /// A draft was produced but failed validation; the reason is in
    /// the audit (`skill_draft_rejected`).
    DraftRejected { reason: String },
    /// A draft passed validation and was staged. The caller asks the
    /// user; on approve the save turn runs, on deny the staged file
    /// is discarded.
    DraftStaged { meta: DraftMeta, path: PathBuf },
}

pub struct CuratorDeps {
    pub provider: Arc<dyn ModelProvider>,
    pub model: String,
    pub purrcode_dir: PathBuf,
    pub audit_logger: Arc<AuditLogger>,
}

pub struct CuratorRunner {
    deps: CuratorDeps,
}

const CURATOR_SYSTEM: &str = "You are the skill curator for PurrCode AI. Your job is to spot a repeatable workflow in the tool-call log of one agent turn and capture it as a reusable skill — or to say there is nothing worth capturing.\n\nRules:\n- Only propose a skill when the turn shows a REPEATABLE workflow (a sequence of steps the user would plausibly reuse: a build/test/fix loop, a project-specific procedure, a debugging routine). A single question, a one-off fix, or a chat with no tool pattern is NOT a skill.\n- The skill name is kebab-case, short, specific (e.g. cargo-test-loop, debug-flaky-test). Never reuse a generic name like coding or testing.\n- The description is one line, at most 500 characters: when to use the skill.\n- The body is concrete: WHEN to use it, then the exact steps/commands in order. No filler, no preamble.\n- Never include secrets, tokens, passwords, or API keys. Never include file paths outside the project.\n- If there is nothing repeatable, answer exactly: NO_SKILL\n\nOutput: either exactly \"NO_SKILL\" on one line, or a complete SKILL.md file:\n---\nname: <kebab-case>\ndescription: <one line>\n---\n<body>";

impl CuratorRunner {
    pub fn new(deps: CuratorDeps) -> Self {
        Self { deps }
    }

    fn audit(&self, session_id: &str, event: &str, decision: &str, detail: &str) {
        let _ = self.deps.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            agent: "curator".to_string(),
            event: event.to_string(),
            tool: None,
            decision: decision.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Consume the save turn's tool calls. The save turn runs with
    /// the parent's session id, so its `task` delegation would
    /// otherwise count toward the NEXT turn's threshold — the marker
    /// from `run_after_turn` was written before the save turn ran.
    /// Called by the hook right after the save turn, whether the
    /// save succeeded or failed (the calls are spent either way).
    pub fn consume_save_turn(&self, session_id: &str) {
        self.audit(
            session_id,
            "skill_curator_checked",
            "ok",
            "save turn calls consumed (F3)",
        );
    }

    /// Run the check after one turn. `messages` is the turn's session
    /// history and `from_idx` the index it had before the turn, so
    /// the log is built from this turn's messages only. `skill_mgr`
    /// is read-only here (duplicate-name check).
    pub async fn run_after_turn(
        &self,
        session_id: &str,
        messages: &[ChatMessage],
        from_idx: usize,
        skill_mgr: &SkillManager,
    ) -> CuratorOutcome {
        let count = self
            .deps
            .audit_logger
            .count_tool_calls_since_curator_check(session_id);
        // Marker first: whatever happens next, this turn's count is
        // consumed — the next turn starts from zero.
        self.audit(
            session_id,
            "skill_curator_checked",
            "ok",
            &format!("tool_calls={count} (threshold {CURATOR_THRESHOLD})"),
        );
        if count < CURATOR_THRESHOLD {
            return CuratorOutcome::BelowThreshold { count };
        }
        let log = build_tool_log(messages, from_idx);
        // Scrub before the LLM boundary: a tool-call log can carry a
        // secret the user pasted into a command or a message (AGENTS.md
        // §7 — never send credentials to an LLM). The audit's scanner
        // is the same one the audit log itself uses.
        let log = purrcode_approval::scrub_secrets(&log);
        let text = match self.decide(&log).await {
            Ok(t) => t,
            Err(e) => {
                self.audit(
                    session_id,
                    "skill_curator_failed",
                    "ok",
                    &format!("curator model call failed: {e}"),
                );
                return CuratorOutcome::DecideFailed {
                    reason: e.to_string(),
                };
            }
        };
        let Some(content) = parse_draft_response(&text) else {
            return CuratorOutcome::NoDraft;
        };
        match validate_draft(&content, skill_mgr) {
            Ok(meta) => match stage_draft(
                &self.deps.purrcode_dir,
                session_id,
                &meta.name,
                &meta.content,
            ) {
                Ok(path) => {
                    self.audit(
                        session_id,
                        "skill_draft_created",
                        "ok",
                        &format!("draft '{}' staged at {}", meta.name, path.display()),
                    );
                    CuratorOutcome::DraftStaged { meta, path }
                }
                Err(e) => CuratorOutcome::DraftRejected {
                    reason: e.to_string(),
                },
            },
            Err(reason) => {
                self.audit(session_id, "skill_draft_rejected", "ok", &reason);
                CuratorOutcome::DraftRejected { reason }
            }
        }
    }

    /// One tool-less completion call. `tools: None` is the read-only
    /// seal: the curator has nothing to call, so it cannot write,
    /// delegate, or recurse.
    async fn decide(&self, log: &str) -> Result<String> {
        let req = CompletionRequest {
            model: self.deps.model.clone(),
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: CURATOR_SYSTEM.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: format!(
                        "Turn log (user request first, then the tool calls in order):\n{log}"
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(2048),
            reasoning_effort: None,
            stop: None,
        };
        let mut rx = self.deps.provider.stream_completion(req).await?;
        let mut text = String::new();
        while let Some(chunk) = rx.recv().await {
            text.push_str(&chunk.delta);
            if chunk.is_finished {
                break;
            }
        }
        if text.trim().is_empty() {
            return Err(PurrError::Provider {
                provider: self.deps.provider.provider_name().to_string(),
                message: "curator model returned an empty reply".to_string(),
            });
        }
        Ok(text)
    }
}

/// The prompt that saves a staged draft (run as a normal turn:
/// Meowstro delegates to Tinker, who runs `manage_skill
/// action=create`). The content travels inline — the draft lives
/// outside the workspace, so the read tool could not reach it through
/// confinement. Scope is `global`: a curator draft is a cross-project
/// workflow (it was staged under `~/.purrcode/skills/.drafts/`), and
/// the tool's default would be the project scope.
pub fn save_prompt_for(meta: &DraftMeta) -> String {
    format!(
        "Save the skill draft '{}' by running manage_skill action=create with scope 'global', name '{}', description '{}', and the content below. Do nothing else. Do not read any file — the content is already below.\n\n{}",
        meta.name, meta.name, meta.description, meta.content
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::StreamChunk;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A mock provider whose replies are scripted by content: the
    /// curator system prompt earns a fixed reply; anything else earns
    /// plain text. `fail` makes the stream call fail once.
    struct MockProvider {
        reply: String,
        fail: AtomicBool,
    }

    impl MockProvider {
        fn new(reply: &str) -> Self {
            Self {
                reply: reply.to_string(),
                fail: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl ModelProvider for MockProvider {
        fn provider_name(&self) -> &str {
            "mock"
        }

        fn is_healthy(&self) -> bool {
            true
        }

        async fn stream_completion(
            &self,
            _req: CompletionRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<purrcode_api::provider::StreamChunk>> {
            if self.fail.swap(false, Ordering::SeqCst) {
                return Err(PurrError::Provider {
                    provider: "mock".to_string(),
                    message: "mock provider down".to_string(),
                });
            }
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let reply = self.reply.clone();
            let _ = tx.try_send(StreamChunk {
                delta: reply,
                reasoning_delta: None,
                tool_call_delta: None,
                is_finished: true,
                usage: None,
            });
            Ok(rx)
        }
    }

    fn tmp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "purr-curator-runner-test-{tag}-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn seed_audit(home: &Path, session_id: &str, tool_calls: usize) {
        let logger = AuditLogger::new(home);
        for i in 0..tool_calls {
            let _ = logger.log_entry(AuditEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                session_id: session_id.to_string(),
                agent: "meowstro".to_string(),
                event: "tool_call".to_string(),
                tool: Some(format!("tool_{i}")),
                decision: "allow".to_string(),
                detail: "mock".to_string(),
            });
        }
    }

    fn user_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: MessageRole::User,
            content: text.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    const DRAFT: &str = "---\nname: cargo-test-loop\ndescription: Run tests, fix failures, re-run until green.\n---\n\n# Cargo test loop\n\n## Steps\n1. Run `cargo test`.\n2. Fix the first failure.\n3. Re-run until green.\n";

    fn runner_for(home: &Path, reply: &str) -> CuratorRunner {
        let provider: Arc<dyn ModelProvider> = Arc::new(MockProvider::new(reply));
        CuratorRunner::new(CuratorDeps {
            provider,
            model: "mock:mock-model".to_string(),
            purrcode_dir: home.to_path_buf(),
            audit_logger: Arc::new(AuditLogger::new(home)),
        })
    }

    #[tokio::test]
    async fn test_below_threshold_writes_marker_and_resets() {
        let home = tmp_home("below");
        let sid = "purr_below";
        seed_audit(&home, sid, 3);
        let r = runner_for(&home, DRAFT);
        let mgr = SkillManager::new();
        let out = r.run_after_turn(sid, &[user_msg("hi")], 0, &mgr).await;
        assert!(matches!(out, CuratorOutcome::BelowThreshold { count: 3 }));
        // Marker consumed the count: a second check with no new calls
        // is still below threshold (and finds no staged draft).
        seed_audit(&home, sid, 0);
        let out2 = r.run_after_turn(sid, &[user_msg("hi")], 0, &mgr).await;
        assert!(matches!(out2, CuratorOutcome::BelowThreshold { count: 0 }));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_threshold_stages_draft_and_marker_resets() {
        let home = tmp_home("stage");
        let sid = "purr_stage";
        seed_audit(&home, sid, 5);
        let r = runner_for(&home, DRAFT);
        let mgr = SkillManager::new();
        let out = r
            .run_after_turn(sid, &[user_msg("fix the tests")], 0, &mgr)
            .await;
        match out {
            CuratorOutcome::DraftStaged { meta, path } => {
                assert_eq!(meta.name, "cargo-test-loop");
                assert!(path.exists());
                // Session-keyed staged file (F4).
                assert!(path
                    .to_string_lossy()
                    .contains("skills/.drafts/cargo-test-loop.purr_stage.md"));
            }
            other => panic!("expected DraftStaged, got {other:?}"),
        }
        // The marker reset the counter: the next turn starts at zero
        // even though the audit still holds the first 5 tool_call
        // events.
        seed_audit(&home, sid, 0);
        let out2 = r.run_after_turn(sid, &[user_msg("again")], 0, &mgr).await;
        assert!(matches!(out2, CuratorOutcome::BelowThreshold { count: 0 }));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// F3: the save turn's `task` delegation runs with the parent's
    /// session id. Without a consume step it leaks into the next
    /// turn's threshold; `consume_save_turn` writes a marker that
    /// eats it.
    #[tokio::test]
    async fn test_save_turn_calls_leak_without_consume_and_consumed_with() {
        let home = tmp_home("save-leak");
        let sid = "purr_save_leak";
        seed_audit(&home, sid, 5);
        let r = runner_for(&home, DRAFT);
        let mgr = SkillManager::new();
        let out = r
            .run_after_turn(sid, &[user_msg("fix the tests")], 0, &mgr)
            .await;
        assert!(matches!(out, CuratorOutcome::DraftStaged { .. }));
        // One more tool call, as the save turn's `task` delegation
        // would make: without a consume step the next turn sees it.
        seed_audit(&home, sid, 1);
        let out2 = r.run_after_turn(sid, &[user_msg("hi")], 0, &mgr).await;
        assert!(matches!(out2, CuratorOutcome::BelowThreshold { count: 1 }));
        // Same again, but the hook consumes the save turn first.
        seed_audit(&home, sid, 1);
        r.consume_save_turn(sid);
        let out3 = r.run_after_turn(sid, &[user_msg("hi")], 0, &mgr).await;
        assert!(matches!(out3, CuratorOutcome::BelowThreshold { count: 0 }));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_no_skill_and_decide_failure() {
        let home = tmp_home("noskill");
        let sid = "purr_noskill";
        seed_audit(&home, sid, 6);
        let r = runner_for(&home, "NO_SKILL");
        let mgr = SkillManager::new();
        let out = r
            .run_after_turn(sid, &[user_msg("what time is it")], 0, &mgr)
            .await;
        assert!(matches!(out, CuratorOutcome::NoDraft));

        let home2 = tmp_home("fail");
        let sid2 = "purr_fail";
        seed_audit(&home2, sid2, 5);
        let failing_provider: Arc<dyn ModelProvider> = Arc::new({
            let m = MockProvider::new(DRAFT);
            m.fail.store(true, Ordering::SeqCst);
            m
        });
        let r3 = CuratorRunner::new(CuratorDeps {
            provider: failing_provider,
            model: "mock:mock-model".to_string(),
            purrcode_dir: home2.clone(),
            audit_logger: Arc::new(AuditLogger::new(&home2)),
        });
        let out3 = r3
            .run_after_turn(sid2, &[user_msg("fix the tests")], 0, &mgr)
            .await;
        assert!(matches!(out3, CuratorOutcome::DecideFailed { .. }));
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&home2);
    }

    #[test]
    fn test_save_prompt_carries_content_inline() {
        let meta = DraftMeta {
            name: "cargo-test-loop".to_string(),
            description: "Run tests, fix failures, re-run until green.".to_string(),
            content: DRAFT.to_string(),
        };
        let prompt = save_prompt_for(&meta);
        assert!(prompt.contains("manage_skill action=create"));
        assert!(prompt.contains("cargo-test-loop"));
        assert!(prompt.contains("Do not read any file"));
        assert!(prompt.contains("# Cargo test loop"));
    }
}
