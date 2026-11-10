use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use purrcode_api::provider::ModelProvider;
use purrcode_core::AgentLoop;
use purrcode_tools::ToolRegistry;

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
        let registry = Arc::new(Mutex::new(SubagentRegistry::new()));
        let model_str = model.into();
        let session_path = session_dir.into();

        // Meowstro toolset: read-only + delegation (TaskTool). NO Write/Edit/Bash!
        let mut tool_reg = ToolRegistry::new();
        tool_reg.register(Arc::new(purrcode_tools::fs::ReadTool)).unwrap();
        tool_reg.register(Arc::new(purrcode_tools::search::GrepTool)).unwrap();
        tool_reg.register(Arc::new(purrcode_tools::search::FindTool)).unwrap();
        tool_reg.register(Arc::new(purrcode_tools::coordination::TodoTool::new())).unwrap();
        tool_reg.register(Arc::new(purrcode_tools::coordination::AskTool)).unwrap();
        tool_reg.register(Arc::new(purrcode_tools::web::WebsearchTool::new())).unwrap();

        let mut skill_mgr = purrcode_tools::skill::SkillManager::new();
        skill_mgr.discover_all(&std::path::PathBuf::from("."));
        tool_reg.register(Arc::new(purrcode_tools::skill::ReadSkillTool::new(Arc::new(Mutex::new(skill_mgr))))).unwrap();

        let task_tool = TaskTool::new(
            registry.clone(),
            provider.clone(),
            model_str.clone(),
            session_path,
        );
        tool_reg.register(Arc::new(task_tool)).unwrap();
        tool_reg.freeze();

        let agent_loop = AgentLoop::new(provider, model_str, Arc::new(tool_reg));

        Self {
            agent_loop,
            subagent_registry: registry,
        }
    }

    pub fn agent_loop(&self) -> &AgentLoop {
        &self.agent_loop
    }

    pub fn system_prompt() -> &'static str {
        r#"You are Meowstro, the lead orchestrator and user's representative in PurrCode AI.
You understand architecture, analyze user requests, devise execution plans, and delegate tasks to your specialized subagent team (The Clowder):
- 'tinker': The fixer/implementer who has full write and bash tools to write code, edit files, and run tests.
- 'whisker': The explorer who searches and scouts the codebase.
- 'sphinx': The oracle advisor for complex architectural questions.

RULE OF HARNESS: You NEVER write or edit code directly, and you NEVER execute bash commands directly.
When code needs to be created, modified, or tested, you MUST delegate the work to Tinker using the 'task' tool.
When inspecting files, you may use 'read', 'grep', and 'find'.
Your main context holds plans, summaries, and architectural decisions."#
    }
}
