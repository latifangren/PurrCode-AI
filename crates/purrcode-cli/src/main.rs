mod cli;

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
                println!("🐾 PurrCode AI — Onboarding Wizard");
                println!();
                println!("PurrCode AI requires a configured provider/model before running.");
                println!("Create your configuration at ~/.purrcode/purrcode.jsonc");
                println!("See 'purrcode.jsonc.example' for templates (Magpie, OmniRoute, Ollama, OpenRouter).");
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
        run_one_shot(prompt, args.model, args.json, args.orchestrator, &config).await?;
        return Ok(());
    }

    // 4. Default interactive mode: launch Ratatui TUI
    let target_model = args
        .model
        .or_else(|| config.model.clone())
        .unwrap_or_else(|| {
            for (p_name, p_cfg) in &config.providers {
                if let Some(m_key) = p_cfg.models.keys().next() {
                    return format!("{}:{}", p_name, m_key);
                }
            }
            "unconfigured".to_string()
        });

    let session_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let app = purrcode_channel::TuiApp::new(target_model, config.preset.clone(), session_id);
    purrcode_channel::TuiRunner::run(app)?;
    Ok(())
}

use purrcode_api::tool::ToolContext;
use purrcode_core::AgentLoop;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::ToolRegistry;
use std::sync::Arc;

use purrcode_orchestrator::MeowstroOrchestrator;

async fn run_one_shot(
    prompt: String,
    model_override: Option<String>,
    as_json: bool,
    use_orchestrator: bool,
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

    let agent_loop = AgentLoop::new(provider, wire_model, Arc::new(tool_registry));

    let mut in_reasoning = false;
    let mut stdout = std::io::stdout();

    let system_prompt = if use_orchestrator {
        MeowstroOrchestrator::system_prompt()
    } else {
        "You are PurrCode AI, a modular and highly capable AI assistant. Answer user queries accurately. When you need information about files or want to execute commands, call the appropriate tools."
    };

    let loop_runner = if let Some(ref m) = meowstro {
        m.agent_loop()
    } else {
        &agent_loop
    };

    loop_runner
        .run_turn(
            &prompt,
            Some(system_prompt),
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
        print!("]\n");
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
