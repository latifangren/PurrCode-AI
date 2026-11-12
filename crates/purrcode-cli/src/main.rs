// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod cron;
mod gateway;
mod onboard;

use clap::Parser;
use cli::{Cli, Commands};
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::tui::AskOptionItem;
use purrcode_config::{
    Config, ConfigLoader, ModePresetLoader, ModelPresetLoader, MIN_CONTEXT_WINDOW,
};
use purrcode_core::{AgentLoop, SessionStore, SteeringQueue};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_service::ServiceInstaller;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::{AskOption, AskRequester, ToolRegistry, ToolRegistryExt};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(windows)]
fn init_windows_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleOutputCP(code_page_id: u32) -> i32;
        fn SetConsoleCP(code_page_id: u32) -> i32;
    }
    unsafe {
        let _ = SetConsoleOutputCP(65001);
        let _ = SetConsoleCP(65001);
    }
}

/// Log one audit entry per loaded plugin that declares network/filesystem
/// capabilities. Those are NOT enforced in v0.1 (see
/// `PluginManager::unenforced_capabilities`); the audit entry is the
/// observable record that the plugin runs with full user rights.
fn log_unenforced_plugin_capabilities(
    logger: &AuditLogger,
    session_id: &str,
    agent: &str,
    unenforced: &[(String, Vec<&'static str>)],
) {
    for (name, kinds) in unenforced {
        let _ = logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            event: "plugin_capability_unenforced".to_string(),
            tool: None,
            decision: "warn".to_string(),
            detail: format!(
                "plugin '{name}' declares {} capabilities, which are NOT enforced in v0.1: process plugins run with full user rights; enforcement comes with the v0.3 OS sandbox",
                kinds.join(" + ")
            ),
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Rust ignores SIGPIPE by default, which turns a closed stdout pipe
    // (e.g. `purrcode-ai cron list | head -1`) into a panic with exit 134.
    // Restore the default disposition so the process dies with SIGPIPE
    // (exit 141) like any other Unix command. No-op on Windows.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    #[cfg(windows)]
    init_windows_console();

    let args = Cli::parse();

    // Orchestrator is the default (PRD §4.5.6: Meowstro read-only + delegation,
    // enforced at harness level). `--direct` opts out to a single general
    // agent with all tools; explicit `--orchestrator` wins if both are given.
    let use_orchestrator = cli::use_orchestrator(args.orchestrator, args.direct);
    // Audit labels must say which agent actually ran: the generic single
    // agent is not Meowstro.
    let agent_role = cli::agent_role_for(use_orchestrator);

    // 1. Load configuration
    let mut config = match &args.config {
        Some(path) => ConfigLoader::load_from_file(path)?,
        None => ConfigLoader::load_effective()?,
    };

    if let Some(ref preset_override) = args.preset {
        config.preset = preset_override.clone();
    }
    if let Some(ref mp_override) = args.model_preset {
        config.model_preset = Some(mp_override.clone());
    }

    let workspace_root = if let Some(ref ws) = args.workspace {
        if ws.is_absolute() {
            ws.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(ws)
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let purrcode_dir = onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));

    // 2. Dispatch subcommands if present
    if let Some(cmd) = args.command {
        match cmd {
            Commands::Onboard => {
                onboard::run_interactive_onboard().await?;
                return Ok(());
            }
            Commands::Doctor => {
                run_doctor(&config, &workspace_root);
                return Ok(());
            }
            Commands::Preset { action, name } => {
                let presets = ModePresetLoader::discover_all(&workspace_root);
                if action == "show" {
                    let target = name.as_deref().unwrap_or(&config.preset);
                    let p = ModePresetLoader::load_by_name(target, &workspace_root)?;
                    println!("{}", serde_json::to_string_pretty(&p)?);
                } else {
                    println!("🐾 Mode Presets (preset/v1):");
                    let mut names: Vec<_> = presets.keys().collect();
                    names.sort();
                    for k in names {
                        if let Some(p) = presets.get(k) {
                            println!(
                                "  - {:<10} (approval={:?}, guardrail={:?}, sandbox={:?}) — {}",
                                p.name, p.approval, p.guardrail, p.sandbox, p.description
                            );
                        }
                    }
                }
                return Ok(());
            }
            Commands::ModelPreset { action, name } => {
                let presets = ModelPresetLoader::discover_all(&workspace_root);
                if action == "show" {
                    let target = name
                        .as_deref()
                        .or(config.model_preset.as_deref())
                        .unwrap_or("free");
                    let p = ModelPresetLoader::load_by_name(target, &workspace_root)?;
                    println!("{}", serde_json::to_string_pretty(&p)?);
                } else {
                    println!("🐾 Model Presets (model-preset/v1):");
                    let mut names: Vec<_> = presets.keys().collect();
                    names.sort();
                    for k in names {
                        if let Some(p) = presets.get(k) {
                            println!(
                                "  - {:<16} ({} subagent mappings) — {}",
                                p.name,
                                p.agents.len(),
                                p.description
                            );
                        }
                    }
                }
                return Ok(());
            }
            Commands::Session { action, id, format } => {
                let store = SessionStore::new(&purrcode_dir);
                match action.as_str() {
                    "export" => {
                        let sid = id.ok_or_else(|| {
                            PurrError::Session("Missing session ID for export".to_string())
                        })?;
                        let out = store.export_session(&sid, &format)?;
                        println!("{}", out);
                    }
                    "fork" => {
                        let sid = id.ok_or_else(|| {
                            PurrError::Session("Missing source session ID for fork".to_string())
                        })?;
                        let new_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
                        store.fork_session(&sid, &new_id)?;
                        println!("✓ Forked session '{}' -> '{}'", sid, new_id);
                    }
                    _ => {
                        let list = store.list_sessions();
                        println!("🐾 Saved Sessions ({}):", list.len());
                        for s in list {
                            println!("  - {}", s);
                        }
                    }
                }
                return Ok(());
            }
            Commands::Plugin { action: _ } => {
                let mut pm = PluginManager::new();
                pm.discover_all(&workspace_root);
                println!("🐾 Discovered Plugins ({}):", pm.plugins().len());
                for p in pm.plugins() {
                    let m = p.manifest();
                    println!("  - {} v{} ({:?})", m.name, m.version, m.transport);
                }
                for (name, kinds) in pm.unenforced_capabilities() {
                    println!(
                        "  ⚠ {} declares {} capabilities — NOT enforced in v0.1 (process plugins run with full user rights; enforcement comes with the v0.3 OS sandbox)",
                        name,
                        kinds.join(" + ")
                    );
                }
                return Ok(());
            }
            Commands::Service { action } => {
                match action.as_str() {
                    "systemd" => {
                        println!("{}", ServiceInstaller::generate_systemd_unit());
                    }
                    "openrc" => {
                        println!("{}", ServiceInstaller::generate_openrc_init());
                    }
                    "windows" | "task" => {
                        println!("{}", ServiceInstaller::generate_windows_task_script(None));
                    }
                    "install" => {
                        #[cfg(windows)]
                        {
                            println!("{}", ServiceInstaller::generate_windows_task_script(None));
                        }
                        #[cfg(not(windows))]
                        {
                            println!("{}", ServiceInstaller::generate_systemd_unit());
                        }
                    }
                    _ => {
                        println!("🐾 Service status: {}", ServiceInstaller::status_report());
                    }
                }
                return Ok(());
            }
            Commands::Exec { prompt } => {
                let exec_prompt = match prompt {
                    Some(p) => p,
                    None => {
                        let mut buf = String::new();
                        std::io::stdin()
                            .read_to_string(&mut buf)
                            .map_err(PurrError::Io)?;
                        buf
                    }
                };
                if exec_prompt.trim().is_empty() {
                    eprintln!("Error: Empty prompt provided to `purrcode-ai exec`.");
                    std::process::exit(1);
                }
                confirm_god_mode_if_needed(&config.preset)?;
                run_one_shot(
                    exec_prompt,
                    args.model,
                    args.json,
                    use_orchestrator,
                    args.yolo,
                    &config,
                    &workspace_root,
                )
                .await?;
                return Ok(());
            }
            Commands::Gateway => {
                gateway::run_gateway(&config, &workspace_root, args.model).await?;
                return Ok(());
            }
            Commands::Cron {
                action,
                name,
                schedule,
                prompt,
                allow,
                preset,
                yes,
            } => {
                cron::cli::run_cron_command(cron::cli::CronArgs {
                    action: &action,
                    name,
                    schedule,
                    prompt,
                    allow,
                    preset,
                    yes,
                    config: &config,
                    workspace_root: &workspace_root,
                    purrcode_dir: &purrcode_dir,
                })
                .await?;
                return Ok(());
            }
        }
    }

    confirm_god_mode_if_needed(&config.preset)?;

    // 3. One-shot prompt mode (-p / --prompt)
    if let Some(prompt) = args.prompt {
        run_one_shot(
            prompt,
            args.model,
            args.json,
            use_orchestrator,
            args.yolo,
            &config,
            &workspace_root,
        )
        .await?;
        return Ok(());
    }

    // 4. Default interactive mode: launch Ratatui TUI connected to AgentLoop / Meowstro
    let target_model = resolve_target_model_or_exit(args.model, &config);

    let registry = ProviderRegistry::from_config(&config)?;
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

    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, args.yolo));
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
    log_unenforced_plugin_capabilities(&audit_logger, &session_id, agent_role, &unenforced_caps);

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
        detail: format!("preset={} yolo={}", config.preset, args.yolo),
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
            args.yolo,
            Some(workspace_root.clone()),
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
        .with_yolo(args.yolo);

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
            let _ = event_tx_worker
                .send(purrcode_channel::TuiEvent::TurnFinished)
                .await;
        }
    });

    let app =
        purrcode_channel::TuiApp::new(target_model, config.preset.clone(), session_id.clone())
            .with_model_preset(model_preset_name);
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

