// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::channel::Channel;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::provider::MessageRole;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditLogger};
use purrcode_channel::telegram::{TelegramChannel, TelegramMessageHandler};
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::{AgentLoop, ApprovalRequester};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_service::cron::{CronJob, CronJobDispatcher, CronScheduler};
use purrcode_service::curator::CuratorEngine;
use purrcode_tools::skill::SkillManager;
use purrcode_tools::{ToolRegistry, ToolRegistryExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

struct TelegramApprovalRequester {
    channel: Arc<TelegramChannel>,
}

#[async_trait::async_trait]
impl ApprovalRequester for TelegramApprovalRequester {
    async fn request_approval(&self, tool: &str, description: &str, preview: Option<&str>) -> bool {
        let call_id = uuid::Uuid::new_v4().to_string();
        self.channel
            .request_approval(&call_id, tool, description, preview)
            .await
            .unwrap_or(false)
    }
}

struct TelegramEventListener {
    channel: Arc<TelegramChannel>,
}

impl purrcode_core::AgentEventListener for TelegramEventListener {
    fn on_text_chunk(&self, chunk: &str, is_reasoning: bool) {
        let ch = self.channel.clone();
        let ev = if is_reasoning {
            purrcode_api::event::AgentEvent::ReasoningChunk {
                thinking: chunk.to_string(),
            }
        } else {
            purrcode_api::event::AgentEvent::TextChunk {
                text: chunk.to_string(),
            }
        };
        tokio::spawn(async move {
            let _ = ch.send_event(&ev).await;
        });
    }

    fn on_tool_call_started(&self, name: &str, args: &str) {
        let ch = self.channel.clone();
        let parsed_args =
            serde_json::from_str(args).unwrap_or_else(|_| serde_json::json!({ "raw": args }));
        let ev = purrcode_api::event::AgentEvent::ToolCallStarted {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            arguments: parsed_args,
        };
        tokio::spawn(async move {
            let _ = ch.send_event(&ev).await;
        });
    }

    fn on_tool_call_finished(&self, name: &str, output: &str, success: bool) {
        let ch = self.channel.clone();
        let ev = purrcode_api::event::AgentEvent::ToolCallFinished {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            result: serde_json::json!({ "output": output }),
            success,
        };
        tokio::spawn(async move {
            let _ = ch.send_event(&ev).await;
        });
    }
}

struct GatewayCronDispatcher;

#[async_trait::async_trait]
impl CronJobDispatcher for GatewayCronDispatcher {
    async fn dispatch(&self, job: &CronJob) -> Result<String> {
        println!(
            "⏰ [Cron] Executing scheduled job '{}': {}",
            job.id, job.prompt
        );
        Ok(format!("Job {} completed", job.id))
    }
}

struct GatewayTelegramHandler {
    config: Config,
    workspace_root: PathBuf,
    channel: Arc<TelegramChannel>,
    use_orchestrator: bool,
    yolo: bool,
    start_time: Instant,
    scheduler: Arc<CronScheduler>,
}

#[async_trait::async_trait]
impl TelegramMessageHandler for GatewayTelegramHandler {
    async fn handle_command(&self, _chat_id: i64, command: &str, _args: &str) -> Result<String> {
        match command {
            "/start" | "/help" => {
                let help = "🐾 *PurrCode AI Gateway Help*\n\n\
                *Commands:*\n\
                /status - Display gateway status, active preset, model, and uptime\n\
                /cron   - List scheduled cron jobs\n\
                /skills - List loaded skills\n\
                /help   - Show this help message\n\n\
                To run a task, just send any natural language prompt!";
                Ok(help.to_string())
            }
            "/status" => {
                let uptime_secs = self.start_time.elapsed().as_secs();
                let hrs = uptime_secs / 3600;
                let mins = (uptime_secs % 3600) / 60;
                let secs = uptime_secs % 60;

                let jobs = self.scheduler.list_jobs().await;
                let job_count = jobs.len();

                let status = format!(
                    "🐾 *PurrCode AI Gateway Status*\n\n\
                    • *Preset:* `{}`\n\
                    • *Model Preset:* `{}`\n\
                    • *Target Model:* `{}`\n\
                    • *Uptime:* {}h {}m {}s\n\
                    • *Active Cron Jobs:* {}\n\
                    • *Workspace:* `{}`",
                    self.config.preset,
                    self.config.model_preset.as_deref().unwrap_or("free"),
                    self.config.model.as_deref().unwrap_or("default"),
                    hrs,
                    mins,
                    secs,
                    job_count,
                    self.workspace_root.display()
                );
                Ok(status)
            }
            "/cron" => {
                let jobs = self.scheduler.list_jobs().await;
                if jobs.is_empty() {
                    return Ok("📅 No active cron jobs scheduled.".to_string());
                }
                let mut out = "📅 *Scheduled Cron Jobs:*\n\n".to_string();
                for job in jobs {
                    let next = job
                        .next_run
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_else(|| "none".to_string());
                    out.push_str(&format!(
                        "• `{}`: `{}` (next: {})\n  Prompt: {}\n",
                        job.id, job.schedule, next, job.prompt
                    ));
                }
                Ok(out)
            }
            "/skills" => {
                let mut skill_mgr = SkillManager::new();
                skill_mgr.discover_all(&self.workspace_root);
                let list = skill_mgr.list();
                if list.is_empty() {
                    return Ok("🤹 No custom skills found in workspace.".to_string());
                }
                let mut out = "🤹 *Available Skills:*\n\n".to_string();
                for skill in list {
                    out.push_str(&format!("• `{}`: {}\n", skill.name, skill.description));
                }
                Ok(out)
            }
            _ => Ok(format!(
                "Unknown command `{}`. Send /help for available commands.",
                command
            )),
        }
    }

    async fn handle_message(
        &self,
        _chat_id: i64,
        _user_id: i64,
        _username: Option<&str>,
        prompt: &str,
    ) -> Result<String> {
        let target_model = self
            .config
            .model
            .clone()
            .unwrap_or_else(|| "default".to_string());

        let registry = ProviderRegistry::from_config(&self.config);
        let (provider_name, wire_model, _provider_cfg, _model_cfg) =
            match self.config.resolve_model(&target_model) {
                Some(resolved) => resolved,
                None => {
                    return Ok(format!(
                        "⚠️ Model '{}' could not be resolved from configuration.",
                        target_model
                    ));
                }
            };

        let provider = match registry.get(provider_name) {
            Some(p) => p,
            None => {
                return Ok(format!(
                    "⚠️ Provider '{}' has no base_url or client configured.",
                    provider_name
                ));
            }
        };

        let purrcode_dir =
            crate::onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));
        let mode_preset =
            match ModePresetLoader::load_by_name(&self.config.preset, &self.workspace_root) {
                Ok(p) => p,
                Err(_) => {
                    ModePresetLoader::parse_str(include_str!("../../../presets/tabby.jsonc"))?
                }
            };

        let approval_gate = Arc::new(ApprovalGate::from_preset(&mode_preset, self.yolo));
        let audit_logger = Arc::new(AuditLogger::new(&purrcode_dir));
        let audit_sink: Arc<dyn AuditSink> = audit_logger.clone();
        let policy_gate: Arc<dyn PolicyGate> = approval_gate.clone();

        let mut plugin_mgr = PluginManager::new();
        plugin_mgr.discover_all(&self.workspace_root);
        let plugin_dispatcher: Arc<dyn PluginHookDispatcher> = Arc::new(plugin_mgr);

        let model_preset_name = self
            .config
            .model_preset
            .clone()
            .unwrap_or_else(|| "free".to_string());
        let model_preset =
            ModelPresetLoader::load_by_name(&model_preset_name, &self.workspace_root).ok();

        let mut tool_registry =
            ToolRegistry::with_defaults_and_options(None, Some(&self.workspace_root));
        if !mode_preset.tools.is_empty() {
            tool_registry.retain(|name| {
                name == "read_skill" || mode_preset.tools.iter().any(|allowed| allowed == name)
            });
        }

        let session_id = format!("purr_gw_{}", &uuid::Uuid::new_v4().to_string()[..6]);
        plugin_dispatcher.dispatch_session_start(&session_id).await;

        let approver: Arc<dyn ApprovalRequester> = Arc::new(TelegramApprovalRequester {
            channel: self.channel.clone(),
        });

        let session_dir = purrcode_dir.join("sessions").join(&session_id);
        let _ = tokio::fs::create_dir_all(&session_dir).await;

        let registry_arc = Arc::new(registry);
        let meowstro = if self.use_orchestrator {
            Some(MeowstroOrchestrator::with_complete_options(
                provider.clone(),
                wire_model,
                &session_dir,
                Some(approver.clone()),
                None,
                Some(approval_gate),
                Some(audit_sink.clone()),
                Some(plugin_dispatcher.clone()),
                model_preset,
                Some(self.config.clone()),
                Some(registry_arc),
                None,
                None,
                self.yolo,
                Some(self.workspace_root.clone()),
            ))
        } else {
            None
        };

        let agent_loop = AgentLoop::new(provider, wire_model, Arc::new(tool_registry))
            .with_approval_requester(approver)
            .with_policy_gate(policy_gate)
            .with_audit_sink(audit_sink)
            .with_plugin_dispatcher(plugin_dispatcher.clone())
            .with_yolo(self.yolo);

        let mut skill_mgr = SkillManager::new();
        skill_mgr.discover_all(&self.workspace_root);
        let skills_index = skill_mgr.format_prompt_index();

        let memory = MarkdownMemory::new(&purrcode_dir);
        let mem_context = memory.load_context(Some(&self.workspace_root)).await;

        let mut system_prompt = if self.use_orchestrator {
            MeowstroOrchestrator::system_prompt(&skills_index)
        } else {
            let mut base = String::from("You are PurrCode AI, a modular and highly capable AI assistant. Answer user queries accurately.");
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

        let tool_context = ToolContext {
            session_id: session_id.clone(),
            agent_role: "meowstro".to_string(),
            workspace_root: self.workspace_root.clone(),
            is_dry_run: false,
        };

        let mut messages = Vec::new();
        let listener = TelegramEventListener {
            channel: self.channel.clone(),
        };

        let result = loop_runner
            .run_turn_with_history(
                &mut messages,
                prompt,
                Some(&system_prompt),
                &tool_context,
                Some(&listener),
            )
            .await;

        plugin_dispatcher.dispatch_session_stop(&session_id).await;

        match result {
            Ok(answer) => {
                if answer.trim().is_empty() {
                    let last_text = messages
                        .iter()
                        .rev()
                        .find(|m| m.role == MessageRole::Assistant && !m.content.trim().is_empty())
                        .map(|m| m.content.clone())
                        .unwrap_or_else(|| "🐾 Task completed successfully.".to_string());
                    Ok(last_text)
                } else {
                    Ok(answer)
                }
            }
            Err(e) => Err(e),
        }
    }
}

