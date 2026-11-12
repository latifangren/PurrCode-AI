// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Read;
use std::path::Path;

use purrcode_api::error::{PurrError, Result};
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::SessionStore;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_service::ServiceInstaller;

use crate::cli::{Cli, Commands};
use crate::gateway;
use crate::onboard;
use crate::one_shot::{confirm_god_mode_if_needed, run_one_shot};

const MIN_CONTEXT_WINDOW: u64 = 64 * 1024;

pub fn run_doctor(config: &Config, workspace_root: &Path) {
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
        let reg = ProviderRegistry::from_config(config);
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
    println!();
    println!("Workspace root: {}", workspace_root.display());
    println!(
        "Skills directory: {}",
        workspace_root.join(".purrcode").join("skills").display()
    );
    println!("System diagnostics complete.");
}

pub async fn handle_subcommand(
    command: Commands,
    args: &Cli,
    config: &Config,
    purrcode_dir: &Path,
    workspace_root: &Path,
) -> Result<()> {
    match command {
        Commands::Onboard => onboard::run_interactive_onboard().await,
        Commands::Doctor => {
            run_doctor(config, workspace_root);
            Ok(())
        }
        Commands::Preset { action, name } => {
            let presets = ModePresetLoader::discover_all(workspace_root);
            if action == "show" {
                let target = name.as_deref().unwrap_or(&config.preset);
                let p = ModePresetLoader::load_by_name(target, workspace_root)?;
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
            Ok(())
        }
        Commands::ModelPreset { action, name } => {
            let presets = ModelPresetLoader::discover_all(workspace_root);
            if action == "show" {
                let target = name
                    .as_deref()
                    .or(config.model_preset.as_deref())
                    .unwrap_or("free");
                let p = ModelPresetLoader::load_by_name(target, workspace_root)?;
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
            Ok(())
        }
        Commands::Session { action, id, format } => {
            let store = SessionStore::new(purrcode_dir);
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
            Ok(())
        }
        Commands::Plugin { action: _ } => {
            let mut pm = PluginManager::new();
            pm.discover_all(workspace_root);
            println!("🐾 Discovered Plugins ({}):", pm.plugins().len());
            for p in pm.plugins() {
                let m = p.manifest();
                println!("  - {} v{} ({:?})", m.name, m.version, m.transport);
            }
            Ok(())
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
            Ok(())
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
                args.model.clone(),
                args.json,
                args.orchestrator,
                args.yolo,
                config,
                workspace_root,
            )
            .await
        }
        Commands::Cron {
            action,
            id_or_schedule,
            prompt,
            tools,
        } => {
            let cron_file = purrcode_dir.join("cron_jobs.json");
            let scheduler = purrcode_service::CronScheduler::new(&cron_file);
            match action.as_str() {
                "list" => {
                    let jobs = scheduler.list_jobs().await;
                    if jobs.is_empty() {
                        println!("🐾 No scheduled cron jobs found in {}", cron_file.display());
                    } else {
                        println!("🐾 Scheduled Cron Jobs ({}):", jobs.len());
                        for j in jobs {
                            let next = j
                                .next_run
                                .map(|t| t.to_rfc3339())
                                .unwrap_or_else(|| "none".to_string());
                            let allowed_str = j
                                .allowed_tools
                                .as_ref()
                                .map(|v| v.join(","))
                                .unwrap_or_else(|| "read-only (default)".to_string());
                            println!(
                                "  - [{}] schedule: \"{}\", next: {}, tools: [{}], prompt: \"{}\"",
                                j.id, j.schedule, next, allowed_str, j.prompt
                            );
                        }
                    }
                }
                "add" => {
                    let sched = id_or_schedule.ok_or_else(|| {
                        purrcode_api::error::PurrError::Config(
                            "Usage: purrcode-ai cron add <schedule> <prompt> [--tools tool1,tool2]"
                                .to_string(),
                        )
                    })?;
                    let p = prompt.ok_or_else(|| {
                        purrcode_api::error::PurrError::Config(
                            "Usage: purrcode-ai cron add <schedule> <prompt> [--tools tool1,tool2]"
                                .to_string(),
                        )
                    })?;
                    let allowed = tools.map(|t| {
                        t.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    });
                    let job = purrcode_service::CronJob {
                        id: format!("cron_{}", &uuid::Uuid::new_v4().to_string()[..8]),
                        schedule: sched.clone(),
                        prompt: p,
                        role: None,
                        model: None,
                        allowed_tools: allowed,
                        enabled: true,
                        next_run: None,
                        last_run: None,
                    };
                    scheduler.add_job(job.clone()).await?;
                    println!("🐾 Added cron job [{}] with schedule '{}'", job.id, sched);
                }
                "remove" | "rm" => {
                    let id = id_or_schedule.ok_or_else(|| {
                        purrcode_api::error::PurrError::Config(
                            "Usage: purrcode-ai cron remove <job_id>".to_string(),
                        )
                    })?;
                    let removed = scheduler.remove_job(&id).await?;
                    if removed {
                        println!("🐾 Removed cron job [{}]", id);
                    } else {
                        println!("⚠️  Job [{}] not found", id);
                    }
                }
                _ => {
                    println!("Usage: purrcode-ai cron [list|add|remove]");
                }
            }
            Ok(())
        }
        Commands::Curator { action, target } => {
            let curator = purrcode_service::CuratorEngine::new(purrcode_dir);
            match action.as_str() {
                "candidates" => {
                    let candidates = curator.list_candidates().await?;
                    if candidates.is_empty() {
                        println!(
                            "🐾 No pending candidate skills found in .purrcode/skills/candidates"
                        );
                    } else {
                        println!("🐾 Pending Candidate Skills ({}):", candidates.len());
                        for c in candidates {
                            println!(
                                "  - [{}] {} (path: {})",
                                c.name,
                                c.description,
                                c.path.display()
                            );
                        }
                        println!("\nTo approve: purrcode-ai curator approve <name>");
                        println!("To reject:  purrcode-ai curator reject <name>");
                    }
                }
                "approve" => {
                    let name = target.ok_or_else(|| {
                        purrcode_api::error::PurrError::Config(
                            "Usage: purrcode-ai curator approve <name>".to_string(),
                        )
                    })?;
                    let path = curator.approve_candidate(&name).await?;
                    println!(
                        "🐾 Approved candidate skill '{}' -> promoted to {}",
                        name,
                        path.display()
                    );
                }
                "reject" => {
                    let name = target.ok_or_else(|| {
                        purrcode_api::error::PurrError::Config(
                            "Usage: purrcode-ai curator reject <name>".to_string(),
                        )
                    })?;
                    curator.reject_candidate(&name).await?;
                    println!("🐾 Rejected candidate skill '{}'", name);
                }
                _ => {
                    println!("🐾 Evaluating skills with Curator engine...");
                    let reports = curator.evaluate_skills().await?;
                    if reports.is_empty() {
                        println!(
                            "  No installed project skills found to evaluate in .purrcode/skills"
                        );
                    } else {
                        println!("Skill Audit Reports ({}):", reports.len());
                        for r in reports {
                            let last = r
                                .last_used
                                .map(|t| t.to_rfc3339())
                                .unwrap_or_else(|| "never".to_string());
                            let act = r.action_taken.as_deref().unwrap_or("none");
                            println!(
                                "  - {:<18} status: {:<8} calls: {:<4} last_used: {:<20} action: {}",
                                r.skill_name,
                                format!("{:?}", r.status),
                                r.total_calls,
                                last,
                                act
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Web { host, port, token } => {
            confirm_god_mode_if_needed(&config.preset)?;
            println!("🐾 Starting PurrCode AI WebUI server (Milestone v0.4)...");
            let channel = std::sync::Arc::new(purrcode_channel::WebChannel::new());
            let dispatcher = std::sync::Arc::new(CliWebPromptDispatcher {
                config: config.clone(),
                workspace_root: workspace_root.to_path_buf(),
                orchestrator: args.orchestrator,
                yolo: args.yolo,
            });
            let server = purrcode_channel::WebServer::new(
                host,
                port,
                token,
                channel,
                purrcode_dir.to_path_buf(),
                workspace_root.to_path_buf(),
            )
            .with_dispatcher(dispatcher);
            let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            server.run(cancel_rx).await
        }
        Commands::Gateway => {
            confirm_god_mode_if_needed(&config.preset)?;
            gateway::run_gateway(config, workspace_root, args.orchestrator, args.yolo).await
        }
    }
}

pub struct CliWebPromptDispatcher {
    pub config: purrcode_config::Config,
    pub workspace_root: std::path::PathBuf,
    pub orchestrator: bool,
    pub yolo: bool,
}

#[async_trait::async_trait]
impl purrcode_channel::PromptDispatcher for CliWebPromptDispatcher {
    async fn dispatch(
        &self,
        prompt: String,
        channel: std::sync::Arc<purrcode_channel::WebChannel>,
    ) -> Result<()> {
        gateway::execute_channel_agent_turn(
            &self.config,
            &self.workspace_root,
            self.orchestrator,
            self.yolo,
            &prompt,
            channel as std::sync::Arc<dyn purrcode_api::channel::Channel>,
        )
        .await
        .map(|_| ())
    }
}
