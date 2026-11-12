// SPDX-License-Identifier: GPL-3.0-or-later

//! The `purrcode gateway` daemon (v0.2, ADR-013).
//!
//! Owns the long-lived wiring the one-shot/TUI paths build per run:
//! provider registry, approval gate, audit log, plugins, model preset,
//! skills, memory — then long-polls Telegram and runs one agent turn
//! per accepted message. Turns in the SAME chat run sequentially (a
//! per-chat lock); turns in DIFFERENT chats run in parallel.
//!
//! Every turn goes through Meowstro (the orchestrator is the default,
//! per the autonomous-round decision): Meowstro plans, Tinker builds.
//! Approval cards and `ask` pickers surface as Telegram inline
//! keyboards in the same chat.

use crate::onboard;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::telegram::{
    telegram_state_dir, InboundMessage, TelegramApiClient, TelegramChannel, TelegramError,
};
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::SessionStore;
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};

/// Shared daemon state: everything a turn needs that outlives one
/// chat message.
struct GatewayState {
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
    /// Plugins that declare network/filesystem capabilities (NOT
    /// enforced in v0.1): logged once per chat session (ADR-013 /
    /// batch 2a).
    unenforced_caps: Vec<(String, Vec<&'static str>)>,
    model_preset: Option<purrcode_config::ModelPreset>,
    /// v0.3 F21: the OS-sandbox strictness for this gateway's turns
    /// (`Some` when the active preset declares `sandbox: "os"`).
    sandbox: Option<purrcode_api::tool::SandboxStrictness>,
    system_prompt: String,
    channel: Arc<TelegramChannel>,
    /// One lock per chat: turns in the same chat run sequentially.
    chat_locks: Mutex<HashMap<i64, Arc<Mutex<()>>>>,
    /// Sessions whose `session_start` plugin hook already fired.
    started_sessions: Mutex<HashSet<String>>,
}

/// Run the gateway daemon until Ctrl-C (or a fatal Telegram error).
pub async fn run_gateway(
    config: &Config,
    workspace_root: &Path,
    model_override: Option<String>,
) -> Result<()> {
    let tg = config.telegram.clone().ok_or_else(|| PurrError::Config(
        "No [telegram] section in config. Add one (see purrcode.jsonc.example) and set TELEGRAM_BOT_TOKEN in ~/.purrcode/credentials, then run `purrcode-ai gateway`.".to_string(),
    ))?;
    // The loader already expanded ${TELEGRAM_BOT_TOKEN} from env /
    // ~/.purrcode/credentials. A leftover "${...}" means the variable
    // was never set — fail with a clear message, not a 401 later.
    let token = tg.bot_token.clone().unwrap_or_default();
    if token.trim().is_empty() || token.contains("${") {
        return Err(PurrError::Config(
            "Telegram bot token is not set. Put TELEGRAM_BOT_TOKEN in ~/.purrcode/credentials (or the environment), then run `purrcode-ai gateway`.".to_string(),
        ));
    }
    if tg.allow_from.is_empty() {
        // Fail-closed (ADR-013): an empty allowlist authorizes nobody,
        // and running a daemon that answers nobody is a misconfiguration
        // worth surfacing loudly instead of silently.
        eprintln!(
            "⚠️  [telegram] allow_from is empty — the bot will answer NOBODY. Add your chat ID to allow_from to let anyone talk to it."
        );
    }

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

    let mode_preset = ModePresetLoader::load_by_name_or_tabby(&config.preset, workspace_root)?;
    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, false));

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
    let mut system_prompt = MeowstroOrchestrator::system_prompt(&skills_index);
    if !mem_context.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&mem_context);
    }

    // Validate the token up front: getMe must answer before the daemon
    // pretends to listen.
    let api = Arc::new(TelegramApiClient::new(tg.api_base(), &token)?);
    let bot_info = match api.get_me().await {
        Ok(info) => info,
        Err(TelegramError::Unauthorized) => {
            return Err(PurrError::Config(
                "Telegram rejected the bot token (401). Check TELEGRAM_BOT_TOKEN in ~/.purrcode/credentials.".to_string(),
            ));
        }
        Err(e) => {
            return Err(PurrError::Config(format!(
                "Could not reach the Telegram Bot API at {} ({e}). For a local mock, set telegram.api_base to the mock server.",
                tg.api_base()
            )));
        }
    };
    println!(
        "🐾 PurrCode AI Gateway — bot @{} (preset: {}, model: {}:{})",
        bot_info.username, config.preset, provider_name, wire_model
    );
    println!("   Listening for Telegram messages (Ctrl-C to stop)...");
    let _ = provider; // the provider Arc lives in `registry` from here on

    let channel = Arc::new(TelegramChannel::new(api, &tg, &telegram_state_dir()));
    channel.set_bot_info(&bot_info).await;

    let state = Arc::new(GatewayState {
        config: config.clone(),
        workspace_root: workspace_root.to_path_buf(),
        purrcode_dir: purrcode_dir.clone(),
        provider_name: provider_name.to_string(),
        wire_model: wire_model.to_string(),
        registry: Arc::new(registry),
        approval_gate,
        audit_logger: audit_logger.clone(),
        audit_sink,
        plugin_dispatcher,
        unenforced_caps,
        model_preset,
        // v0.3 F21: pin the OS-sandbox strictness at startup so every
        // turn's `bash` children get the same ruleset.
        sandbox: if mode_preset.sandbox == purrcode_config::SandboxMode::Os {
            Some(mode_preset.sandbox_mode)
        } else {
            None
        },
        system_prompt,
        channel: channel.clone(),
        chat_locks: Mutex::new(HashMap::new()),
        started_sessions: Mutex::new(HashSet::new()),
    });

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<InboundMessage>(32);

    // Ctrl-C → flip the shutdown watch; the poll loop and the receiver
    // below both notice it.
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        println!("\n🐾 Gateway stopping...");
        let _ = shutdown_tx_ctrlc.send(true);
    });

    // Cron tick task (v0.3 F19, ADR-014 item 5): fires due jobs and drains
    // pending Telegram deliveries on its own interval, inside its own
    // error boundary (`tick_once` never fails) — a broken job or a dead
    // Telegram can never kill the poll loop, and vice versa. Not waited
    // for on shutdown: like the poll task, it is aborted with the daemon.
    if config.cron.as_ref().map(|c| c.enabled).unwrap_or(true) {
        let tick_state = state.clone();
        let tick_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            cron_tick_loop(tick_state, tick_shutdown).await;
        });
    } else {
        println!("   Cron scheduler disabled in config ([cron] enabled = false).");
    }

    // Heartbeat tick task (v0.3 F22, ADR-014 item 6): the proactive
    // two-phase check (decide, then run only what is worth running),
    // on its own interval inside its own error boundary — a broken
    // heartbeat can never kill the poll loop, and vice versa. Not
    // waited for on shutdown: like the poll task, it is aborted with
    // the daemon.
    if config.heartbeat.as_ref().map(|c| c.enabled).unwrap_or(true) {
        let hb_state = state.clone();
        let hb_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            heartbeat_tick_loop(hb_state, hb_shutdown).await;
        });
    } else {
        println!("   Heartbeat disabled in config ([heartbeat] enabled = false).");
    }

    // Poll task: on a fatal Telegram error (bad token mid-run), log it
    // and shut the daemon down instead of retrying forever.
    let channel_poll = channel.clone();
    let shutdown_rx_poll = shutdown_rx.clone();
    let shutdown_tx_poll = shutdown_tx.clone();
    let poll_handle = tokio::spawn(async move {
        match channel_poll.listen(inbound_tx, shutdown_rx_poll).await {
            Ok(()) => {}
            Err(TelegramError::Unauthorized) => {
                eprintln!("❌ Telegram rejected the bot token (401). Gateway stopping.");
                let _ = shutdown_tx_poll.send(true);
            }
            Err(e) => {
                eprintln!("❌ Telegram poll loop failed: {e}. Gateway stopping.");
                let _ = shutdown_tx_poll.send(true);
            }
        }
    });

    // Turn dispatcher: one task per message; per-chat serialization
    // happens inside `run_turn` via the chat lock.
    let mut shutdown_dispatch = shutdown_rx.clone();
    loop {
        tokio::select! {
            _ = shutdown_dispatch.changed() => {
                break;
            }
            msg = inbound_rx.recv() => {
                let Some(inbound) = msg else {
                    break; // poll task ended
                };
                let state = state.clone();
                tokio::spawn(async move {
                    run_turn(state, inbound).await;
                });
            }
        }
    }

    // Do not wait for a poll stuck inside a long getUpdates: abort the
    // poll task instead, or shutdown can hang up to poll_timeout + 15s
    // after Ctrl-C. Turns already running are NOT waited for — the
    // daemon exits with them in flight (documented in AUDIT_STATUS).
    poll_handle.abort();
    let _ = poll_handle.await;
    Ok(())
}

