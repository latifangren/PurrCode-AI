// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::Result;
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::tui::AskOptionItem;
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::{AgentLoop, ApprovalRequester, SessionStore, SteeringQueue};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::{AskOption, AskRequester, ToolRegistry, ToolRegistryExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) async fn run_tui(
    config: &Config,
    model_override: Option<String>,
    yolo: bool,
    use_orchestrator: bool,
    workspace_root: PathBuf,
    purrcode_dir: PathBuf,
) -> Result<()> {
    // Audit labels must say which agent actually ran: the generic single
    // agent is not Meowstro.
    let agent_role = crate::cli::agent_role_for(use_orchestrator);

    let target_model = crate::one_shot::resolve_target_model_or_exit(model_override, config);

    let registry = ProviderRegistry::from_config(config)?;
    let (provider_name, wire_model, _provider_cfg, _model_cfg) =
        match config.resolve_model(&target_model) {
            Some(resolved) => resolved,
            None => {
                eprintln!(
                    "Error: Model '{}' could not be resolved from configuration.",
                    target_model
                );
                std::process::exit(1);
            }
        };

    let provider = match registry.get(provider_name) {
        Some(p) => p,
        None => {
            eprintln!(
                "Error: Provider '{}' has no base_url or client configured.",
                provider_name
            );
            std::process::exit(1);
        }
    };

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<purrcode_channel::TuiEvent>(64);
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (steer_tx, mut steer_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let steering_queue = SteeringQueue::new();
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let sq_forwarder = steering_queue.clone();
    tokio::spawn(async move {
        while let Some(msg) = steer_rx.recv().await {
            sq_forwarder.push(msg);
        }
    });

    struct TuiApprovalRequester {
        event_tx: tokio::sync::mpsc::Sender<purrcode_channel::TuiEvent>,
        /// Set when the user presses `A` on an approval card ("approve always
        /// for this session"). Later prompts auto-approve without a card.
        /// Only skips the prompt: the policy gate's hard denials run before
        /// this requester is consulted, so "always" cannot override them.
        session_allow: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl purrcode_core::ApprovalRequester for TuiApprovalRequester {
        async fn request_approval(
            &self,
            _call_id: &str,
            tool: &str,
            description: &str,
            preview: Option<&str>,
        ) -> bool {
            if self.session_allow.load(Ordering::SeqCst) {
                return true;
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            let req = purrcode_channel::ApprovalRequest {
                tool_name: tool.to_string(),
                description: description.to_string(),
                diff_preview: preview.map(|p| p.to_string()),
                tx,
                always: self.session_allow.clone(),
            };
            if self
                .event_tx
                .send(purrcode_channel::TuiEvent::ApprovalRequested(req))
                .await
                .is_err()
            {
                return false;
            }
            rx.await.unwrap_or(false)
        }
    }

    struct TuiAskRequester {
        event_tx: tokio::sync::mpsc::Sender<purrcode_channel::TuiEvent>,
    }

    #[async_trait::async_trait]
    impl AskRequester for TuiAskRequester {
        async fn request_decision(&self, question: &str, options: &[AskOption]) -> Option<String> {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let req = purrcode_channel::AskPromptRequest {
                question: question.to_string(),
                options: options
                    .iter()
                    .map(|o| AskOptionItem {
                        id: o.id.clone(),
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect(),
                selected_idx: 0,
                tx,
            };
            if self
                .event_tx
                .send(purrcode_channel::TuiEvent::AskRequested(req))
                .await
                .is_err()
            {
                return None;
            }
            rx.await.ok().flatten()
        }
    }

    let approval_requester: Arc<dyn purrcode_core::ApprovalRequester> =
        Arc::new(TuiApprovalRequester {
            event_tx: event_tx.clone(),
            session_allow: Arc::new(AtomicBool::new(false)),
        });
    let ask_requester: Arc<dyn AskRequester> = Arc::new(TuiAskRequester {
        event_tx: event_tx.clone(),
    });

    let mode_preset = match ModePresetLoader::load_by_name(&config.preset, &workspace_root) {
        Ok(p) => p,
        Err(_) => ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?,
    };
    let model_preset_name = config
        .model_preset
        .clone()
        .unwrap_or_else(|| "free".to_string());
    let model_preset = ModelPresetLoader::load_by_name(&model_preset_name, &workspace_root).ok();

    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, yolo));
    let policy_gate: Arc<dyn PolicyGate> = approval_gate.clone();

    let audit_logger = Arc::new(AuditLogger::new(&purrcode_dir));
    let audit_sink: Arc<dyn AuditSink> = audit_logger.clone();

    let mut plugin_mgr = PluginManager::new();
    plugin_mgr.discover_all(&workspace_root);
    let unenforced_caps = plugin_mgr.unenforced_capabilities();
    let plugin_dispatcher: Arc<dyn PluginHookDispatcher> = Arc::new(plugin_mgr);

    let mut tool_registry =
        ToolRegistry::with_defaults_and_options(Some(ask_requester.clone()), Some(&workspace_root));
    if !mode_preset.tools.is_empty() {
        tool_registry.retain(|name| {
            name == "read_skill" || mode_preset.tools.iter().any(|allowed| allowed == name)
        });
    }
    let mut mcp_manager = McpManager::new();
    let _ = mcp_manager
        .load_servers(&config.mcp_servers, &mut tool_registry)
        .await;

    let session_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
    plugin_dispatcher.dispatch_session_start(&session_id).await;
    purrcode_service::log_unenforced_plugin_capabilities(
        &audit_logger,
        &session_id,
        agent_role,
        &unenforced_caps,
    );

    let session_store = Arc::new(SessionStore::new(&purrcode_dir));
    if let (Ok(p_val), Ok(mp_val)) = (
        serde_json::to_value(&mode_preset),
        serde_json::to_value(&model_preset),
    ) {
        let _ = session_store.save_preset_snapshots(&session_id, &p_val, &mp_val);
    }

    let _ = audit_logger.log_entry(AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: session_id.clone(),
        agent: agent_role.to_string(),
        event: "session_start".to_string(),
        tool: None,
        decision: "allow".to_string(),
        detail: format!("preset={} yolo={}", config.preset, yolo),
    });

    let registry_arc = Arc::new(registry);
    let session_dir = session_store.session_dir(&session_id);
    let meowstro = if use_orchestrator {
        Some(MeowstroOrchestrator::with_complete_options(
            provider.clone(),
            wire_model,
            &session_dir,
            Some(approval_requester.clone()),
            Some(ask_requester.clone()),
            Some(approval_gate.clone()),
            Some(audit_sink.clone()),
            Some(plugin_dispatcher.clone()),
            model_preset.clone(),
            Some(config.clone()),
            Some(registry_arc),
            Some(steering_queue.clone()),
            Some(cancel_flag.clone()),
            yolo,
            Some(workspace_root.clone()),
            None,
        ))
    } else {
        None
    };

    let direct_loop = AgentLoop::new(provider.clone(), wire_model, Arc::new(tool_registry))
        .with_approval_requester(approval_requester)
        .with_policy_gate(policy_gate)
        .with_audit_sink(audit_sink)
        .with_plugin_dispatcher(plugin_dispatcher.clone())
        .with_steering_queue(steering_queue)
        .with_cancel_flag(cancel_flag.clone())
        .with_yolo(yolo);

    let tool_context = ToolContext {
        session_id: session_id.clone(),
        agent_role: agent_role.to_string(),
        workspace_root: workspace_root.clone(),
        is_dry_run: false,
        is_cron: false,
    };

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(&workspace_root);
    let skills_index = skill_mgr.format_prompt_index();

    let memory = MarkdownMemory::new(&purrcode_dir);
    let mem_context = memory.load_context(Some(&workspace_root)).await;

    let mut system_prompt = if use_orchestrator {
        MeowstroOrchestrator::system_prompt(&skills_index)
    } else {
        let mut base = String::from("You are PurrCode AI, a modular and highly capable AI assistant. Answer user queries accurately. When you need information about files or want to execute commands, call the appropriate tools.");
        if !skills_index.is_empty() {
            base.push_str("\n\n");
            base.push_str(&skills_index);
        }
        base
    };
    if !mem_context.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&mem_context);
    }

    struct TuiEventListener {
        event_tx: tokio::sync::mpsc::Sender<purrcode_channel::TuiEvent>,
    }

    impl purrcode_core::AgentEventListener for TuiEventListener {
        fn on_text_chunk(&self, chunk: &str, is_reasoning: bool) {
            if is_reasoning {
                let _ = self
                    .event_tx
                    .try_send(purrcode_channel::TuiEvent::ReasoningChunk(
                        chunk.to_string(),
                    ));
            } else {
                let _ = self
                    .event_tx
                    .try_send(purrcode_channel::TuiEvent::TextChunk(chunk.to_string()));
            }
        }

        fn on_tool_call_started(&self, name: &str, args: &str) {
            let _ = self
                .event_tx
                .try_send(purrcode_channel::TuiEvent::ToolCallStarted {
                    name: name.to_string(),
                    args: args.to_string(),
                });
        }

        fn on_tool_call_finished(&self, name: &str, output: &str, success: bool) {
            let _ = self
                .event_tx
                .try_send(purrcode_channel::TuiEvent::ToolCallFinished {
                    name: name.to_string(),
                    output: output.to_string(),
                    success,
                });
        }
    }

    let listener: Arc<dyn purrcode_core::AgentEventListener> = Arc::new(TuiEventListener {
        event_tx: event_tx.clone(),
    });

    let mut session_messages: Vec<purrcode_api::provider::ChatMessage> = Vec::new();
    let event_tx_worker = event_tx.clone();
    let store_worker = session_store.clone();
    let sid_worker = session_id.clone();

    // F20 curator deps, captured for the turn worker below (ADR-014
    // item 7). `wire_model` is a borrowed &str (Copy), so it is still
    // usable here even though the orchestrator/direct loop took it.
    let curator_provider = provider.clone();
    let curator_model = wire_model.to_string();
    let curator_purrcode_dir = purrcode_dir.clone();
    let curator_audit = audit_logger.clone();

    tokio::spawn(async move {
        while let Some(user_prompt) = prompt_rx.recv().await {
            if user_prompt == "/__internal_reset" {
                session_messages.clear();
                continue;
            }
            let runner = if let Some(ref m) = meowstro {
                m.agent_loop()
            } else {
                &direct_loop
            };
            let prev_len = session_messages.len();
            let _ = runner
                .run_turn_with_history(
                    &mut session_messages,
                    &user_prompt,
                    Some(&system_prompt),
                    &tool_context,
                    Some(listener.as_ref()),
                )
                .await;
            for msg in session_messages.iter().skip(prev_len) {
                let _ = store_worker.append_message(&sid_worker, msg);
            }
            // F20 curator (post-turn, ADR-014 item 7): only when the
            // turn made enough tool calls. The approval card is a
            // second TuiApprovalRequester with its own session_allow,
            // so pressing `A` ("always") here auto-approves future
            // skill drafts only — never tool calls. The runner is
            // hoisted out of the outcome block: it is needed again
            // after the save turn (F3: consume its calls).
            let curator_runner = purrcode_service::curator::CuratorRunner::new(
                purrcode_service::curator::CuratorDeps {
                    provider: curator_provider.clone(),
                    model: curator_model.clone(),
                    purrcode_dir: curator_purrcode_dir.clone(),
                    audit_logger: curator_audit.clone(),
                },
            );
            let curator_outcome = {
                let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
                skill_mgr.discover_all(&tool_context.workspace_root);
                curator_runner
                    .run_after_turn(&sid_worker, &session_messages, prev_len, &skill_mgr)
                    .await
            };
            // Every outcome is matched explicitly (the audit already
            // carries the detail; the debug lines make the curator's
            // decision observable while debugging, and reading the
            // fields keeps the enum honest).
            match curator_outcome {
                purrcode_service::curator::CuratorOutcome::DraftStaged { meta, path } => {
                    let skill_approver = TuiApprovalRequester {
                        event_tx: event_tx_worker.clone(),
                        session_allow: Arc::new(AtomicBool::new(false)),
                    };
                    let preview: String = meta.content.chars().take(2000).collect();
                    let approved = skill_approver
                    .request_approval(
                        &format!("skill-draft:{}", meta.name),
                        "manage_skill",
                        &format!(
                            "Save the drafted skill '{}'?\n{}\n\nDraft staged at {} — it becomes a live skill only if you approve.",
                            meta.name,
                            meta.description,
                            path.display()
                        ),
                        Some(&preview),
                    )
                    .await;
                    if approved {
                        // Save turn: a normal turn (Meowstro delegates to
                        // Tinker, who runs manage_skill action=create).
                        let save_prompt = purrcode_service::curator::save_prompt_for(&meta);
                        let prev_save = session_messages.len();
                        let _ = runner
                            .run_turn_with_history(
                                &mut session_messages,
                                &save_prompt,
                                Some(&system_prompt),
                                &tool_context,
                                Some(listener.as_ref()),
                            )
                            .await;
                        for msg in session_messages.iter().skip(prev_save) {
                            let _ = store_worker.append_message(&sid_worker, msg);
                        }
                        // F3: the save turn's `task` delegation runs
                        // with the parent's session id — consume it so
                        // it does not count toward the next turn's
                        // threshold.
                        curator_runner.consume_save_turn(&sid_worker);
                        let _ = purrcode_service::curator::discard_draft(
                            &curator_purrcode_dir,
                            &sid_worker,
                            &meta.name,
                        );
                    } else {
                        let _ = purrcode_service::curator::discard_draft(
                            &curator_purrcode_dir,
                            &sid_worker,
                            &meta.name,
                        );
                        let _ = curator_audit.log_entry(AuditEntry {
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            session_id: sid_worker.clone(),
                            agent: "curator".to_string(),
                            event: "skill_draft_discarded".to_string(),
                            tool: None,
                            decision: "ok".to_string(),
                            detail: format!("draft '{}' discarded by the user", meta.name),
                        });
                    }
                }
                purrcode_service::curator::CuratorOutcome::BelowThreshold { count } => {
                    tracing::debug!("curator: below threshold ({count} tool calls)");
                }
                purrcode_service::curator::CuratorOutcome::DecideFailed { reason } => {
                    tracing::debug!("curator: decide failed: {reason}");
                }
                purrcode_service::curator::CuratorOutcome::NoDraft => {
                    tracing::debug!("curator: model answered NO_SKILL");
                }
                purrcode_service::curator::CuratorOutcome::DraftRejected { reason } => {
                    tracing::debug!("curator: draft rejected: {reason}");
                }
            }
            let _ = event_tx_worker
                .send(purrcode_channel::TuiEvent::TurnFinished)
                .await;
        }
    });

    let mut app =
        purrcode_channel::TuiApp::new(target_model, config.preset.clone(), session_id.clone())
            .with_model_preset(model_preset_name);
    // Pending deliveries at open (v0.3 F22, ADR-014 item 10): the
    // delivery queue is the second endpoint of the shared delivery
    // path (the gateway is the first). Showing a delivery here IS its
    // delivery, so it is marked delivered right after seeding — the
    // same at-most-once rule as the Telegram drain: showing it twice
    // would be worse than the TUI crashing before the user reads it.
    seed_pending_deliveries(&mut app, &purrcode_dir);
    let tui_res = purrcode_channel::TuiRunner::run_with_controls(
        app,
        Some(event_rx),
        Some(prompt_tx),
        Some(steer_tx),
        Some(cancel_flag),
    );
    plugin_dispatcher.dispatch_session_stop(&session_id).await;
    tui_res
}

