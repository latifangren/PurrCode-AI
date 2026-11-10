pub mod coordination;
pub mod execution;
pub mod fs;
pub mod mcp;
pub mod search;
pub mod skill;
pub mod web;

pub use coordination::{AskOption, AskRequester, AskTool, TodoTool};
pub use execution::BashTool;
pub use fs::{EditTool, ReadTool, WriteTool};
pub use search::{FindTool, GlobTool, GrepTool};
pub use skill::{ReadSkillTool, SkillManager};
pub use web::{WebfetchTool, WebsearchTool};

use purrcode_api::tool::Tool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    is_frozen: bool,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            is_frozen: false,
        }
    }

    /// Creates and populates the default toolset for PurrCode
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(ReadTool)).unwrap();
        registry.register(Arc::new(WriteTool)).unwrap();
        registry.register(Arc::new(EditTool)).unwrap();
        registry.register(Arc::new(BashTool)).unwrap();
        registry.register(Arc::new(GrepTool)).unwrap();
        registry.register(Arc::new(GlobTool)).unwrap();
        registry.register(Arc::new(FindTool)).unwrap();
        registry.register(Arc::new(TodoTool::new())).unwrap();
        registry.register(Arc::new(AskTool::new())).unwrap();
        registry.register(Arc::new(WebfetchTool::new())).unwrap();
        registry.register(Arc::new(WebsearchTool::new())).unwrap();

        let mut skill_mgr = SkillManager::new();
        skill_mgr.discover_all(&std::path::PathBuf::from("."));
        registry.register(Arc::new(ReadSkillTool::new(Arc::new(Mutex::new(skill_mgr))))).unwrap();

        registry
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), &'static str> {
        if self.is_frozen {
            return Err("Cannot register tool to a frozen registry");
        }
        let name = tool.definition().name;
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.is_frozen = true;
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<purrcode_api::tool::ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