/// Cron scheduler tick loop (v0.3 F19, ADR-014 item 5). The first tick
/// runs immediately (to catch jobs that came due while the daemon was
/// down), then every `tick_secs` on a steady cadence — missed ticks are
/// skipped, never burst-fired, matching the jobs' own skip semantics.
/// `tick_once` never fails (its errors are logged inside), so this task
/// only ends on shutdown.
async fn cron_tick_loop(state: Arc<GatewayState>, mut shutdown_rx: watch::Receiver<bool>) {
    let tick_secs = state
        .config
        .cron
        .as_ref()
        .map(|c| c.tick_secs())
        .unwrap_or(60);
    let deps = purrcode_service::cron::CronRunnerDeps {
        config: state.config.clone(),
        workspace_root: state.workspace_root.clone(),
        purrcode_dir: state.purrcode_dir.clone(),
        registry: state.registry.clone(),
        provider_name: state.provider_name.clone(),
        wire_model: state.wire_model.clone(),
        audit_logger: state.audit_logger.clone(),
        plugin_dispatcher: state.plugin_dispatcher.clone(),
        unenforced_caps: state.unenforced_caps.clone(),
        model_preset: state.model_preset.clone(),
        system_prompt: state.system_prompt.clone(),
    };
    let runner = purrcode_service::cron::CronRunner::new(deps);
    let channel = state.channel.clone();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(tick_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            _ = interval.tick() => {
                runner.tick_once(&channel, tick_secs).await;
            }
        }
    }
}

