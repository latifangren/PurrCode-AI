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

mod inference;
mod result;

pub use inference::{resolve_role_model_spec, ResolvedRoleInference};
pub use result::format_validated_result_markdown;

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
        // No fallback candidate resolved to a configured provider (or no
        // routing info at all): fall back to the SESSION provider + SESSION
        // model — the documented dead-model fallback (preset → frontmatter →
        // session model). The previous code returned `spec.model` here, which
        // is the role's *preset* model id (e.g. `ollama:qwen2.5-coder:7b`)
        // wired to the session provider: a strict provider 404s on the
        // foreign id, and the task-result label names a model that never ran.
        // (Checker finding F1, batch 2c verification, 2026-11-12.) An
        // explicit `task()` model naming an unconfigured provider falls
        // through here the same way rather than sending a foreign id to the
        // session provider.
        (self.provider.clone(), self.model.clone())
    }

    fn build_role_tools(&self, role: &str, workspace_root: Option<&Path>) -> ToolRegistry {
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
                reg = ToolRegistry::with_defaults_and_options(
                    self.ask_requester.clone(),
                    workspace_root,
                );
            }
        }
        // In a cron run the job allowlist (enforced by the gate) also
        // narrows the toolset the model sees, so it does not waste steps
        // attempting tools that will be denied. `read_skill` is always
        // kept (it loads skill bodies on demand), mirroring the direct
        // path's retain in main.rs. The shadow auditor does NOT go
        // through here — its fixed read-only registry is intentional:
        // an audit must be able to read the plan/diff it judges.
        if let Some(gate) = &self.approval_gate {
            if let Some(allow) = gate.tool_allowlist() {
                reg.retain(|name| name == "read_skill" || allow.contains(name));
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

        // Dry-run handling: Return a structured delegation preview immediately without
        // allocating continuation IDs, touching disk, invoking shadow auditor, or running the child loop.
        if ctx.is_dry_run {
            let role_spec = resolve_role_model_spec(
                &args.role,
                args.model.as_deref(),
                self.model_preset.as_ref(),
                &self.model,
                &ctx.workspace_root,
            );
            let proposed_details = args
                .diff_preview
                .as_deref()
                .or(args.plan_json.as_deref())
                .unwrap_or("(none provided)");
            let preview = format!(
                "Delegation Plan to Subagent:\n- **Role:** {}\n- **Target Model:** {}\n- **Continuation ID:** {}\n- **Objective:** {}\n- **Proposed Plan / Diff:**\n{}",
                args.role,
                role_spec.model,
                args.id.as_deref().unwrap_or("new"),
                args.brief,
                proposed_details
            );
            return Ok(ToolResult {
                output: format!("Dry-run delegation plan preview for role '{}'", args.role),
                diff_preview: Some(preview),
                metadata: Some(serde_json::json!({
                    "is_dry_run": true,
                    "role": args.role,
                    "model": role_spec.model,
                })),
            });
        }

        // 1. Resolve or allocate stable continuation ID + load prior conversation history
        let (subagent_id, mut history) = {
            let mut reg = self.registry.lock().await;
            let id = reg.get_or_create(args.id.clone(), &args.role, &args.brief)?;
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

        // 2b. Narrow Tuxeshadow Shadow Auditor trigger on risky/cron/replan/plan_json delegations (ADR-012).
        // `is_cron` is forced from the run context, not trusted from the
        // model's own `task()` arguments: a cron run is a cron run even
        // if Meowstro forgets (or refuses) to set the flag. Likewise a
        // model-supplied `shadow_override_reason` is ignored in a cron
        // run — without it, a Tabby objection fails the job closed
        // (shadow.rs: Tabby + object + no reason => Err).
        let is_cron = args.is_cron.unwrap_or(false) || ctx.is_cron;
        let override_reason = if ctx.is_cron {
            None
        } else {
            args.shadow_override_reason.as_deref()
        };
        let mut effective_brief = args.brief.clone();
        if should_trigger_shadow(
            &args.role,
            &existing_brief,
            is_cron,
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
                    override_reason,
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
        let child_tools = Arc::new(self.build_role_tools(&args.role, Some(&ctx.workspace_root)));

        let child_context = ToolContext {
            session_id: format!("{}:{}", ctx.session_id, subagent_id),
            agent_role: args.role.clone(),
            workspace_root: ctx.workspace_root.clone(),
            is_dry_run: ctx.is_dry_run,
            is_cron: ctx.is_cron,
        };

        let mut child_loop = AgentLoop::new(child_provider, &child_model, child_tools)
            // 25, not 15: a real implementation sub-task (scaffold + implement
            // + test + fix) needs ~20 tool calls. Dogfooding 2026-11-12 hit the
            // 15-step cap mid-task and the summary came back a stub, forcing a
            // second delegation to finish the job. Same default as the parent
            // loop (loop_engine.rs); the continuation contract still bounds
            // runaway children (resume, never respawn).
            .with_max_steps(25)
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
        let result_entry = match format_validated_result_markdown(
            &subagent_id,
            &args.role,
            "completed",
            &child_result,
        ) {
            Ok(entry) => entry,
            Err(e) => {
                let mut reg = self.registry.lock().await;
                let _ = reg.record_failure(&subagent_id, "failed", &e.to_string());
                return Err(e);
            }
        };
        if let Err(e) = tokio::fs::write(&result_file, result_entry.as_bytes()).await {
            let mut reg = self.registry.lock().await;
            let _ = reg.record_failure(&subagent_id, "failed", &e.to_string());
            return Err(PurrError::Subagent {
                id: subagent_id.clone(),
                message: format!("Failed to write result.md: {e}"),
            });
        }

        // 7. Save updated history & registry record to memory and disk
        {
            let mut reg = self.registry.lock().await;
            reg.save_history(&subagent_id, history)?;
            reg.record_result(&subagent_id, &child_result)?;
        }

        // The summary below is complete and inline: the result.md path is a
        // session-state record, not a source to re-read. (Dogfooding 2026-11-12:
        // the old "Handoff result saved to: <path>" line made Meowstro read
        // result.md via the `read` tool, which workspace confinement denies —
        // the handoff file lives outside the workspace root. Pointing the
        // orchestrator at a file it cannot open is a wart; the inline summary
        // is the handoff.)
        let summary = format!(
            "Subagent '{}' (role: {}, model: {}) completed its task.\nContinuation ID: {}\nHandoff result saved to session state at {} (record only — the full summary follows inline below, do not re-read the file).\n\nSummary Report:\n{}",
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
mod tests;
