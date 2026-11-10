mod cli;
mod onboard;

use clap::Parser;
use cli::{Cli, Commands};
use purrcode_api::error::Result;
use purrcode_config::{Config, ConfigLoader};
use purrcode_provider::ProviderRegistry;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    // 1. Load configuration
    let config = match &args.config {
        Some(path) => ConfigLoader::load_from_file(path)?,
        None => ConfigLoader::load_effective()?,
    };

    // 2. Dispatch subcommands if present
    if let Some(cmd) = args.command {
        match cmd {
            Commands::Onboard => {
                onboard::run_interactive_onboard()?;
                return Ok(());
            }
            Commands::Doctor => {
                run_doctor(&config);
                return Ok(());
            }
            Commands::Preset { action } => {
                println!("🐾 Presets (action: {}):", action);
                println!("  - carrier (strict): Manual approval, strict guardrail, confinement");
                println!("  - tabby   (default): Supervised approval, default guardrail, confinement");
                println!("  - oyen    (god): Supervised approval, god mode (guardrails off)");
                return Ok(());
            }
            Commands::ModelPreset { action } => {
                println!("🐾 Model Presets (action: {}):", action);
                println!("  - custom-endpoint: Local proxy / router (Magpie, OmniRoute)");
                println!("  - free: Free-first (Ollama + OpenRouter free tier)");
                println!("  - local: Offline Ollama");
                println!("  - premium: Flagship models with BYO key");
                return Ok(());
            }
        }
    }

    // 3. One-shot prompt mode (-p / --prompt)
    if let Some(prompt) = args.prompt {
        run_one_shot(prompt, args.model, args.json, args.orchestrator, args.yolo, &config).await?;
        return Ok(());
    }

    // 4. Default interactive mode: launch Ratatui TUI connected to AgentLoop
    let target_model = match args.model.or_else(|| config.model.clone()) {
        Some(m) => m,
        None => {
            let mut found = None;
            for (p_name, p_cfg) in &config.providers {
                if let Some(m_key) = p_cfg.models.keys().next() {
                    found = Some(format!("{}:{}", p_name, m_key));
                    break;
                }
            }
            match found {
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
    };

    let registry = ProviderRegistry::from_config(&config);
    let (provider_name, wire_model, _provider_cfg, _model_cfg) = match config.resolve_model(&target_model) {
        Some(resolved) => resolved,
        None => {
            eprintln!("Error: Model '{}' could not be resolved from configuration.", target_model);
            std::process::exit(1);
        }
    };

    let provider = match registry.get(provider_name) {
        Some(p) => p,
        None => {
            eprintln!("Error: Provider '{}' has no base_url or client configured.", provider_name);
            std::process::exit(1);
        }
    };

    let mut tool_registry = ToolRegistry::with_defaults();
    let mut mcp_manager = McpManager::new();
    let _ = mcp_manager.load_servers(&config.mcp_servers, &mut tool_registry).await;

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<purrcode_channel::TuiEvent>(64);
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    struct TuiApprovalRequester {
        event_tx: tokio::sync::mpsc::Sender<purrcode_channel::TuiEvent>,
    }

    #[async_trait::async_trait]
    impl purrcode_core::ApprovalRequester for TuiApprovalRequester {
        async fn request_approval(&self, tool: &str, description: &str, preview: Option<&str>) -> bool {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let req = purrcode_channel::ApprovalRequest {
                tool_name: tool.to_string(),
                description: description.to_string(),
                diff_preview: preview.map(|p| p.to_string()),
                tx,
            };
            if self.event_tx.send(purrcode_channel::TuiEvent::ApprovalRequested(req)).await.is_err() {
                return false;
            }
            rx.await.unwrap_or(false)
        }
    }

    let approval_requester = Arc::new(TuiApprovalRequester {
        event_tx: event_tx.clone(),
    });

    let agent_loop = AgentLoop::new(provider.clone(), wire_model, Arc::new(tool_registry))
        .with_approval_requester(approval_requester);

    let session_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);

    let tool_context = ToolContext {
        session_id: session_id.clone(),
        agent_role: "meowstro".to_string(),
        workspace_root: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        is_dry_run: false,
    };

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(&tool_context.workspace_root);
    let skills_index = skill_mgr.format_prompt_index();

    let mut system_prompt = String::from("You are PurrCode AI, a modular and highly capable AI assistant. Answer user queries accurately. When you need information about files or want to execute commands, call the appropriate tools.");
    if !skills_index.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&skills_index);
    }

    struct TuiEventListener {
        event_tx: tokio::sync::mpsc::Sender<purrcode_channel::TuiEvent>,
    }

    impl purrcode_core::AgentEventListener for TuiEventListener {
        fn on_text_chunk(&self, chunk: &str, is_reasoning: bool) {
            if is_reasoning {
                let _ = self.event_tx.try_send(purrcode_channel::TuiEvent::ReasoningChunk(chunk.to_string()));
            } else {
                let _ = self.event_tx.try_send(purrcode_channel::TuiEvent::TextChunk(chunk.to_string()));
            }
        }

        fn on_tool_call_started(&self, name: &str, args: &str) {
            let _ = self.event_tx.try_send(purrcode_channel::TuiEvent::ToolCallStarted {
                name: name.to_string(),
                args: args.to_string(),
            });
        }

        fn on_tool_call_finished(&self, name: &str, output: &str, success: bool) {
            let _ = self.event_tx.try_send(purrcode_channel::TuiEvent::ToolCallFinished {
                name: name.to_string(),
                output: output.to_string(),
                success,
            });
        }
    }

    let listener: Arc<dyn purrcode_core::AgentEventListener> = Arc::new(TuiEventListener {
        event_tx: event_tx.clone(),
    });

    // Multi-turn message history persistent across turns in TUI
    let mut session_messages: Vec<purrcode_api::provider::ChatMessage> = Vec::new();

    // Spawn background worker for TUI interactive prompts
    let event_tx_worker = event_tx.clone();
    tokio::spawn(async move {
        while let Some(user_prompt) = prompt_rx.recv().await {
            let _ = agent_loop
                .run_turn_with_history(
                    &mut session_messages,
                    &user_prompt,
                    Some(&system_prompt),
                    &tool_context,
                    Some(listener.as_ref()),
                )
                .await;
            let _ = event_tx_worker.send(purrcode_channel::TuiEvent::TurnFinished).await;
        }
    });

    let app = purrcode_channel::TuiApp::new(target_model, config.preset.clone(), session_id);
    purrcode_channel::TuiRunner::run(app, Some(event_rx), Some(prompt_tx))?;
    Ok(())
}

