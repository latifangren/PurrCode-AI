// SPDX-License-Identifier: GPL-3.0-or-later

//! The `purrcode web` command (v0.3 R3).
//!
//! Owns the long-lived wiring the gateway owns, but for the browser
//! instead of Telegram: provider registry, approval gate, audit log,
//! plugins, model preset, skills, memory — plus one shared [`EventBus`]
//! (R3) and the [`WebChannel`] the server streams from.
//!
//! Event flow (exactly once per event, by construction):
//!
//! * The loop publishes every chunk, tool call, approval request and
//!   turn boundary to the bus (see `AgentLoop::with_event_bus`).
//! * One forwarder task pumps the bus into the `WebChannel` broadcast,
//!   which the server's `/api/events` SSE endpoint streams to the
//!   browser. `WebChannel::request_approval` deliberately does NOT
//!   broadcast — the loop's bus event is the card — so the UI renders
//!   each approval exactly once.
//! * Tool approvals resolve through `/api/approval` (keyed by the
//!   loop's call id, which the bus event carries); skill-draft
//!   approvals resolve through `/api/skills/approve` (keyed
//!   `skill-draft:<name>`, announced by `WebSkillDraftRequester`).
//!
//! Turns serialize on one lock (the browser page is a single chat, like
//! one Telegram chat in the gateway). Session `purr_web` persists in
//! the session store, so the WebUI chat survives a restart.

