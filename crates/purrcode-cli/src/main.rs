// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod cron_cli;
mod gateway;
mod heartbeat_cli;
mod onboard;
mod one_shot;
mod tui_mode;
mod web_cli;
mod web_cron_registry;
mod web_skill_registry;

use clap::Parser;
use cli::{Cli, Commands};
use purrcode_api::error::{PurrError, Result};
use purrcode_config::{
    Config, ConfigLoader, ModePresetLoader, ModelPresetLoader, MIN_CONTEXT_WINDOW,
};
use purrcode_core::SessionStore;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_service::ServiceInstaller;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

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
                one_shot::run_one_shot(
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
            Commands::Web { host, port, token } => {
                confirm_god_mode_if_needed(&config.preset)?;
                web_cli::run_web_command(
                    &config,
                    &workspace_root,
                    use_orchestrator,
                    args.yolo,
                    args.model,
                    host,
                    port,
                    token,
                )
                .await?;
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
                cron_cli::run_cron_command(cron_cli::CronArgs {
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
            Commands::Heartbeat { action } => {
                heartbeat_cli::run_heartbeat_command(
                    &config,
                    heartbeat_cli::HeartbeatArgs { action },
                )
                .await?;
                return Ok(());
            }
        }
    }

    confirm_god_mode_if_needed(&config.preset)?;

    // 3. One-shot prompt mode (-p / --prompt)
    if let Some(prompt) = args.prompt {
        one_shot::run_one_shot(
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
    tui_mode::run_tui(
        &config,
        args.model,
        args.yolo,
        use_orchestrator,
        workspace_root,
        purrcode_dir,
    )
    .await
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
