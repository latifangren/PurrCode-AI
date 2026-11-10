pub mod execution;
pub mod fs;
pub mod mcp;
pub mod search;

use fs::{EditTool, ReadTool, WriteTool};
use execution::BashTool;
use search::{FindTool, GrepTool};
use purrcode_api::tool::Tool;
use std::collections::HashMap;
use std::sync::Arc;

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
        registry.register(Arc::new(FindTool)).unwrap();
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