/// Seed pending queue deliveries into the TUI transcript at open
/// (v0.3 F22, ADR-014 item 10). One assistant line per delivery
/// (summary first, detail capped at 400 chars so a long cron result
/// does not flood the first screen). Each seeded delivery is marked
/// delivered: the TUI showing it IS the delivery, and the queue is
/// shared with the Telegram drain — whoever reaches the user first
/// consumes it.
fn seed_pending_deliveries(app: &mut purrcode_channel::TuiApp, purrcode_dir: &std::path::Path) {
    let queue = purrcode_service::cron::DeliveryQueue::new(purrcode_dir);
    let pending = match queue.pending() {
        Ok(p) => p,
        Err(_) => return, // a broken queue file must not block the TUI
    };
    for delivery in &pending {
        let kind = match delivery.kind {
            purrcode_service::cron::queue::DeliveryKind::Result => "📬 Cron",
            purrcode_service::cron::queue::DeliveryKind::Failure => "⚠️ Cron (failed)",
            purrcode_service::cron::queue::DeliveryKind::Heartbeat => "💓 Heartbeat",
        };
        let detail: String = delivery.detail.chars().take(400).collect();
        let text = if detail.is_empty() {
            format!("{kind}: {} — {}", delivery.job_name, delivery.summary)
        } else {
            format!(
                "{kind}: {} — {}\n{}",
                delivery.job_name, delivery.summary, detail
            )
        };
        app.items.push(purrcode_channel::MessageItem::Assistant {
            text,
            reasoning: None,
        });
        if let Err(e) = queue.mark_delivered(&delivery.id) {
            tracing::warn!(
                "tui: failed to mark delivery {} delivered: {e}",
                delivery.id
            );
        }
    }
}
