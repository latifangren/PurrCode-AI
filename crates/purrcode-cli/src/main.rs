// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod commands;
mod gateway;
mod onboard;
mod one_shot;

use clap::Parser;
use cli::Cli;
pub use one_shot::resolve_target_model_or_exit;
use purrcode_api::error::Result;
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_channel::tui::AskOptionItem;
use purrcode_config::{ConfigLoader, ModePresetLoader, ModelPresetLoader};
use purrcode_core::{AgentLoop, SessionStore, SteeringQueue};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::{AskOption, AskRequester, ToolRegistry, ToolRegistryExt};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)]
    init_windows_console();

    let args = Cli::parse();

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
    if let Some(cmd) = args.command.clone() {
        commands::handle_subcommand(cmd, &args, &config, &purrcode_dir, &workspace_root).await?;
        return Ok(());
    }

    // 3. One-shot non-interactive mode (-p / --prompt)
    if let Some(prompt) = args.prompt {
        one_shot::confirm_god_mode_if_needed(&config.preset)?;
        one_shot::run_one_shot(
            prompt,
            args.model,
            args.json,
            args.orchestrator,
            args.yolo,
            &config,
            &workspace_root,
        )
        .await?;
        return Ok(());
    }

    // 4. Default interactive mode: launch Ratatui TUI connected to AgentLoop / Meowstro
    let target_model = resolve_target_model_or_exit(args.model, &config);

    let registry = ProviderRegistry::from_config(&config);
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
    }

    #[async_trait::async_trait]
    impl purrcode_core::ApprovalRequester for TuiApprovalRequester {
        async fn request_approval(
            &self,
            tool: &str,
            description: &str,
            preview: Option<&str>,
        ) -> bool {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let req = purrcode_channel::ApprovalRequest {
                tool_name: tool.to_string(),
                description: description.to_string(),
                diff_preview: preview.map(|p| p.to_string()),
                tx,
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

    let ask_requester: Arc<dyn AskRequester> = Arc::new(TuiAskRequester {
        event_tx: event_tx.clone(),
    });
    let approval_requester: Arc<dyn purrcode_core::ApprovalRequester> =
        Arc::new(TuiApprovalRequester {
            event_tx: event_tx.clone(),
        });

    let mode_preset = match ModePresetLoader::load_by_name(&config.preset, &workspace_root) {
        Ok(p) => p,
        Err(_) => ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?,
    };
    let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, args.yolo));
    let policy_gate: Arc<dyn PolicyGate> = approval_gate.clone();

    let audit_logger = Arc::new(AuditLogger::new(&purrcode_dir));
    let audit_sink: Arc<dyn AuditSink> = audit_logger.clone();

    let mut plugin_mgr = PluginManager::new();
    plugin_mgr.discover_all(&workspace_root);
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

    let session_store = Arc::new(SessionStore::new(&purrcode_dir));
    let model_preset_name = config
        .model_preset
        .clone()
        .unwrap_or_else(|| "free".to_string());
    let model_preset = ModelPresetLoader::load_by_name(&model_preset_name, &workspace_root).ok();

    if let (Ok(p_val), Ok(mp_val)) = (
        serde_json::to_value(&mode_preset),
        serde_json::to_value(&model_preset),
    ) {
        let _ = session_store.save_preset_snapshots(&session_id, &p_val, &mp_val);
    }

    let _ = audit_logger.log_entry(AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: session_id.clone(),
        agent: "meowstro".to_string(),
        event: "session_start".to_string(),
        tool: None,
        decision: "allow".to_string(),
        detail: format!("preset={} yolo={}", config.preset, args.yolo),
    });

    let registry_arc = Arc::new(registry);
    let session_dir = session_store.session_dir(&session_id);
    let meowstro = if args.orchestrator {
        Some(MeowstroOrchestrator::with_complete_options(
            provider.clone(),
            wire_model,
            &session_dir,
            Some(approval_requester.clone()),
            Some(ask_requester),
            Some(approval_gate),
            Some(audit_sink.clone()),
            Some(plugin_dispatcher.clone()),
            model_preset,
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

    let direct_loop = AgentLoop::new(provider, wire_model, Arc::new(tool_registry))
        .with_approval_requester(approval_requester)
        .with_policy_gate(policy_gate)
        .with_audit_sink(audit_sink)
        .with_plugin_dispatcher(plugin_dispatcher.clone())
        .with_steering_queue(steering_queue)
        .with_cancel_flag(cancel_flag.clone())
        .with_yolo(args.yolo);

    let tool_context = ToolContext {
        session_id: session_id.clone(),
        agent_role: "meowstro".to_string(),
        workspace_root: workspace_root.clone(),
        is_dry_run: false,
    };

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(&workspace_root);
    let skills_index = skill_mgr.format_prompt_index();

    let memory = MarkdownMemory::new(&purrcode_dir);
    let mem_context = memory.load_context(Some(&workspace_root)).await;

    let mut system_prompt = if args.orchestrator {
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
            models: HashMap::new(),
        };
        p1.models.insert(
            "beta-model".to_string(),
            ModelConfig {
                name: Some("Beta Model".to_string()),
                ..Default::default()
            },
        );
        config.providers.insert("provider-b".to_string(), p1);

        let mut p2 = ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: None,
            api_key: None,
            headers: HashMap::new(),
            models: HashMap::new(),
        };
        p2.models.insert(
            "alpha-model".to_string(),
            ModelConfig {
                name: Some("Alpha Model".to_string()),
                ..Default::default()
            },
        );
        config.providers.insert("provider-a".to_string(), p2);

        // Run multiple times - must deterministically select "provider-a:alpha-model"
        for _ in 0..10 {
            let model = resolve_target_model_or_exit(None, &config);
            assert_eq!(model, "provider-a:alpha-model");
        }
    }
}
