// SPDX-License-Identifier: GPL-3.0-or-later

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalTier {
    Read,
    Write,
    Exec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub tier: ApprovalTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub session_id: String,
    pub agent_role: String,
    pub workspace_root: std::path::PathBuf,
    pub is_dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub diff_preview: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult>;
}

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

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> std::result::Result<(), &'static str> {
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

    pub fn is_frozen(&self) -> bool {
        self.is_frozen
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&str) -> bool,
    {
        if !self.is_frozen {
            self.tools.retain(|name, _| f(name));
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
