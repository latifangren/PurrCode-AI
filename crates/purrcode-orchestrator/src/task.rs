// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::{
    ApprovalTier, AuditSink, PolicyGate, Tool, ToolContext, ToolDefinition, ToolResult,
};
use purrcode_approval::ApprovalGate;
use purrcode_config::{Config, GuardrailMode, ModelPreset};
use purrcode_core::{AgentLoop, ApprovalRequester};
use purrcode_provider::{FallbackRouter, ProviderRegistry};
use purrcode_tools::{AskRequester, ToolRegistry, ToolRegistryExt};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::shadow::{should_trigger_shadow, ShadowAuditor};
use crate::SubagentRegistry;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRoleInference {
    pub model: String,
    pub fallback_candidates: Vec<String>,
    pub temperature: Option<f32>,
    pub reasoning_effort: Option<String>,
}

/// Resolves the inference settings (model, fallback candidates, temperature, reasoning effort)
/// for a subagent role according to ARCHITECTURE.md §3.5 precedence:
/// 1. Explicit `model` in `task()` (disables fallback chain).
/// 2. Session `ModelPreset` entry for `role`.
/// 3. Frontmatter `model:` in `.purrcode/agents/<role>.md` or `agents/<role>.md`.
/// 4. Session model (with `reasoning_effort = Some("high")` for `tuxeshadow` and `sphinx` per ADR-012).
pub fn resolve_role_model_spec(
    role: &str,
    explicit_model: Option<&str>,
    model_preset: Option<&ModelPreset>,
    session_model: &str,
    workspace_root: &Path,
) -> ResolvedRoleInference {
    let default_effort = if role == "tuxeshadow" || role == "sphinx" {
        Some("high".to_string())
    } else {
        None
    };

    // 1. Explicit model override in task()
    if let Some(exp) = explicit_model {
        let trimmed = exp.trim();
        if !trimmed.is_empty() {
            return ResolvedRoleInference {
                model: trimmed.to_string(),
                fallback_candidates: vec![trimmed.to_string()],
                temperature: Some(0.2),
                reasoning_effort: default_effort,
            };
        }
    }

    // 2. Session ModelPreset entry for role
    if let Some(preset) = model_preset {
        if let Some(spec) = preset.agents.get(role) {
            let mut candidates = spec.model.candidates();
            for fb in &spec.fallback {
                if !candidates.contains(fb) {
                    candidates.push(fb.clone());
                }
            }
            let primary = candidates
                .first()
                .cloned()
                .unwrap_or_else(|| session_model.to_string());
            return ResolvedRoleInference {
                model: primary,
                fallback_candidates: candidates,
                temperature: spec.temperature.or(Some(0.2)),
                reasoning_effort: spec.variant.clone().or(default_effort),
            };
        }
    }

    // 3. Agent Markdown frontmatter (`model:`)
    if let Some(fm_model) = read_agent_frontmatter_model(role, workspace_root) {
        return ResolvedRoleInference {
            model: fm_model.clone(),
            fallback_candidates: vec![fm_model],
            temperature: Some(0.2),
            reasoning_effort: default_effort,
        };
    }

    // 4. Session model fallback
    ResolvedRoleInference {
        model: session_model.to_string(),
        fallback_candidates: vec![session_model.to_string()],
        temperature: Some(0.2),
        reasoning_effort: default_effort,
    }
}

