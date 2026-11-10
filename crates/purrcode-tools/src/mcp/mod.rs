// SPDX-License-Identifier: GPL-3.0-or-later

pub mod client;

pub use client::{McpDynamicTool, McpToolDefinition, StdioMcpClient};
use purrcode_api::error::Result;
use purrcode_config::McpServerConfig;
use std::collections::HashMap;
use std::sync::Arc;

pub struct McpManager {
    clients: HashMap<String, Arc<StdioMcpClient>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Spawns configured MCP servers and registers their tools into the provided registry
    pub async fn load_servers(
        &mut self,
        servers: &HashMap<String, McpServerConfig>,
        registry: &mut crate::ToolRegistry,
    ) -> Result<()> {
        for (name, cfg) in servers {
            if let Some(ref cmd) = cfg.command {
                match StdioMcpClient::spawn(name.clone(), cmd, &cfg.args, &cfg.env).await {
                    Ok(client) => {
                        let client_arc = Arc::new(client);
                        if let Ok(tools) = client_arc.list_tools().await {
                            for tool_def in tools {
                                let dynamic_tool =
                                    McpDynamicTool::new(client_arc.clone(), tool_def);
                                let _ = registry.register(Arc::new(dynamic_tool));
                            }
                        }
                        self.clients.insert(name.clone(), client_arc);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to start MCP server '{}': {}", name, e);
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