pub async fn run_gateway(
    config: &Config,
    workspace_root: &Path,
    use_orchestrator: bool,
    yolo: bool,
) -> Result<()> {
    println!("🐾 PurrCode AI Gateway daemon starting...");
    println!("   Preset: {}", config.preset);
    println!("   Workspace root: {}", workspace_root.display());

    let purrcode_dir =
        crate::onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));
    let cron_path = purrcode_dir.join("cron_jobs.json");
    let scheduler = Arc::new(CronScheduler::new(cron_path));

    // 1. Run skill curator check
    let curator = CuratorEngine::new(&purrcode_dir);
    match curator.evaluate_skills().await {
        Ok(actions) => {
            if !actions.is_empty() {
                println!(
                    "🧹 Skill Curator performed {} maintenance actions.",
                    actions.len()
                );
            }
        }
        Err(e) => {
            tracing::warn!("Skill curator evaluation notice: {}", e);
        }
    }

    let (cancel_tx, cancel_rx) = watch::channel(false);

    // 2. Spawn 60s autonomous cron scheduler ticker
    let sched_ticker = scheduler.clone();
    let mut cron_cancel_rx = cancel_rx.clone();
    let dispatcher: Arc<dyn CronJobDispatcher> = Arc::new(GatewayCronDispatcher);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cron_cancel_rx.changed() => {
                    if *cron_cancel_rx.borrow() {
                        break;
                    }
                }
                _ = interval.tick() => {
                    let executed = sched_ticker.tick(&dispatcher).await;
                    if let Ok(res) = executed {
                        if !res.is_empty() {
                            println!("⏰ [Cron] Completed {} scheduled tasks.", res.len());
                        }
                    }
                }
            }
        }
    });

    // 3. Telegram Channel setup if token available
    let tg_token = config
        .telegram
        .as_ref()
        .and_then(|t| t.bot_token.clone())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            std::env::var("TELEGRAM_BOT_TOKEN")
                .ok()
                .filter(|t| !t.trim().is_empty())
        });

    if let Some(token) = tg_token {
        let allowed_users = config
            .telegram
            .as_ref()
            .map(|t| t.allowed_users.clone())
            .unwrap_or_default();

        let tg_channel = Arc::new(TelegramChannel::new(token, allowed_users, None));
        let handler = Arc::new(GatewayTelegramHandler {
            config: config.clone(),
            workspace_root: workspace_root.to_path_buf(),
            channel: tg_channel.clone(),
            use_orchestrator,
            yolo,
            start_time: Instant::now(),
            scheduler: scheduler.clone(),
        });

        let tg_cancel = cancel_rx.clone();
        tokio::spawn(async move {
            tg_channel.run_polling_loop(handler, tg_cancel).await;
        });
        println!("🤖 Telegram Bot gateway active and listening for authorized messages.");
    } else {
        println!("ℹ️ Telegram Bot not configured (set telegram.bot_token in config or TELEGRAM_BOT_TOKEN env var).");
    }

    // 4. HTTP Gateway Server
    let gw_cfg = config.gateway.clone().unwrap_or_default();
    let host = gw_cfg.host.as_deref().unwrap_or("127.0.0.1");
    let port = gw_cfg.port.unwrap_or(8080);
    let bind_addr = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => {
            println!("🌐 HTTP Gateway listening on http://{}", bind_addr);
            Some(l)
        }
        Err(e) => {
            tracing::warn!("Could not bind HTTP gateway to {}: {}", bind_addr, e);
            None
        }
    };

    if let Some(l) = listener {
        let mut http_cancel_rx = cancel_rx.clone();
        let preset_name = config.preset.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = http_cancel_rx.changed() => {
                        if *http_cancel_rx.borrow() {
                            break;
                        }
                    }
                    accept_res = l.accept() => {
                        if let Ok((mut stream, _)) = accept_res {
                            let preset = preset_name.clone();
                            tokio::spawn(async move {
                                let mut buf = [0u8; 1024];
                                if let Ok(n) = stream.read(&mut buf).await {
                                    if n > 0 {
                                        let req = String::from_utf8_lossy(&buf[..n]);
                                        if req.starts_with("GET /health") || req.starts_with("GET / ") {
                                            let body = format!("{{\"status\":\"ok\",\"gateway\":\"purrcode-ai\",\"preset\":\"{}\"}}\n", preset);
                                            let resp = format!(
                                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                                body.len(),
                                                body
                                            );
                                            let _ = stream.write_all(resp.as_bytes()).await;
                                        } else {
                                            let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                            let _ = stream.write_all(not_found.as_bytes()).await;
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            }
        });
    }

    println!("🚀 PurrCode AI Gateway daemon is running (press Ctrl-C to terminate).");

    tokio::signal::ctrl_c()
        .await
        .map_err(|e| PurrError::Internal(format!("Signal error: {}", e)))?;

    println!("\n🛑 Shutting down PurrCode AI Gateway daemon...");
    let _ = cancel_tx.send(true);
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("👋 Gateway stopped gracefully.");

    Ok(())
}