/// Heartbeat tick loop (v0.3 F22, ADR-014 item 6). The first tick
/// runs immediately (to catch checks that became due while the daemon
/// was down), then every `interval_mins` on a steady cadence —
/// missed ticks are skipped, never burst-fired. `tick_once` never
/// fails (its errors are logged inside), so this task only ends on
/// shutdown. The tick is a no-op without a `HEARTBEAT.md` task file,
/// so a gateway with no heartbeat configured costs no LLM call.
async fn heartbeat_tick_loop(state: Arc<GatewayState>, mut shutdown_rx: watch::Receiver<bool>) {
    let interval_mins = state
        .config
        .heartbeat
        .as_ref()
        .map(|c| c.interval_mins())
        .unwrap_or(30);
    let deps = purrcode_service::cron::CronRunnerDeps {
        config: state.config.clone(),
        workspace_root: state.workspace_root.clone(),
        purrcode_dir: state.purrcode_dir.clone(),
        registry: state.registry.clone(),
        provider_name: state.provider_name.clone(),
        wire_model: state.wire_model.clone(),
        audit_logger: state.audit_logger.clone(),
        plugin_dispatcher: state.plugin_dispatcher.clone(),
        unenforced_caps: state.unenforced_caps.clone(),
        model_preset: state.model_preset.clone(),
        system_prompt: state.system_prompt.clone(),
    };
    let runner = purrcode_service::heartbeat::HeartbeatRunner::new(deps);
    let channel = state.channel.clone();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_mins * 60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            _ = interval.tick() => {
                runner.tick_once(&channel).await;
            }
        }
    }
}