use crate::onboard;
use purrcode_api::channel::Channel;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::event::AgentEvent;
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::{PromptDispatcher, WebChannel, WebServer};
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::{AgentLoop, ApprovalRequester, EventBus, SessionStore};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_tools::{ToolRegistry, ToolRegistryExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

/// Fixed session id for the WebUI chat (one shared chat, like one
/// Telegram chat in the gateway).
const WEB_SESSION_ID: &str = "purr_web";

/// [`ApprovalRequester`] that waits on the WebUI card. The card itself
/// comes from the loop's bus event (published just before the loop
/// calls here), so this only waits — it never broadcasts a second
/// card. A channel error fails closed (deny).
struct WebApprovalRequester {
    channel: Arc<WebChannel>,
}

#[async_trait::async_trait]
impl ApprovalRequester for WebApprovalRequester {
    async fn request_approval(
        &self,
        call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool {
        self.channel
            .request_approval(call_id, tool, description, preview)
            .await
            .unwrap_or(false)
    }
}

/// Shared daemon state: everything a turn needs that outlives one
/// browser prompt.
struct WebState {
    config: Config,
    workspace_root: PathBuf,
    purrcode_dir: PathBuf,
    provider_name: String,
    wire_model: String,
    registry: Arc<ProviderRegistry>,
    approval_gate: Arc<ApprovalGate>,
    audit_logger: Arc<AuditLogger>,
    audit_sink: Arc<dyn AuditSink>,
    plugin_dispatcher: Arc<dyn PluginHookDispatcher>,
    unenforced_caps: Vec<(String, Vec<&'static str>)>,
    model_preset: Option<purrcode_config::ModelPreset>,
    system_prompt: String,
    use_orchestrator: bool,
    yolo: bool,
    bus: Arc<EventBus>,
    /// One lock for the whole page: turns never interleave.
    turn_lock: Mutex<()>,
    /// Whether the `session_start` plugin hook already fired.
    started: Mutex<bool>,
}

/// Run the WebUI server until Ctrl-C.
#[allow(clippy::too_many_arguments)]
pub async fn run_web_command(
    config: &Config,
    workspace_root: &Path,
    use_orchestrator: bool,
    yolo: bool,
    model_override: Option<String>,
    host: String,
    port: u16,
    token: Option<String>,
) -> Result<()> {
    let target_model = crate::one_shot::resolve_target_model_or_exit(model_override, config);
    let registry = ProviderRegistry::from_config(config)?;
    let (provider_name, wire_model, _provider_cfg, _model_cfg) = match config
        .resolve_model(&target_model)
    {
        Some(resolved) => resolved,
        None => {
            eprintln!("Error: Model '{target_model}' could not be resolved from configuration.");
            eprintln!(
                "Available providers: {:?}",
                config.providers.keys().collect::<Vec<_>>()
            );
            std::process::exit(1);
        }
    };
    let provider = match registry.get(provider_name) {
        Some(p) => p,
        None => {
            eprintln!("Error: Provider '{provider_name}' has no base_url or client configured.");
            std::process::exit(1);
        }
    };

    let purrcode_dir = onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));

    let mode_preset = match ModePresetLoader::load_by_name(&config.preset, workspace_root) {
        Ok(p) => p,
        Err(_) => ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?,
    };
    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, yolo));

    let audit_logger = Arc::new(AuditLogger::new(&purrcode_dir));
    let audit_sink: Arc<dyn AuditSink> = audit_logger.clone();

    let mut plugin_mgr = PluginManager::new();
    plugin_mgr.discover_all(workspace_root);
    let unenforced_caps = plugin_mgr.unenforced_capabilities();
    let plugin_dispatcher: Arc<dyn PluginHookDispatcher> = Arc::new(plugin_mgr);

    let model_preset_name = config
        .model_preset
        .clone()
        .unwrap_or_else(|| "free".to_string());
    let model_preset = ModelPresetLoader::load_by_name(&model_preset_name, workspace_root).ok();

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(workspace_root);
    let skills_index = skill_mgr.format_prompt_index();

    let memory = MarkdownMemory::new(&purrcode_dir);
    let mem_context = memory.load_context(Some(workspace_root)).await;
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

    let bus = Arc::new(EventBus::new(256));
    let channel = Arc::new(WebChannel::new());

    // Forwarder: the bus is the single event source; every event the
    // loop publishes is sent to the channel exactly once. The task
    // ends when the bus is dropped at shutdown.
    {
        let mut bus_rx = bus.subscribe();
        let ch = channel.clone();
        tokio::spawn(async move {
            loop {
                match bus_rx.recv().await {
                    Ok(ev) => {
                        let _ = ch.send_event(&ev).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    println!(
        "🐾 PurrCode AI WebUI — http://{}:{} (preset: {}, model: {}:{})",
        host, port, config.preset, provider_name, wire_model
    );
    if token.is_some() {
        println!(
            "🔒 Token authentication enabled for remote access (open the page with ?token=...)."
        );
    }
    println!("   Listening for browser prompts (Ctrl-C to stop)...");
    let _ = provider; // the provider Arc lives in `registry` from here on

    let state = Arc::new(WebState {
        config: config.clone(),
        workspace_root: workspace_root.to_path_buf(),
        purrcode_dir: purrcode_dir.clone(),
        provider_name: provider_name.to_string(),
        wire_model: wire_model.to_string(),
        registry: Arc::new(registry),
        approval_gate,
        audit_logger,
        audit_sink,
        plugin_dispatcher,
        unenforced_caps,
        model_preset,
        system_prompt,
        use_orchestrator,
        yolo,
        bus,
        turn_lock: Mutex::new(()),
        started: Mutex::new(false),
    });

    let dispatcher = Arc::new(CliWebPromptDispatcher {
        state: state.clone(),
    });
    let server = WebServer::new(
        host,
        port,
        token,
        channel,
        purrcode_dir,
        workspace_root.to_path_buf(),
    )
    .with_dispatcher(dispatcher);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        println!("\n🐾 WebUI stopping...");
        let _ = shutdown_tx_ctrlc.send(true);
    });

    server.run(shutdown_rx).await
}

pub struct CliWebPromptDispatcher {
    state: Arc<WebState>,
}

#[async_trait::async_trait]
impl PromptDispatcher for CliWebPromptDispatcher {
    async fn dispatch(&self, prompt: String, channel: Arc<WebChannel>) -> Result<()> {
        self.state.run_web_turn(prompt, channel).await
    }
}

impl WebState {
    /// Run one agent turn for one browser prompt, then the F20 curator
    /// hook (mirrors `gateway::run_turn`, minus the Telegram renderer:
    /// events flow through the bus, errors surface as `AgentEvent::Error`).
    async fn run_web_turn(&self, prompt: String, channel: Arc<WebChannel>) -> Result<()> {
        let _guard = self.turn_lock.lock().await;

        let session_id = WEB_SESSION_ID.to_string();
        let session_store = SessionStore::new(&self.purrcode_dir);
        let session_dir = session_store.session_dir(&session_id);

        // Plugin session_start fires once per daemon lifetime.
        {
            let mut started = self.started.lock().await;
            if !*started {
                *started = true;
                self.plugin_dispatcher
                    .dispatch_session_start(&session_id)
                    .await;
                purrcode_service::log_unenforced_plugin_capabilities(
                    &self.audit_logger,
                    &session_id,
                    "meowstro",
                    &self.unenforced_caps,
                );
            }
        }

        let _ = self.audit_logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            agent: "meowstro".to_string(),
            event: "web_turn_start".to_string(),
            tool: None,
            decision: "allow".to_string(),
            detail: "session=purr_web".to_string(),
        });

        let provider = match self.registry.get(self.provider_name.as_str()) {
            Some(p) => p,
            None => {
                return Err(PurrError::Config(
                    "Provider not found in configuration.".to_string(),
                ));
            }
        };
        // F20 curator provider (cloned before the orchestrator takes
        // the Arc below).
        let curator_provider = provider.clone();

        let approval: Arc<dyn ApprovalRequester> = Arc::new(WebApprovalRequester {
            channel: channel.clone(),
        });
        let policy_gate: Arc<dyn PolicyGate> = self.approval_gate.clone();

        let agent_role = crate::cli::agent_role_for(self.use_orchestrator);
        let meowstro = if self.use_orchestrator {
            Some(MeowstroOrchestrator::with_complete_options(
                provider.clone(),
                self.wire_model.clone(),
                &session_dir,
                Some(approval.clone()),
                None,
                Some(self.approval_gate.clone()),
                Some(self.audit_sink.clone()),
                Some(self.plugin_dispatcher.clone()),
                self.model_preset.clone(),
                Some(self.config.clone()),
                Some(self.registry.clone()),
                None,
                None,
                self.yolo,
                Some(self.workspace_root.clone()),
                Some(self.bus.clone()),
            ))
        } else {
            None
        };

        let direct_loop = if self.use_orchestrator {
            None
        } else {
            let mut tool_registry =
                ToolRegistry::with_defaults_and_options(None, Some(&self.workspace_root));
            let mode_preset =
                match ModePresetLoader::load_by_name(&self.config.preset, &self.workspace_root) {
                    Ok(p) => p,
                    Err(_) => {
                        ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?
                    }
                };
            if !mode_preset.tools.is_empty() {
                let mode_tools = mode_preset.tools.clone();
                tool_registry
                    .retain(|name| name == "read_skill" || mode_tools.iter().any(|a| a == name));
            }
            Some(
                AgentLoop::new(provider, self.wire_model.clone(), Arc::new(tool_registry))
                    .with_approval_requester(approval.clone())
                    .with_policy_gate(policy_gate)
                    .with_audit_sink(self.audit_sink.clone())
                    .with_plugin_dispatcher(self.plugin_dispatcher.clone())
                    .with_event_bus(self.bus.clone())
                    .with_yolo(self.yolo),
            )
        };

        // Fail-closed: exactly one of the two loops is built above.
        let loop_runner: &AgentLoop = match (&meowstro, &direct_loop) {
            (Some(m), _) => m.agent_loop(),
            (None, Some(l)) => l,
            (None, None) => {
                return Err(PurrError::Internal(
                    "no agent loop built for web turn".to_string(),
                ));
            }
        };

        let ctx = ToolContext {
            session_id: session_id.clone(),
            agent_role: agent_role.to_string(),
            workspace_root: self.workspace_root.clone(),
            is_dry_run: false,
            is_cron: false,
        };

        // Load the chat's history (empty for a new session) and run the
        // turn with it; the loop appends to `messages`, and the new
        // entries are persisted below.
        let mut messages = session_store.load_messages(&session_id).unwrap_or_default();
        let loaded_len = messages.len();

        let result = loop_runner
            .run_turn_with_history(
                &mut messages,
                &prompt,
                Some(&self.system_prompt),
                &ctx,
                None,
            )
            .await;

        if let Err(e) = result {
            tracing::warn!("web: turn failed: {e}");
            let _ = channel
                .send_event(&AgentEvent::Error {
                    code: "turn_failed".to_string(),
                    message: e.to_string(),
                })
                .await;
            // The error event already told the UI; persist the
            // messages this turn added anyway, then surface the error
            // to the server log via the dispatcher's Result.
            for msg in messages.iter().skip(loaded_len) {
                if let Err(e) = session_store.append_message(&session_id, msg) {
                    tracing::warn!("web: failed to persist message: {e}");
                }
            }
            return Err(e);
        }
        // Persist the messages this turn added.
        for msg in messages.iter().skip(loaded_len) {
            if let Err(e) = session_store.append_message(&session_id, msg) {
                tracing::warn!("web: failed to persist message: {e}");
            }
        }

        // F20 curator (post-turn, ADR-014 item 7): only when the turn
        // made enough tool calls. The approval is the skill-draft card
        // (Ya / Tidak, 120s → deny) rendered in the WebUI; on approve
        // a second, normal turn saves the draft (Meowstro delegates to
        // Tinker). The hook holds the turn lock while it waits — same
        // as a tool approval inside a turn — so two turns never
        // interleave.
        let curator_runner =
            purrcode_service::curator::CuratorRunner::new(purrcode_service::curator::CuratorDeps {
                provider: curator_provider,
                model: self.wire_model.clone(),
                purrcode_dir: self.purrcode_dir.clone(),
                audit_logger: self.audit_logger.clone(),
            });
        let curator_outcome = {
            let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
            skill_mgr.discover_all(&self.workspace_root);
            curator_runner
                .run_after_turn(&session_id, &messages, loaded_len, &skill_mgr)
                .await
        };
        match curator_outcome {
            purrcode_service::curator::CuratorOutcome::DraftStaged { meta, path: _ } => {
                let draft_approver = channel
                    .skill_draft_requester(session_id.clone(), Some(self.audit_sink.clone()));
                let preview: String = meta.content.chars().take(2000).collect();
                let approved = draft_approver
                    .request_approval(&meta.name, &meta.description, Some(&preview))
                    .await;
                if approved {
                    let save_prompt = purrcode_service::curator::save_prompt_for(&meta);
                    let prev_save = messages.len();
                    let save_result = loop_runner
                        .run_turn_with_history(
                            &mut messages,
                            &save_prompt,
                            Some(&self.system_prompt),
                            &ctx,
                            None,
                        )
                        .await;
                    // F3: the save turn's `task` delegation runs with
                    // the parent's session id — consume it so it does
                    // not count toward the next turn's threshold. Runs
                    // whether the save succeeded or failed (the calls
                    // are spent either way).
                    curator_runner.consume_save_turn(&session_id);
                    if let Err(e) = save_result {
                        tracing::warn!("web: skill save turn failed: {e}");
                        let _ = channel
                            .send_event(&AgentEvent::Error {
                                code: "skill_save_failed".to_string(),
                                message: format!(
                                    "Gagal menyimpan skill draft '{}'. Draft masih tersimpan dan bisa dicoba lagi.",
                                    meta.name
                                ),
                            })
                            .await;
                    } else {
                        let _ = purrcode_service::curator::discard_draft(
                            &self.purrcode_dir,
                            &session_id,
                            &meta.name,
                        );
                    }
                    for msg in messages.iter().skip(prev_save) {
                        if let Err(e) = session_store.append_message(&session_id, msg) {
                            tracing::warn!("web: failed to persist message: {e}");
                        }
                    }
                } else {
                    let _ = purrcode_service::curator::discard_draft(
                        &self.purrcode_dir,
                        &session_id,
                        &meta.name,
                    );
                    let _ = self.audit_logger.log_entry(AuditEntry {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        session_id: session_id.clone(),
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

        Ok(())
    }
}