fn read_agent_frontmatter_model(role: &str, workspace_root: &Path) -> Option<String> {
    let candidates = [
        workspace_root
            .join(".purrcode")
            .join("agents")
            .join(format!("{}.md", role)),
        workspace_root.join("agents").join(format!("{}.md", role)),
    ];
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Some(rest) = content.strip_prefix("---") {
                if let Some(end_idx) = rest.find("---") {
                    for line in rest[..end_idx].lines() {
                        if let Some(m) = line.trim().strip_prefix("model:") {
                            let val = m.trim().trim_matches('"').trim_matches('\'');
                            if !val.is_empty() {
                                return Some(val.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Validates and formats `result.md` according to the schema in `docs/SUBAGENTS.md` §2.2.
pub fn format_validated_result_markdown(
    subagent_id: &str,
    role: &str,
    status: &str,
    raw_summary: &str,
) -> String {
    let has_changed = raw_summary.contains("Changed Files");
    let has_verified = raw_summary.contains("Verified Evidence");
    let has_failures = raw_summary.contains("Failures") || raw_summary.contains("Blockers");
    let has_unverified = raw_summary.contains("Unverified Items");

    let mut out = format!(
        "# Result Report: {}\n- **Status:** {}\n- **Role:** {}\n- **Timestamp:** {}\n",
        subagent_id,
        status,
        role,
        chrono::Utc::now().to_rfc3339()
    );

    if !has_changed {
        out.push_str("- **Changed Files:** (see summary below)\n");
    }
    if !has_verified {
        out.push_str("- **Verified Evidence:** (see summary below)\n");
    }
    if !has_failures {
        out.push_str("- **Failures / Blockers:** None reported.\n");
    }
    if !has_unverified {
        out.push_str("- **Unverified Items:** None reported.\n");
    }

    out.push_str(&format!("\n## Summary\n{}\n", raw_summary.trim()));
    out
}

pub struct TaskTool {
    registry: Arc<Mutex<SubagentRegistry>>,
    provider: Arc<dyn ModelProvider>,
    model: String,
    session_dir: PathBuf,
    approval_requester: Option<Arc<dyn ApprovalRequester>>,
    ask_requester: Option<Arc<dyn AskRequester>>,
    approval_gate: Option<Arc<ApprovalGate>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    plugin_dispatcher: Option<Arc<dyn PluginHookDispatcher>>,
    model_preset: Option<ModelPreset>,
    config: Option<Config>,
    provider_registry: Option<Arc<ProviderRegistry>>,
    yolo: bool,
}

impl TaskTool {
    pub fn new(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::with_options(registry, provider, model, session_dir, None, None, false)
    }

    pub fn with_options(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        yolo: bool,
    ) -> Self {
        Self::with_full_options(
            registry,
            provider,
            model,
            session_dir,
            approval_requester,
            ask_requester,
            None,
            None,
            yolo,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_full_options(
        registry: Arc<Mutex<SubagentRegistry>>,
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        approval_gate: Option<Arc<ApprovalGate>>,
        audit_sink: Option<Arc<dyn AuditSink>>,
        yolo: bool,
    ) -> Self {
        Self {
            registry,
            provider,
            model: model.into(),
            session_dir: session_dir.into(),
            approval_requester,
            ask_requester,
            approval_gate,
            audit_sink,
            plugin_dispatcher: None,
            model_preset: None,
            config: None,
            provider_registry: None,
            yolo,
        }
    }

    pub fn with_plugin_dispatcher(mut self, dispatcher: Arc<dyn PluginHookDispatcher>) -> Self {
        self.plugin_dispatcher = Some(dispatcher);
        self
    }

    pub fn with_model_preset(mut self, preset: Option<ModelPreset>) -> Self {
        self.model_preset = preset;
        self
    }

    pub fn with_provider_routing(
        mut self,
        config: Config,
        provider_registry: Arc<ProviderRegistry>,
    ) -> Self {
        self.config = Some(config);
        self.provider_registry = Some(provider_registry);
        self
    }

    fn resolve_provider_and_model(
        &self,
        spec: &ResolvedRoleInference,
    ) -> (Arc<dyn ModelProvider>, String) {
        if let (Some(ref cfg), Some(ref preg)) = (&self.config, &self.provider_registry) {
            let mut resolved_chain = Vec::new();
            for cand in &spec.fallback_candidates {
                if let Some((p_name, wire_model, _, _)) = cfg.resolve_model(cand) {
                    if let Some(p_arc) = preg.get(p_name) {
                        resolved_chain.push((p_arc, wire_model.to_string()));
                    }
                }
            }
            if resolved_chain.len() == 1 {
                return resolved_chain.remove(0);
            } else if resolved_chain.len() > 1 {
                let first_wire = resolved_chain[0].1.clone();
                let router = FallbackRouter::new("subagent-router", resolved_chain);
                return (Arc::new(router), first_wire);
            }
        }
        (self.provider.clone(), spec.model.clone())
    }

    fn build_role_tools(&self, role: &str) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        match role {
            "whisker" => {
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
                let _ = reg.register(Arc::new(purrcode_tools::web::WebfetchTool::new()));
                let _ = reg.register(Arc::new(purrcode_tools::web::WebsearchTool::new()));
            }
            "sphinx" | "tuxeshadow" => {
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
                let ask_tool = match self.ask_requester.clone() {
                    Some(req) => purrcode_tools::coordination::AskTool::with_requester(req),
                    None => purrcode_tools::coordination::AskTool::new(),
                };
                let _ = reg.register(Arc::new(ask_tool));
            }
            "calico" => {
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::fs::WriteTool));
                let _ = reg.register(Arc::new(purrcode_tools::fs::EditTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
            }
            "clowder" => {
                let _ = reg.register(Arc::new(purrcode_tools::fs::ReadTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GrepTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::GlobTool));
                let _ = reg.register(Arc::new(purrcode_tools::search::FindTool));
            }
            _ => {
                reg = ToolRegistry::with_defaults_and_ask(self.ask_requester.clone());
            }
        }
        reg.freeze();
        reg
    }
}

#[derive(Deserialize)]
struct TaskArgs {
    role: String,
    id: Option<String>,
    brief: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    shadow_override_reason: Option<String>,
    #[serde(default)]
    plan_json: Option<String>,
    #[serde(default)]
    diff_preview: Option<String>,
    #[serde(default)]
    is_cron: Option<bool>,
    #[serde(default)]
    is_replan: Option<bool>,
}

#[async_trait]
impl Tool for TaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "task".to_string(),
            description: "Delegate a task to a specialized subagent (tinker, whisker, sphinx, calico, tuxeshadow, clowder). Preserves context via continuation ID.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "enum": ["tinker", "whisker", "sphinx", "calico", "tuxeshadow", "clowder"],
                        "description": "Subagent role: 'tinker' (fixer), 'whisker' (explorer), 'sphinx' (oracle), 'calico' (designer), 'tuxeshadow' (shadow auditor), 'clowder' (council)"
                    },
                    "id": {
                        "type": "string",
                        "description": "Optional continuation ID (purr_<id>) to resume an existing subagent without losing previous context"
                    },
                    "brief": {
                        "type": "string",
                        "description": "Clear instructions, target files/lines, constraints, and expected deliverables"
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional explicit model override for this subagent delegation (overrides model-preset)"
                    },
                    "plan_json": {
                        "type": "string",
                        "description": "Optional raw execution plan JSON artifact for Tuxeshadow pre-execution audit"
                    },
                    "diff_preview": {
                        "type": "string",
                        "description": "Optional raw diff preview artifact for Tuxeshadow audit"
                    },
                    "is_cron": {
                        "type": "boolean",
                        "description": "Set true when invoked from an automated scheduled/cron job (mandatory Tuxeshadow audit)"
                    },
                    "is_replan": {
                        "type": "boolean",
                        "description": "Set true when replanning after a previous error or failure (mandatory Tuxeshadow audit)"
                    },
                    "shadow_override_reason": {
                        "type": "string",
                        "description": "Optional reason to override an advisory Tuxeshadow objection in Tabby (default) mode (recorded in audit.jsonl)"
                    }
                },
                "required": ["role", "brief"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args: TaskArgs = serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
            tool: "task".to_string(),
            message: format!("Invalid task arguments: {e}"),
        })?;

        if ctx.agent_role != "meowstro" && !ctx.agent_role.is_empty() {
            return Err(PurrError::Subagent {
                id: ctx.session_id.clone(),
                message: "Subagent nesting is forbidden in v0.1 (max depth = 1)".to_string(),
            });
        }

        // 1. Resolve or allocate stable continuation ID + load prior conversation history
        let (subagent_id, mut history) = {
            let mut reg = self.registry.lock().await;
            let id = reg.get_or_create(args.id.clone(), &args.role, &args.brief);
            let hist = reg.take_history(&id);
            (id, hist)
        };

        // 2. Prepare handoff directory: sessions/<session_id>/subagents/<id>/
        let subagent_dir = self.session_dir.join("subagents").join(&subagent_id);
        tokio::fs::create_dir_all(&subagent_dir)
            .await
            .map_err(|e| PurrError::Subagent {
                id: subagent_id.clone(),
                message: format!("Failed to create subagent handoff directory: {e}"),
            })?;

        let brief_file = subagent_dir.join("brief.md");
        let result_file = subagent_dir.join("result.md");

        let mut existing_brief = tokio::fs::read_to_string(&brief_file)
            .await
            .unwrap_or_default();
        if existing_brief.is_empty() {
            existing_brief.push_str(&format!(
                "# Delegation Brief: {}\n- **Subagent ID:** {}\n- **Role:** {}\n- **Objective:** {}\n",
                args.role, subagent_id, args.role, args.brief
            ));
        } else {
            existing_brief.push_str(&format!(
                "\n## Follow-Up Turn ({})\n- **Brief:** {}\n",
                chrono::Utc::now().to_rfc3339(),
                args.brief
            ));
        }
        if let Some(ref pjson) = args.plan_json {
            existing_brief.push_str(&format!(
                "\n### Raw Plan JSON (plan_json:)\n```json\n{}\n```\n",
                pjson
            ));
        }
        if let Some(ref dprev) = args.diff_preview {
            existing_brief.push_str(&format!(
                "\n### Raw Diff Artifact (diff_preview:)\n```diff\n{}\n```\n",
                dprev
            ));
        }
        tokio::fs::write(&brief_file, existing_brief.as_bytes())
            .await
            .map_err(|e| PurrError::Subagent {
                id: subagent_id.clone(),
                message: format!("Failed to write brief.md: {e}"),
            })?;

        // 2b. Narrow Tuxeshadow Shadow Auditor trigger on risky/cron/replan/plan_json delegations (ADR-012)
        let mut effective_brief = args.brief.clone();
        if should_trigger_shadow(
            &args.role,
            &existing_brief,
            args.is_cron.unwrap_or(false),
            args.is_replan.unwrap_or(false),
        ) {
            let guardrail = self
                .approval_gate
                .as_ref()
                .map(|g| g.guardrail)
                .unwrap_or(GuardrailMode::Default);
            let shadow_spec = resolve_role_model_spec(
                "tuxeshadow",
                None,
                self.model_preset.as_ref(),
                &self.model,
                &ctx.workspace_root,
            );
            let (shadow_provider, shadow_model) = self.resolve_provider_and_model(&shadow_spec);
            let mut auditor = ShadowAuditor::new(shadow_provider, shadow_model, guardrail)
                .with_reasoning_effort(shadow_spec.reasoning_effort);
            if let Some(ref sink) = self.audit_sink {
                auditor = auditor.with_audit_sink(sink.clone());
            }
            if let Some(ref req) = self.approval_requester {
                auditor = auditor.with_approval_requester(req.clone());
            }
            let verdict = match auditor.audit_raw_artifact(&existing_brief, ctx).await {
                Ok(v) => v,
                Err(e) => {
                    let mut reg = self.registry.lock().await;
                    let _ = reg.record_failure(&subagent_id, "failed", &e.to_string());
                    return Err(e);
                }
            };
            match auditor
                .enforce_verdict_with_preview(
                    &verdict,
                    &ctx.session_id,
                    args.shadow_override_reason.as_deref(),
                    args.diff_preview
                        .as_deref()
                        .or(args.plan_json.as_deref())
                        .or(Some(existing_brief.as_str())),
                )
                .await
            {
                Ok(Some(shadow_note)) => {
                    effective_brief.push_str(&format!("\n\n{}", shadow_note));
                }
                Ok(None) => {}
                Err(e) => {
                    let mut reg = self.registry.lock().await;
                    let _ = reg.record_failure(&subagent_id, "blocked", &e.to_string());
                    return Err(e);
                }
            }
        }

        // 3. Resolve per-role model & inference parameters from ModelPreset / frontmatter / override
        let role_spec = resolve_role_model_spec(
            &args.role,
            args.model.as_deref(),
            self.model_preset.as_ref(),
            &self.model,
            &ctx.workspace_root,
        );
        let (child_provider, child_model) = self.resolve_provider_and_model(&role_spec);

        // 4. Assemble child toolset according to role
        let child_tools = Arc::new(self.build_role_tools(&args.role));

        let child_context = ToolContext {
            session_id: format!("{}:{}", ctx.session_id, subagent_id),
            agent_role: args.role.clone(),
            workspace_root: ctx.workspace_root.clone(),
            is_dry_run: ctx.is_dry_run,
        };

        let mut child_loop = AgentLoop::new(child_provider, &child_model, child_tools)
            .with_max_steps(15)
            .with_temperature(role_spec.temperature)
            .with_reasoning_effort(role_spec.reasoning_effort)
            .with_yolo(self.yolo);
        if let Some(ref req) = self.approval_requester {
            child_loop = child_loop.with_approval_requester(req.clone());
        }
        if let Some(ref parent_gate) = self.approval_gate {
            let narrowed: Arc<dyn PolicyGate> =
                Arc::new(parent_gate.narrowed_child_gate(parent_gate.mode));
            child_loop = child_loop.with_policy_gate(narrowed);
        }
        if let Some(ref sink) = self.audit_sink {
            child_loop = child_loop.with_audit_sink(sink.clone());
        }
        if let Some(ref pd) = self.plugin_dispatcher {
            child_loop = child_loop.with_plugin_dispatcher(pd.clone());
        }

        let system_prompt = format!(
            "You are {}, a specialized subagent of PurrCode AI.\nYour continuation ID is '{}'.\nPerform your assigned task thoroughly using your allowed tools and return a concise structured summary (changed files:line, verified evidence, blockers, unverified items).",
            args.role, subagent_id
        );

        // 5. Run child loop (Clowder runs 2 council seats and merges verdicts)
        let child_result_res = if args.role == "clowder" {
            let mut h1 = history.clone();
            let mut h2 = history.clone();
            let seat1_prompt = format!(
                "[Council Seat 1 — Architecture & Safety Lens]\n{}",
                effective_brief
            );
            let seat2_prompt = format!(
                "[Council Seat 2 — Pragmatism & Edge-Case Lens]\n{}",
                effective_brief
            );
            let (r1, r2) = tokio::join!(
                child_loop.run_turn_with_history(
                    &mut h1,
                    &seat1_prompt,
                    Some(&system_prompt),
                    &child_context,
                    None
                ),
                child_loop.run_turn_with_history(
                    &mut h2,
                    &seat2_prompt,
                    Some(&system_prompt),
                    &child_context,
                    None
                )
            );
            let v1 = r1.unwrap_or_else(|e| format!("[Seat 1 failed: {e}]"));
            let v2 = r2.unwrap_or_else(|e| format!("[Seat 2 failed: {e}]"));
            history = h1;
            Ok(format!(
                "### Council Seat 1\n{}\n\n### Council Seat 2\n{}",
                v1, v2
            ))
        } else {
            child_loop
                .run_turn_with_history(
                    &mut history,
                    &effective_brief,
                    Some(&system_prompt),
                    &child_context,
                    None,
                )
                .await
        };

        let child_result = match child_result_res {
            Ok(r) => r,
            Err(e) => {
                let mut reg = self.registry.lock().await;
                let _ = reg.record_failure(&subagent_id, "failed", &e.to_string());
                return Err(e);
            }
        };

        // 6. Write validated result.md
        let result_entry =
            format_validated_result_markdown(&subagent_id, &args.role, "completed", &child_result);
        tokio::fs::write(&result_file, result_entry.as_bytes())
            .await
            .map_err(|e| PurrError::Subagent {
                id: subagent_id.clone(),
                message: format!("Failed to write result.md: {e}"),
            })?;

        // 7. Save updated history & registry record to memory and disk
        {
            let mut reg = self.registry.lock().await;
            reg.save_history(&subagent_id, history)?;
            reg.record_result(&subagent_id, &child_result)?;
        }

        let summary = format!(
            "Subagent '{}' (role: {}, model: {}) completed its task.\nContinuation ID: {}\nHandoff result saved to: {}\n\nSummary Report:\n{}",
            subagent_id,
            args.role,
            child_model,
            subagent_id,
            result_file.display(),
            child_result
        );

        Ok(ToolResult {
            output: summary,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "subagent_id": subagent_id,
                "role": args.role,
                "model": child_model,
                "status": "completed"
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::{CompletionRequest, MessageRole, StreamChunk};
    use purrcode_config::{AgentModelSpec, ModelTarget};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StatefulEchoProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ModelProvider for StatefulEchoProvider {
        fn provider_name(&self) -> &str {
            "mock-stateful"
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stream_completion(
            &self,
            req: CompletionRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let user_turns = req
                .messages
                .iter()
                .filter(|m| m.role == MessageRole::User)
                .count();
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            let _ = tx.try_send(StreamChunk {
                delta: format!(
                    "call={} user_turns={} model={} effort={:?}",
                    n, user_turns, req.model, req.reasoning_effort
                ),
                reasoning_delta: None,
                tool_call_delta: None,
                is_finished: true,
                usage: None,
            });
            Ok(rx)
        }
    }

    #[test]
    fn test_per_role_model_preset_resolution_and_precedence() {
        let mut agents = HashMap::new();
        agents.insert(
            "tuxeshadow".to_string(),
            AgentModelSpec {
                model: ModelTarget::Chain(vec![
                    "oracle-primary".to_string(),
                    "oracle-backup".to_string(),
                ]),
                variant: Some("high".to_string()),
                temperature: Some(0.1),
                options: None,
                fallback: vec!["oracle-last".to_string()],
            },
        );
        let preset = ModelPreset {
            schema: "model-preset/v1".to_string(),
            name: "test".to_string(),
            description: String::new(),
            extends: None,
            agents,
        };

        // 1. ModelPreset resolves primary, fallback chain, temperature, and high variant
        let spec = resolve_role_model_spec(
            "tuxeshadow",
            None,
            Some(&preset),
            "session-default",
            Path::new("."),
        );
        assert_eq!(spec.model, "oracle-primary");
        assert_eq!(spec.fallback_candidates.len(), 3);
        assert_eq!(spec.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(spec.temperature, Some(0.1));

        // 2. Explicit model in task() wins over ModelPreset and disables fallback chain
        let exp = resolve_role_model_spec(
            "tuxeshadow",
            Some("override-model"),
            Some(&preset),
            "session-default",
            Path::new("."),
        );
        assert_eq!(exp.model, "override-model");
        assert_eq!(exp.fallback_candidates, vec!["override-model".to_string()]);
    }

    #[tokio::test]
    async fn test_subagent_chain_handoff_and_resume_by_id() {
        let temp_dir =
            std::env::temp_dir().join(format!("purr_orch_test_{}", uuid::Uuid::new_v4()));
        let provider: Arc<dyn ModelProvider> = Arc::new(StatefulEchoProvider {
            calls: AtomicUsize::new(0),
        });
        let reg = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
        let gate = Arc::new(ApprovalGate::new("tabby"));
        let task_tool = TaskTool::with_full_options(
            reg.clone(),
            provider.clone(),
            "mock-model",
            &temp_dir,
            None,
            None,
            Some(gate),
            None,
            true,
        );

        let ctx = ToolContext {
            session_id: "root_sess".to_string(),
            agent_role: "meowstro".to_string(),
            workspace_root: temp_dir.clone(),
            is_dry_run: false,
        };

        // Step 1: Delegate to Whisker
        let r1 = task_tool
            .execute(
                serde_json::json!({
                    "role": "whisker",
                    "brief": "Scout auth handler"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let whisker_id = r1.metadata.unwrap()["subagent_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(whisker_id.starts_with("purr_"));
        assert!(r1.output.contains("user_turns=1"));

        // Step 2: Delegate to Sphinx (verifies high reasoning effort default)
        let r2 = task_tool
            .execute(
                serde_json::json!({
                    "role": "sphinx",
                    "brief": "Verify root cause in auth.rs"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let sphinx_id = r2.metadata.unwrap()["subagent_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(whisker_id, sphinx_id);
        assert!(r2.output.contains("effort=Some(\"high\")"));

        // Step 3: Resume Whisker by exact continuation ID after simulated restart
        let reg_reloaded = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(&temp_dir)));
        let task_tool_reloaded = TaskTool::with_options(
            reg_reloaded,
            provider,
            "mock-model",
            &temp_dir,
            None,
            None,
            true,
        );

        let r3 = task_tool_reloaded
            .execute(
                serde_json::json!({
                    "role": "whisker",
                    "id": whisker_id,
                    "brief": "Follow up on second auth file"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let resumed_id = r3.metadata.unwrap()["subagent_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(resumed_id, whisker_id);
        assert!(r3.output.contains("user_turns=2"));

        let res_md = std::fs::read_to_string(
            temp_dir
                .join("subagents")
                .join(&whisker_id)
                .join("result.md"),
        )
        .unwrap();
        assert!(res_md.contains("- **Changed Files:**"));
        assert!(res_md.contains("- **Verified Evidence:**"));
        assert!(res_md.contains("- **Failures / Blockers:**"));
        assert!(res_md.contains("- **Unverified Items:**"));

        assert!(temp_dir
            .join("subagents")
            .join(format!("{}.json", whisker_id))
            .exists());
        assert!(temp_dir.join("workgraph.json").exists());

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