/// Run one agent turn for one accepted Telegram message.
async fn run_turn(state: Arc<GatewayState>, inbound: InboundMessage) {
    // Serialize turns within a chat. A second message in the same chat
    // waits here until the first turn finishes (its reply lands first).
    let lock = {
        let mut locks = state.chat_locks.lock().await;
        locks
            .entry(inbound.chat_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;

    let session_id = inbound.session_id.clone();
    let session_store = SessionStore::new(&state.purrcode_dir);
    let session_dir = session_store.session_dir(&session_id);

    // Plugin session_start fires once per chat session.
    {
        let mut started = state.started_sessions.lock().await;
        if started.insert(session_id.clone()) {
            state
                .plugin_dispatcher
                .dispatch_session_start(&session_id)
                .await;
            purrcode_service::log_unenforced_plugin_capabilities(
                &state.audit_logger,
                &session_id,
                "meowstro",
                &state.unenforced_caps,
            );
        }
    }

    let _ = state.audit_logger.log_entry(AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: session_id.clone(),
        agent: "meowstro".to_string(),
        event: "telegram_turn_start".to_string(),
        tool: None,
        decision: "allow".to_string(),
        detail: format!("chat={}", inbound.chat_id),
    });

    let renderer = state.channel.renderer(inbound.chat_id);
    let listener = purrcode_channel::telegram::render::TelegramEventListener::new(renderer.clone());
    let approval = state.channel.approval_requester(
        inbound.chat_id,
        session_id.clone(),
        Some(state.audit_sink.clone()),
    );
    let ask = state.channel.ask_requester(inbound.chat_id);

    // Load the chat's history (empty for a new session) and run the
    // turn with it; the loop appends to `messages`, and the new entries
    // are persisted below.
    let mut messages = session_store.load_messages(&session_id).unwrap_or_default();
    let loaded_len = messages.len();

    let provider = match state.registry.get(state.provider_name.as_str()) {
        Some(p) => p,
        None => {
            renderer
                .send_standalone("Terjadi kesalahan: provider tidak ditemukan di konfigurasi.")
                .await;
            return;
        }
    };
    // F20 curator provider (cloned before the orchestrator takes the
    // Arc below).
    let curator_provider = provider.clone();

    let meowstro = MeowstroOrchestrator::with_complete_options(
        provider,
        state.wire_model.clone(),
        &session_dir,
        Some(approval.clone()),
        Some(ask),
        Some(state.approval_gate.clone()),
        Some(state.audit_sink.clone()),
        Some(state.plugin_dispatcher.clone()),
        state.model_preset.clone(),
        Some(state.config.clone()),
        Some(state.registry.clone()),
        None,
        None,
        false,
        Some(state.workspace_root.clone()),
        None,
    );

    let ctx = ToolContext {
        session_id: session_id.clone(),
        agent_role: "meowstro".to_string(),
        workspace_root: state.workspace_root.clone(),
        is_dry_run: false,
        is_cron: false,
        sandbox: state.sandbox,
    };

    // Typing indicator while the turn runs.
    let typing_api = state.channel.api().clone();
    let typing_chat = inbound.chat_id;
    let typing_handle = tokio::spawn(async move {
        loop {
            if typing_api
                .send_chat_action(typing_chat, "typing")
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
    });

    let result = meowstro
        .agent_loop()
        .run_turn_with_history(
            &mut messages,
            &inbound.text,
            Some(&state.system_prompt),
            &ctx,
            Some(&listener),
        )
        .await;

    typing_handle.abort();
    renderer.flush().await;

    match result {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("telegram: turn failed for chat {}: {e}", inbound.chat_id);
            renderer
                .send_standalone("Terjadi kesalahan saat memproses pesan itu. Coba lagi, atau /new untuk sesi baru.")
                .await;
        }
    }
    // Persist the messages this turn added, even when the turn failed:
    // otherwise the user's message is lost from history on every error.
    for msg in messages.iter().skip(loaded_len) {
        if let Err(e) = session_store.append_message(&session_id, msg) {
            tracing::warn!("telegram: failed to persist message for {session_id}: {e}");
        }
    }

    // F20 curator (post-turn, ADR-014 item 7): only when the turn made
    // enough tool calls. The approval is the skill-draft card (Ya /
    // Tidak / Selalu, 120s → deny); on approve a second, normal turn
    // saves the draft (Meowstro delegates to Tinker). The hook holds
    // the chat lock while it waits — same as a tool approval inside a
    // turn — so two turns in one chat never interleave.
    // The runner is hoisted out of the outcome block: it is needed
    // again after the save turn (F3: consume its calls).
    let curator_runner =
        purrcode_service::curator::CuratorRunner::new(purrcode_service::curator::CuratorDeps {
            provider: curator_provider,
            model: state.wire_model.clone(),
            purrcode_dir: state.purrcode_dir.clone(),
            audit_logger: state.audit_logger.clone(),
        });
    let curator_outcome = {
        let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
        skill_mgr.discover_all(&state.workspace_root);
        curator_runner
            .run_after_turn(&session_id, &messages, loaded_len, &skill_mgr)
            .await
    };
    // Every outcome is matched explicitly (the audit already carries
    // the detail; the debug lines make the curator's decision
    // observable while debugging, and reading the fields keeps the
    // enum honest).
    match curator_outcome {
        purrcode_service::curator::CuratorOutcome::DraftStaged { meta, path: _ } => {
            let draft_approver = state.channel.skill_draft_requester(
                inbound.chat_id,
                session_id.clone(),
                Some(state.audit_sink.clone()),
            );
            let preview: String = meta.content.chars().take(2000).collect();
            let approved = draft_approver
                .request_approval(&meta.name, &meta.description, Some(&preview))
                .await;
            if approved {
                let save_prompt = purrcode_service::curator::save_prompt_for(&meta);
                let prev_save = messages.len();
                let save_result = meowstro
                    .agent_loop()
                    .run_turn_with_history(
                        &mut messages,
                        &save_prompt,
                        Some(&state.system_prompt),
                        &ctx,
                        Some(&listener),
                    )
                    .await;
                renderer.flush().await;
                // F3: the save turn's `task` delegation runs with the
                // parent's session id — consume it so it does not
                // count toward the next turn's threshold. Runs
                // whether the save succeeded or failed (the calls
                // are spent either way).
                curator_runner.consume_save_turn(&session_id);
                if let Err(e) = save_result {
                    tracing::warn!(
                        "telegram: skill save turn failed for chat {}: {e}",
                        inbound.chat_id
                    );
                    renderer
                    .send_standalone(
                        "Gagal menyimpan skill draft. Draft masih tersimpan dan bisa dicoba lagi.",
                    )
                    .await;
                } else {
                    let _ = purrcode_service::curator::discard_draft(
                        &state.purrcode_dir,
                        &session_id,
                        &meta.name,
                    );
                }
                for msg in messages.iter().skip(prev_save) {
                    if let Err(e) = session_store.append_message(&session_id, msg) {
                        tracing::warn!("telegram: failed to persist message for {session_id}: {e}");
                    }
                }
            } else {
                let _ = purrcode_service::curator::discard_draft(
                    &state.purrcode_dir,
                    &session_id,
                    &meta.name,
                );
                let _ = state.audit_logger.log_entry(purrcode_approval::AuditEntry {
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
}
