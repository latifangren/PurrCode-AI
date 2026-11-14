// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::AuditSink;
use purrcode_approval::ApprovalGate;
use purrcode_config::{Config, ModelPreset};
use purrcode_core::{AgentLoop, ApprovalRequester, EventBus, SteeringQueue};
use purrcode_provider::ProviderRegistry;
use purrcode_tools::{AskRequester, ToolRegistry};

use crate::task::TaskTool;
use crate::SubagentRegistry;

pub struct MeowstroOrchestrator {
    agent_loop: AgentLoop,
    pub subagent_registry: Arc<Mutex<SubagentRegistry>>,
}

impl MeowstroOrchestrator {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::with_options(provider, model, session_dir, None, None, false)
    }

    pub fn with_options(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        yolo: bool,
    ) -> Self {
        Self::with_full_options(
            provider,
            model,
            session_dir,
            approval_requester,
            ask_requester,
            None,
            None,
            None,
            None,
            yolo,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_full_options(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        approval_gate: Option<Arc<ApprovalGate>>,
        audit_sink: Option<Arc<dyn AuditSink>>,
        steering_queue: Option<SteeringQueue>,
        cancel_flag: Option<Arc<AtomicBool>>,
        yolo: bool,
    ) -> Self {
        Self::with_complete_options(
            provider,
            model,
            session_dir,
            approval_requester,
            ask_requester,
            approval_gate,
            audit_sink,
            None,
            None,
            None,
            None,
            steering_queue,
            cancel_flag,
            yolo,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_complete_options(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        session_dir: impl Into<PathBuf>,
        approval_requester: Option<Arc<dyn ApprovalRequester>>,
        ask_requester: Option<Arc<dyn AskRequester>>,
        approval_gate: Option<Arc<ApprovalGate>>,
        audit_sink: Option<Arc<dyn AuditSink>>,
        plugin_dispatcher: Option<Arc<dyn PluginHookDispatcher>>,
        model_preset: Option<ModelPreset>,
        config: Option<Config>,
        provider_registry: Option<Arc<ProviderRegistry>>,
        steering_queue: Option<SteeringQueue>,
        cancel_flag: Option<Arc<AtomicBool>>,
        yolo: bool,
        workspace_root: Option<PathBuf>,
        // EventBus the parent loop (and, via the TaskTool, every child
        // loop) publishes to. `None` = listener-only, pre-R3 behavior.
        event_bus: Option<Arc<EventBus>>,
    ) -> Self {
        let session_path = session_dir.into();
        let registry = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(
            &session_path,
        )));
        let model_str = model.into();
        let ws_root = workspace_root
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Meowstro toolset: full orchestrator suite with execution and delegation tools
        let mut tool_reg = ToolRegistry::new();
        let _ = tool_reg.register(Arc::new(purrcode_tools::fs::ReadTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::fs::WriteTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::fs::EditTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::BashTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::GrepTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::GlobTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::FindTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::coordination::TodoTool::new()));
        let _ = tool_reg.register(Arc::new(purrcode_tools::web::WebfetchTool::new()));
        let _ = tool_reg.register(Arc::new(purrcode_tools::web::WebsearchTool::new()));
        let ask_tool = match ask_requester.clone() {
            Some(req) => purrcode_tools::coordination::AskTool::with_requester(req),
            None => purrcode_tools::coordination::AskTool::new(),
        };
        let _ = tool_reg.register(Arc::new(ask_tool));

        let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
        skill_mgr.discover_all(&ws_root);
        let shared_skill_mgr = Arc::new(Mutex::new(skill_mgr));
        let _ = tool_reg.register(Arc::new(purrcode_tools::skill::ReadSkillTool::new(
            Arc::clone(&shared_skill_mgr),
        )));
        let _ = tool_reg.register(Arc::new(purrcode_tools::skill::ManageSkillTool::new(
            shared_skill_mgr,
        )));

        let meowstro_spec = model_preset.as_ref().and_then(|p| {
            p.agents
                .get("Meowstro")
                .or_else(|| p.agents.get("meowstro"))
                .cloned()
        });

        let mut task_tool = TaskTool::with_full_options(
            registry.clone(),
            provider.clone(),
            model_str.clone(),
            session_path,
            approval_requester.clone(),
            ask_requester,
            approval_gate.clone(),
            audit_sink.clone(),
            yolo,
        )
        .with_model_preset(model_preset);

        let live_streaming = config
            .as_ref()
            .map(|c| c.ui.streaming_mode == "live")
            .unwrap_or(true);
        let auto_compact_limit = config
            .as_ref()
            .and_then(|c| purrcode_core::compute_auto_compact_limit(c, &model_str));
        if let Some(ref pd) = plugin_dispatcher {
            task_tool = task_tool.with_plugin_dispatcher(pd.clone());
        }
        if let (Some(cfg), Some(preg)) = (config, provider_registry) {
            task_tool = task_tool.with_provider_routing(cfg, preg);
        }
        if let Some(ref bus) = event_bus {
            task_tool = task_tool.with_event_bus(bus.clone());
        }

        let _ = tool_reg.register(Arc::new(task_tool));
        tool_reg.freeze();

        let mut agent_loop = AgentLoop::new(provider, model_str, Arc::new(tool_reg))
            .with_yolo(yolo)
            .with_live_streaming(live_streaming)
            .with_auto_compact_limit(auto_compact_limit);
        if let Some(spec) = meowstro_spec {
            agent_loop = agent_loop
                .with_temperature(spec.temperature)
                .with_reasoning_effort(spec.variant);
        }
        if let Some(req) = approval_requester {
            agent_loop = agent_loop.with_approval_requester(req);
        }
        if let Some(gate) = approval_gate {
            agent_loop = agent_loop.with_policy_gate(gate);
        }
        if let Some(sink) = audit_sink {
            agent_loop = agent_loop.with_audit_sink(sink);
        }
        if let Some(pd) = plugin_dispatcher {
            agent_loop = agent_loop.with_plugin_dispatcher(pd);
        }
        if let Some(sq) = steering_queue {
            agent_loop = agent_loop.with_steering_queue(sq);
        }
        if let Some(cf) = cancel_flag {
            agent_loop = agent_loop.with_cancel_flag(cf);
        }
        if let Some(bus) = event_bus {
            agent_loop = agent_loop.with_event_bus(bus);
        }

        Self {
            agent_loop,
            subagent_registry: registry,
        }
    }

    pub fn agent_loop(&self) -> &AgentLoop {
        &self.agent_loop
    }
}

const MEOWSTRO_MD: &str = include_str!("../../../agents/meowstro.md");

impl MeowstroOrchestrator {
    pub fn system_prompt(skills_index: &str) -> String {
        Self::system_prompt_with_workspace(skills_index, None)
    }

    /// Builds Meowstro's system prompt from the embedded `agents/meowstro.md` spec
    /// (or `.purrcode/agents/meowstro.md` override), combined with the shared core
    /// engineering harness contract, structured `<environment>` context, and the
    /// discovered skills index.
    pub fn system_prompt_with_workspace(
        skills_index: &str,
        workspace_root: Option<&Path>,
    ) -> String {
        let override_body = workspace_root.and_then(|ws| {
            let path = ws.join(".purrcode").join("agents").join("meowstro.md");
            std::fs::read_to_string(path).ok().and_then(|raw| {
                let body = crate::task::prompts::strip_yaml_frontmatter(&raw)
                    .trim()
                    .to_string();
                if body.is_empty() {
                    None
                } else {
                    Some(body)
                }
            })
        });

        let mut prompt = override_body.unwrap_or_else(|| {
            crate::task::prompts::strip_yaml_frontmatter(MEOWSTRO_MD)
                .trim()
                .to_string()
        });

        prompt.push_str("\n\n");
        prompt.push_str(purrcode_core::CORE_HARNESS_CONTRACT);
        prompt.push_str("\n\n");
        prompt.push_str(&purrcode_core::format_environment_block(workspace_root));

        if !skills_index.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(skills_index);
        }

        prompt
    }
}
