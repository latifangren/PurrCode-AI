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

    let target_model = super::resolve_target_model_or_exit(model_override, config);
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
            super::log_unenforced_plugin_capabilities(
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
    );

    let ctx = ToolContext {
        session_id: session_id.clone(),
        agent_role: "meowstro".to_string(),
        workspace_root: state.workspace_root.clone(),
        is_dry_run: false,
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
}