fn confirm_god_mode_if_needed(preset_name: &str) -> Result<()> {
    if preset_name.eq_ignore_ascii_case("oyen") || preset_name.eq_ignore_ascii_case("god") {
        if !std::io::stdin().is_terminal() {
            return Err(PurrError::ApprovalDenied(
                "Preset 'oyen' (God mode) requires explicit interactive confirmation per session and cannot run unattended.".to_string(),
            ));
        }
        eprint!("⚠️  Preset 'oyen' disables agent-side content guardrails (approval and audit log stay active). Confirm for this session? [y/N]: ");
        let _ = std::io::stderr().flush();
        let ans = onboard::read_line_trimmed();
        if !ans.eq_ignore_ascii_case("y") && !ans.eq_ignore_ascii_case("yes") {
            return Err(PurrError::ApprovalDenied(
                "God mode ('oyen') was not confirmed by the user.".to_string(),
            ));
        }
    }
    Ok(())
}

fn resolve_target_model_or_exit(model_override: Option<String>, config: &Config) -> String {
    match model_override.or_else(|| config.model.clone()) {
        Some(m) => m,
        None => {
            // Collect all candidates deterministically
            let mut candidates: Vec<String> = Vec::new();
            let mut sorted_providers: Vec<_> = config.providers.iter().collect();
            sorted_providers.sort_by_key(|(name, _)| (*name).clone());

            for (p_name, p_cfg) in sorted_providers {
                let mut sorted_models: Vec<_> = p_cfg.models.keys().collect();
                sorted_models.sort();
                for m_key in sorted_models {
                    candidates.push(format!("{}:{}", p_name, m_key));
                }
            }

            // Prefer candidates marked primary, default, or main
            let chosen = candidates
                .iter()
                .find(|c| {
                    let lower = c.to_lowercase();
                    lower.contains("primary") || lower.contains("default") || lower.contains("main")
                })
                .or_else(|| candidates.first())
                .cloned();

            match chosen {
                Some(m) => m,
                None => {
                    eprintln!("Error: No model configured.");
                    eprintln!("PurrCode AI has no hardcoded default models.");
                    eprintln!();
                    eprintln!("Please configure your provider in '~/.purrcode/purrcode.jsonc' or run 'purrcode-ai onboard'.");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Non-TTY approval requester for `-p` / `exec` runs. `yolo` is remembered
/// so the denial hint is truthful: with `--yolo` already on, a denial comes
/// from a policy rule `--yolo` cannot override (e.g. an outside-workspace
/// write), not from a missing flag.
struct CliApprovalRequester {
    yolo: bool,
}

#[async_trait::async_trait]
impl purrcode_core::ApprovalRequester for CliApprovalRequester {
    async fn request_approval(&self, tool: &str, description: &str, preview: Option<&str>) -> bool {
        if !std::io::stdin().is_terminal() {
            if self.yolo {
                eprintln!(
                    "⚠️  [Approval Denied]: Tool '{}' requires approval, but --yolo is already on — this denial comes from a policy rule --yolo cannot override (e.g. an outside-workspace write). Run in a terminal to approve it interactively.",
                    tool
                );
            } else {
                eprintln!(
                    "⚠️  [Approval Denied]: Tool '{}' requires approval. Run in terminal or pass --yolo.",
                    tool
                );
            }
            return false;
        }

        eprintln!();
        eprintln!("⚠️  Approval Required: {}", description);
        if let Some(diff) = preview {
            eprintln!("Diff preview:\n{}", diff);
        }
        eprint!("Approve execution of tool '{}'? [y/N]: ", tool);
        let _ = std::io::stderr().flush();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim();
            trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes")
        } else {
            false
        }
    }
}

async fn run_one_shot(
    prompt: String,
    model_override: Option<String>,
    as_json: bool,
    use_orchestrator: bool,
    yolo: bool,
    config: &Config,
    workspace_root: &Path,
) -> Result<()> {
    let target_model = resolve_target_model_or_exit(model_override, config);
    let registry = ProviderRegistry::from_config(config)?;

    let (provider_name, wire_model, _provider_cfg, _model_cfg) =
        match config.resolve_model(&target_model) {
            Some(resolved) => resolved,
            None => {
                eprintln!(
                    "Error: Model '{}' could not be resolved from configuration.",
                    target_model
                );
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
            eprintln!(
                "Error: Provider '{}' has no base_url or client configured.",
                provider_name
            );
            std::process::exit(1);
        }
    };

    let purrcode_dir = onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));

    let mode_preset = match ModePresetLoader::load_by_name(&config.preset, workspace_root) {
        Ok(p) => p,
        Err(_) => ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?,
    };
    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, yolo));
    let policy_gate: Arc<dyn PolicyGate> = approval_gate.clone();

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

    let mut tool_registry = ToolRegistry::with_defaults_and_options(None, Some(workspace_root));
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

    let session_store = SessionStore::new(&purrcode_dir);
    let session_dir = session_store.session_dir(&session_id);

    // Audit labels must say which agent actually ran: the generic single
    // agent is not Meowstro.
    let agent_role = cli::agent_role_for(use_orchestrator);

    log_unenforced_plugin_capabilities(&audit_logger, &session_id, agent_role, &unenforced_caps);

    let _ = audit_logger.log_entry(AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: session_id.clone(),
        agent: agent_role.to_string(),
        event: "one_shot_start".to_string(),
        tool: None,
        decision: "allow".to_string(),
        detail: format!("preset={} yolo={}", config.preset, yolo),
    });

    let tool_context = ToolContext {
        session_id: session_id.clone(),
        agent_role: agent_role.to_string(),
        workspace_root: workspace_root.to_path_buf(),
        is_dry_run: false,
        is_cron: false,
    };

    let cli_approver: Arc<dyn purrcode_core::ApprovalRequester> =
        Arc::new(CliApprovalRequester { yolo });
    let registry_arc = Arc::new(registry);
    let meowstro = if use_orchestrator {
        Some(MeowstroOrchestrator::with_complete_options(
            provider.clone(),
            wire_model,
            &session_dir,
            Some(cli_approver.clone()),
            None,
            Some(approval_gate),
            Some(audit_sink.clone()),
            Some(plugin_dispatcher.clone()),
            model_preset,
            Some(config.clone()),
            Some(registry_arc),
            None,
            None,
            yolo,
            Some(workspace_root.to_path_buf()),
        ))
    } else {
        None
    };

    let agent_loop = AgentLoop::new(provider, wire_model, Arc::new(tool_registry))
        .with_approval_requester(cli_approver)
        .with_policy_gate(policy_gate)
        .with_audit_sink(audit_sink)
        .with_plugin_dispatcher(plugin_dispatcher.clone())
        .with_yolo(yolo);

    let mut in_reasoning = false;
    let mut stdout = std::io::stdout();

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

    let loop_runner = if let Some(ref m) = meowstro {
        m.agent_loop()
    } else {
        &agent_loop
    };

    loop_runner
        .run_turn(
            &prompt,
            Some(&system_prompt),
            &tool_context,
            |chunk, is_reasoning| {
                if as_json {
                    let ev = serde_json::json!({
                        "type": if is_reasoning { "reasoning" } else { "text" },
                        "delta": chunk
                    });
                    println!("{}", ev);
                } else {
                    if is_reasoning {
                        if !in_reasoning {
                            print!("\n[Thinking: ");
                            in_reasoning = true;
                        }
                        print!("{}", chunk);
                    } else {
                        if in_reasoning {
                            print!("]\n\n");
                            in_reasoning = false;
                        }
                        print!("{}", chunk);
                    }
                    let _ = stdout.flush();
                }
            },
        )
        .await?;

    if in_reasoning {
        println!("]");
        let _ = stdout.flush();
    }

    if !as_json {
        println!();
    }

    plugin_dispatcher.dispatch_session_stop(&session_id).await;
    Ok(())
}