use purrcode_api::tool::ToolContext;
use purrcode_core::AgentLoop;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::ToolRegistry;
use std::sync::Arc;

use purrcode_orchestrator::MeowstroOrchestrator;

struct CliApprovalRequester;

#[async_trait::async_trait]
impl purrcode_core::ApprovalRequester for CliApprovalRequester {
    async fn request_approval(&self, tool: &str, description: &str, preview: Option<&str>) -> bool {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            eprintln!("⚠️  [Approval Denied]: Tool '{}' requires approval. Run in terminal or pass --yolo.", tool);
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
) -> Result<()> {
    // Determine target model
    let target_model = match model_override.or_else(|| config.model.clone()) {
        Some(m) => m,
        None => {
            let mut found_model = None;
            for (p_name, p_cfg) in &config.providers {
                if let Some(m_key) = p_cfg.models.keys().next() {
                    found_model = Some(format!("{}:{}", p_name, m_key));
                    break;
                }
            }

            match found_model {
                Some(m) => m,
                None => {
                    eprintln!("Error: No model configured.");
                    eprintln!("PurrCode AI has no hardcoded default models.");
                    eprintln!();
                    eprintln!("Please configure your provider in '~/.purrcode/purrcode.jsonc' or 'purrcode.jsonc'.");
                    eprintln!("Example configuration is available in 'purrcode.jsonc.example'.");
                    std::process::exit(1);
                }
            }
        }
    };

    let registry = ProviderRegistry::from_config(config);

    // Resolve provider and model configuration
    let (provider_name, wire_model, _provider_cfg, _model_cfg) = match config.resolve_model(&target_model) {
        Some(resolved) => resolved,
        None => {
            eprintln!("Error: Model '{}' could not be resolved from configuration.", target_model);
            eprintln!("Available providers: {:?}", config.providers.keys().collect::<Vec<_>>());
            std::process::exit(1);
        }
    };

    let provider = match registry.get(provider_name) {
        Some(p) => p,
        None => {
            eprintln!("Error: Provider '{}' has no base_url or client configured.", provider_name);
            std::process::exit(1);
        }
    };

    // Initialize tools & MCP servers
    let mut tool_registry = ToolRegistry::with_defaults();
    let mut mcp_manager = McpManager::new();
    let _ = mcp_manager.load_servers(&config.mcp_servers, &mut tool_registry).await;

    let tool_context = ToolContext {
        session_id: "one_shot".to_string(),
        agent_role: "meowstro".to_string(),
        workspace_root: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        is_dry_run: false,
    };

    let session_dir = std::env::temp_dir().join("purrcode_session");
    let meowstro = if use_orchestrator {
        Some(MeowstroOrchestrator::new(provider.clone(), wire_model, &session_dir))
    } else {
        None
    };

    let agent_loop = AgentLoop::new(provider, wire_model, Arc::new(tool_registry))
        .with_approval_requester(Arc::new(CliApprovalRequester))
        .with_yolo(yolo);

    let mut in_reasoning = false;
    let mut stdout = std::io::stdout();

    let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
    skill_mgr.discover_all(&tool_context.workspace_root);
    let skills_index = skill_mgr.format_prompt_index();

    let system_prompt = if use_orchestrator {
        MeowstroOrchestrator::system_prompt(&skills_index)
    } else {
        let mut base = String::from("You are PurrCode AI, a modular and highly capable AI assistant. Answer user queries accurately. When you need information about files or want to execute commands, call the appropriate tools.");
        if !skills_index.is_empty() {
            base.push_str("\n\n");
            base.push_str(&skills_index);
        }
        base
    };

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

    Ok(())
}

fn run_doctor(config: &Config) {
    println!("🐾 PurrCode AI Diagnostic (Doctor)");
    println!("--------------------------------------------------");
    println!("Preset: {}", config.preset);
    println!("Model Preset: {:?}", config.model_preset);
    println!("Default Model Override: {:?}", config.model);
    println!();

    if config.providers.is_empty() {
        println!("⚠️  No providers configured yet.");
        println!("   Create '~/.purrcode/purrcode.jsonc' to configure your custom endpoint or Ollama.");
    } else {
        println!("Configured Providers ({}) :", config.providers.len());
        for (name, p) in &config.providers {
            println!("  • [{}]", name);
            println!("    Base URL: {}", p.base_url.as_deref().unwrap_or("(none)"));
            println!("    API Type: {}", p.api);
            println!("    Models: {} configured", p.models.len());
            for (m_id, m_cfg) in &p.models {
                println!(
                    "      - {} (context: {}k, reasoning: {})",
                    m_id,
                    m_cfg.context_window / 1024,
                    m_cfg.reasoning
                );
            }
        }
    }
    println!("--------------------------------------------------");
}
