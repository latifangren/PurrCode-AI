// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcNotification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub struct StdioMcpClient {
    pub server_name: String,
    stdin: Arc<Mutex<ChildStdin>>,
    reader: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
    _child: Arc<Mutex<Child>>,
    next_id: AtomicU64,
}

impl StdioMcpClient {
    pub async fn spawn(
        server_name: impl Into<String>,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let name = server_name.into();
        let mut cmd = Command::new(command);
        cmd.args(args);
        scrub_sensitive_env(&mut cmd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| PurrError::Tool {
            tool: format!("mcp:{}", name),
            message: format!(
                "Failed to spawn MCP server '{}' (command: '{}'): {e}",
                name, command
            ),
        })?;

        let stdin = child.stdin.take().ok_or_else(|| PurrError::Tool {
            tool: format!("mcp:{}", name),
            message: "Failed to open stdin for MCP server".to_string(),
        })?;

        let stdout = child.stdout.take().ok_or_else(|| PurrError::Tool {
            tool: format!("mcp:{}", name),
            message: "Failed to open stdout for MCP server".to_string(),
        })?;

        let client = Self {
            server_name: name,
            stdin: Arc::new(Mutex::new(stdin)),
            reader: Arc::new(Mutex::new(BufReader::new(stdout))),
            _child: Arc::new(Mutex::new(child)),
            next_id: AtomicU64::new(1),
        };

        // Initialize MCP handshake
        client.initialize().await?;

        Ok(client)
    }

    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        let req_json = serde_json::to_string(&req)? + "\n";

        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(req_json.as_bytes())
                .await
                .map_err(|e| PurrError::Tool {
                    tool: format!("mcp:{}", self.server_name),
                    message: format!("Failed to write to MCP stdin: {e}"),
                })?;
            stdin.flush().await.map_err(|e| PurrError::Tool {
                tool: format!("mcp:{}", self.server_name),
                message: format!("Failed to flush MCP stdin: {e}"),
            })?;
        }

        // Read response line-by-line
        let mut reader = self.reader.lock().await;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| PurrError::Tool {
                    tool: format!("mcp:{}", self.server_name),
                    message: format!("Failed to read line from MCP stdout: {e}"),
                })?;

            if bytes_read == 0 {
                return Err(PurrError::Tool {
                    tool: format!("mcp:{}", self.server_name),
                    message: "MCP server process closed connection (EOF)".to_string(),
                });
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if resp.id == Some(id) {
                    if let Some(err) = resp.error {
                        return Err(PurrError::Tool {
                            tool: format!("mcp:{}", self.server_name),
                            message: format!("MCP error {}: {}", err.code, err.message),
                        });
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
            }
        }
    }

    async fn send_notification(&self, method: &str) -> Result<()> {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0",
            method,
        };
        let notif_json = serde_json::to_string(&notif)? + "\n";
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(notif_json.as_bytes())
            .await
            .map_err(|e| PurrError::Tool {
                tool: format!("mcp:{}", self.server_name),
                message: format!("Failed to write notification to MCP stdin: {e}"),
            })?;
        stdin.flush().await.map_err(|e| PurrError::Tool {
            tool: format!("mcp:{}", self.server_name),
            message: format!("Failed to flush MCP stdin: {e}"),
        })?;
        Ok(())
    }

    pub async fn initialize(&self) -> Result<()> {
        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "purrcode-ai",
                "version": "0.1.0"
            }
        });

        let _ = self.send_request("initialize", Some(init_params)).await?;
        self.send_notification("notifications/initialized").await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let res = self
            .send_request("tools/list", Some(serde_json::json!({})))
            .await?;
        let tools_val = res
            .get("tools")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(tools_val).map_err(|e| PurrError::Tool {
            tool: format!("mcp:{}", self.server_name),
            message: format!("Failed to parse tools/list result: {e}"),
        })
    }

    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<String> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });

        let res = self.send_request("tools/call", Some(params)).await?;
        if let Some(content) = res.get("content").and_then(|c| c.as_array()) {
            let mut out = String::new();
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    out.push_str(text);
                }
            }
            Ok(out)
        } else {
            Ok(res.to_string())
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<Value>,
}

/// Dynamic wrapper bridging an individual MCP tool into PurrCode's `Tool` trait
pub struct McpDynamicTool {
    server_client: Arc<StdioMcpClient>,
    pub mcp_tool_name: String,
    pub full_tool_name: String,
    pub description: String,
    pub schema: Value,
}

impl McpDynamicTool {
    pub fn new(server_client: Arc<StdioMcpClient>, def: McpToolDefinition) -> Self {
        let full_name = format!("mcp__{}__{}", server_client.server_name, def.name);
        Self {
            mcp_tool_name: def.name,
            full_tool_name: full_name,
            description: def.description.unwrap_or_else(|| "MCP tool".to_string()),
            schema: def
                .input_schema
                .unwrap_or_else(|| serde_json::json!({"type": "object"})),
            server_client,
        }
    }
}

#[async_trait]
impl Tool for McpDynamicTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.full_tool_name.clone(),
            description: format!(
                "[MCP: {}] {}",
                self.server_client.server_name, self.description
            ),
            parameters: self.schema.clone(),
            tier: ApprovalTier::Exec,
        }
    }

    async fn execute(&self, arguments: Value, ctx: &ToolContext) -> Result<ToolResult> {
        if ctx.is_dry_run {
            return Ok(ToolResult {
                output: format!(
                    "Dry-run: would call MCP tool '{}' on server '{}'",
                    self.mcp_tool_name, self.server_client.server_name
                ),
                diff_preview: Some(format!(
                    "[MCP: {}] {}({})",
                    self.server_client.server_name, self.mcp_tool_name, arguments
                )),
                metadata: None,
            });
        }
        let result_text = self
            .server_client
            .call_tool(&self.mcp_tool_name, arguments)
            .await?;
        Ok(ToolResult {
            output: result_text,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "mcp_server": self.server_client.server_name,
                "mcp_tool": self.mcp_tool_name
            })),
        })
    }
}

/// Removes PurrCode and provider secret environment variables before spawning child processes.
pub fn scrub_sensitive_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if let Some(k_str) = key.to_str() {
            let upper = k_str.to_ascii_uppercase();
            if upper.starts_with("PURRCODE_")
                || upper.ends_with("_API_KEY")
                || upper.ends_with("_SECRET")
                || upper.ends_with("_TOKEN")
                || upper == "OPENAI_API_KEY"
                || upper == "ANTHROPIC_API_KEY"
                || upper == "GEMINI_API_KEY"
                || upper == "DEEPSEEK_API_KEY"
                || upper == "OPENROUTER_API_KEY"
            {
                cmd.env_remove(&key);
            }
        }
    }
}
