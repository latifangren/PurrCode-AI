// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

use purrcode_api::provider::ModelProvider;
use purrcode_api::tool::AuditSink;
use purrcode_approval::ApprovalGate;
use purrcode_core::{AgentLoop, ApprovalRequester, SteeringQueue};
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
        let session_path = session_dir.into();
        let registry = Arc::new(Mutex::new(SubagentRegistry::with_session_dir(
            &session_path,
        )));
        let model_str = model.into();

        // Meowstro toolset: strictly read, grep, glob, find, todo, ask, task (+ read_skill).
        // NEVER write, edit, or bash!
        let mut tool_reg = ToolRegistry::new();
        let _ = tool_reg.register(Arc::new(purrcode_tools::fs::ReadTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::GrepTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::GlobTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::search::FindTool));
        let _ = tool_reg.register(Arc::new(purrcode_tools::coordination::TodoTool::new()));
        let ask_tool = match ask_requester.clone() {
            Some(req) => purrcode_tools::coordination::AskTool::with_requester(req),
            None => purrcode_tools::coordination::AskTool::new(),
        };
        let _ = tool_reg.register(Arc::new(ask_tool));

        let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
        skill_mgr.discover_all(&std::path::PathBuf::from("."));
        let _ = tool_reg.register(Arc::new(purrcode_tools::skill::ReadSkillTool::new(
            Arc::new(Mutex::new(skill_mgr)),
        )));

        let task_tool = TaskTool::with_full_options(
            registry.clone(),
            provider.clone(),
            model_str.clone(),
            session_path,
            approval_requester.clone(),
            ask_requester,
            approval_gate.clone(),
            audit_sink.clone(),
            yolo,
        );
        let _ = tool_reg.register(Arc::new(task_tool));
        tool_reg.freeze();

        let mut agent_loop =
            AgentLoop::new(provider, model_str, Arc::new(tool_reg)).with_yolo(yolo);
        if let Some(req) = approval_requester {
            agent_loop = agent_loop.with_approval_requester(req);
        }
        if let Some(gate) = approval_gate {
            agent_loop = agent_loop.with_policy_gate(gate);
        }
        if let Some(sink) = audit_sink {
            agent_loop = agent_loop.with_audit_sink(sink);
        }
        if let Some(sq) = steering_queue {
            agent_loop = agent_loop.with_steering_queue(sq);
        }
        if let Some(cf) = cancel_flag {
            agent_loop = agent_loop.with_cancel_flag(cf);
        }

        Self {
            agent_loop,
            subagent_registry: registry,
        }
    }

    pub fn agent_loop(&self) -> &AgentLoop {
        &self.agent_loop
    }

    pub fn system_prompt(skills_index: &str) -> String {
        let mut prompt = String::from(
            r#"You are Meowstro, the lead orchestrator and user's representative in PurrCode AI.
You understand architecture, analyze user requests, devise execution plans, and delegate tasks to your specialized subagent team (The Clowder):
- 'tinker': The fixer/implementer who has full write and bash tools to write code, edit files, and run tests.
- 'whisker': The explorer who searches and scouts the codebase and documentation.
- 'sphinx': The oracle advisor for complex architectural questions and hard debugging.
- 'calico': The UI/TUI designer who reads, writes, and edits UI code.
- 'tuxeshadow': The shadow critic and auditor who reviews plans and risky actions before execution.
- 'clowder': The multi-seat council for ambiguous or high-risk architectural decisions.

RULE OF HARNESS: You NEVER write or edit code directly, and you NEVER execute bash commands directly.
When code needs to be created, modified, or tested, you MUST delegate the work to Tinker using the 'task' tool.
When inspecting files, you may use 'read', 'grep', 'glob', and 'find'.
Your main context holds plans, summaries, and architectural decisions."#,
        );

        if !skills_index.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(skills_index);
        }

        prompt
    }
}
