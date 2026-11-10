mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use purrcode_api::error::Result;
use purrcode_api::provider::{ChatMessage, CompletionRequest, MessageRole};
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
        run_one_shot(prompt, args.model, args.json, &config).await?;
        return Ok(());
    }

    // 4. Default interactive greeting (TUI placeholder for Sprint 3)
    println!("🐾 PurrCode AI (purrcode-ai) v0.1.0");
    println!("Modular AI Agent — Orchestrator & Subagents with Zero Hardcode");
    println!();
    println!("Quick start:");
    println!("  purrcode-ai -p \"<prompt>\"       Execute one-shot prompt");
    println!("  purrcode-ai doctor              Diagnose providers & config");
    println!("  purrcode-ai --help              Show all options");
    Ok(())
}

async fn run_one_shot(
    prompt: String,
    model_override: Option<String>,
    as_json: bool,
    config: &Config,
) -> Result<()> {
    // Determine target model
    let target_model = match model_override.or_else(|| config.model.clone()) {
        Some(m) => m,
        None => {
            // Pick first configured model across all providers if any
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
    let (provider_name, wire_model, _provider_cfg, model_cfg) = match config.resolve_model(&target_model) {
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

    let request = CompletionRequest {
        model: wire_model.to_string(),
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: Some(0.3),
        max_tokens: model_cfg.max_tokens.map(|t| t as u32),
        reasoning_effort: model_cfg.reasoning_effort.clone(),
        stop: None,
    };

    let mut stream = provider.stream_completion(request).await?;

    let mut stdout = std::io::stdout();
    let mut in_reasoning = false;

    while let Some(chunk) = stream.recv().await {
        if as_json {
            let json_str = serde_json::to_string(&chunk).unwrap_or_default();
            println!("{}", json_str);
            let _ = stdout.flush();
        } else {
            // Handle reasoning delta if present
            if let Some(thought) = chunk.reasoning_delta {
                if !in_reasoning {
                    print!("\n[Thinking: ");
                    in_reasoning = true;
                }
                print!("{}", thought);
                let _ = stdout.flush();
            }

            // Handle main text delta
            if !chunk.delta.is_empty() {
                if in_reasoning {
                    print!("]\n\n");
                    in_reasoning = false;
                }
                print!("{}", chunk.delta);
                let _ = stdout.flush();
            }
        }

        if chunk.is_finished {
            break;
        }
    }

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