fn run_doctor(config: &Config, workspace_root: &std::path::Path) {
    println!("🐾 PurrCode AI Diagnostic (Doctor)");
    println!("--------------------------------------------------");
    println!("Preset: {}", config.preset);
    println!("Model Preset: {:?}", config.model_preset);
    println!("Default Model Override: {:?}", config.model);
    println!(
        "Mode Presets Discovered: {}",
        ModePresetLoader::discover_all(workspace_root).len()
    );
    println!(
        "Model Presets Discovered: {}",
        ModelPresetLoader::discover_all(workspace_root).len()
    );
    println!();

    if config.providers.is_empty() {
        println!("⚠️  No providers configured yet.");
        println!(
            "   Create '~/.purrcode/purrcode.jsonc' or run 'purrcode-ai onboard' to configure your endpoint."
        );
    } else {
        // `doctor` reports problems instead of failing on them: a provider
        // whose HTTP client cannot be built is diagnosed here, not hidden.
        let reg = match ProviderRegistry::from_config(config) {
            Ok(r) => r,
            Err(e) => {
                println!("⚠️  Failed to build provider clients: {e}");
                ProviderRegistry::new()
            }
        };
        println!("Configured Providers ({}):", config.providers.len());
        for (name, p) in &config.providers {
            let eff_url = p.base_url.as_deref().unwrap_or("(none)");
            let healthy = reg.get(name).map(|prov| prov.is_healthy()).unwrap_or(false);
            println!("  • [{}] (healthy: {})", name, healthy);
            println!("    Base URL: {}", eff_url);
            println!("    API Type: {}", p.api);
            println!("    Models: {} configured", p.models.len());
            for (m_id, m_cfg) in &p.models {
                let status = if m_cfg.context_window >= MIN_CONTEXT_WINDOW {
                    "OK (>= 64K)"
                } else {
                    "INVALID (< 64K)"
                };
                println!(
                    "      - {} (context: {}k [{}], reasoning: {})",
                    m_id,
                    m_cfg.context_window / 1024,
                    status,
                    m_cfg.reasoning
                );
            }
        }
    }
    println!("--------------------------------------------------");
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_config::{Config, ModelConfig, ProviderConfig};
    use std::collections::HashMap;

    #[test]
    fn test_resolve_target_model_deterministic_priority() {
        let mut config = Config::default();
        let mut p_cfg = ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: None,
            api_key: None,
            headers: HashMap::new(),
            user_agent: None,
            models: HashMap::new(),
        };
        p_cfg.models.insert(
            "secondary-coder".to_string(),
            ModelConfig {
                name: Some("Secondary Coder".to_string()),
                ..Default::default()
            },
        );
        p_cfg.models.insert(
            "primary-model".to_string(),
            ModelConfig {
                name: Some("Primary Model".to_string()),
                ..Default::default()
            },
        );
        config.providers.insert("custom".to_string(), p_cfg);

        // Run multiple times - must deterministically select "custom:primary-model"
        for _ in 0..10 {
            let model = resolve_target_model_or_exit(None, &config);
            assert_eq!(model, "custom:primary-model");
        }
    }

    #[test]
    fn test_resolve_target_model_deterministic_lexicographical_fallback() {
        let mut config = Config::default();
        let mut p1 = ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: None,
            api_key: None,
            headers: HashMap::new(),
            user_agent: None,
            models: HashMap::new(),
        };
        p1.models.insert(
            "zebra-coder".to_string(),
            ModelConfig {
                name: None,
                ..Default::default()
            },
        );
        p1.models.insert(
            "alpha-coder".to_string(),
            ModelConfig {
                name: None,
                ..Default::default()
            },
        );
        config.providers.insert("my-provider".to_string(), p1);

        for _ in 0..10 {
            let model = resolve_target_model_or_exit(None, &config);
            assert_eq!(model, "my-provider:alpha-coder");
        }
    }

    #[tokio::test]
    async fn test_cli_approval_requester_denies_without_tty() {
        use purrcode_core::ApprovalRequester;
        // Under `cargo test` stdin is not a TTY, so both variants deny
        // (the `yolo` variant only changes the hint, never the decision).
        assert!(
            !CliApprovalRequester { yolo: false }
                .request_approval("write", "Run tool 'write'", None)
                .await
        );
        assert!(
            !CliApprovalRequester { yolo: true }
                .request_approval("write", "Run tool 'write'", None)
                .await
        );
    }
}
