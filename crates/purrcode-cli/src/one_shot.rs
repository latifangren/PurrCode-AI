// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::Result;
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::tool::{AuditSink, PolicyGate, ToolContext};
use purrcode_approval::{ApprovalGate, AuditEntry, AuditLogger};
use purrcode_config::{Config, ModePresetLoader, ModelPresetLoader};
use purrcode_core::{AgentLoop, SessionStore};
use purrcode_memory::MarkdownMemory;
use purrcode_orchestrator::MeowstroOrchestrator;
use purrcode_plugin::PluginManager;
use purrcode_provider::ProviderRegistry;
use purrcode_tools::mcp::McpManager;
use purrcode_tools::{ToolRegistry, ToolRegistryExt};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Resolve the session target model, exiting with usage guidance when
/// no model is configured. Thin wrapper over
/// `purrcode_service::cron::resolve_target_model` (the picking logic
/// moved to the service crate in the R2 extraction); the exit behavior
/// is kept here because this is the CLI path — background runners use
/// the fallible service function directly.
pub(crate) fn resolve_target_model_or_exit(
    model_override: Option<String>,
    config: &Config,
) -> String {
    match purrcode_service::cron::resolve_target_model(config, model_override) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("Error: No model configured.");
            eprintln!("PurrCode AI has no hardcoded default models.");
            eprintln!();
            eprintln!("Please configure your provider in '~/.purrcode/purrcode.jsonc' or run 'purrcode-ai onboard'.");
            std::process::exit(1);
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
    async fn request_approval(
        &self,
        _call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool {
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

pub(crate) async fn run_one_shot(
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

    let purrcode_dir =
        crate::onboard::get_purrcode_dir().unwrap_or_else(|_| PathBuf::from(".purrcode"));

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
    let agent_role = crate::cli::agent_role_for(use_orchestrator);

    purrcode_service::log_unenforced_plugin_capabilities(
        &audit_logger,
        &session_id,
        agent_role,
        &unenforced_caps,
    );

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
            None,
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
                .request_approval("test-call", "write", "Run tool 'write'", None)
                .await
        );
        assert!(
            !CliApprovalRequester { yolo: true }
                .request_approval("test-call", "write", "Run tool 'write'", None)
                .await
        );
    }
}
