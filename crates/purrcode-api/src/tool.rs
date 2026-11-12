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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow,
    Ask,
    Deny,
}

pub trait PolicyGate: Send + Sync {
    fn evaluate_tool(
        &self,
        tool_name: &str,
        tier: ApprovalTier,
        is_outside_workspace: bool,
    ) -> ApprovalDecision;
}

pub trait AuditSink: Send + Sync {
    fn record_event(
        &self,
        session_id: &str,
        agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    );
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
    /// True when this turn runs inside an unattended cron job (v0.3 F19).
    /// The `task` tool forces `is_cron` on every delegation and ignores
    /// any model-supplied `shadow_override_reason` while this is set, so
    /// a cron run can never talk its way past a Tuxeshadow objection.
    pub is_cron: bool,
    /// OS sandbox for the `bash` tool (v0.3 F21). `Some` only when the
    /// active mode preset declares `sandbox: "os"`: every `bash` child
    /// is then confined by a Landlock ruleset with this strictness.
    /// `None` (confinement presets) leaves bash unsandboxed at the OS
    /// level — workspace confinement still applies.
    #[serde(default)]
    pub sandbox: Option<SandboxStrictness>,
}

/// Strictness of the OS sandbox (`sandbox: "os"`), a ruleset inside the
/// `os` sandbox mode — not a new preset axis (ADR-014 item 8).
/// Serializes as kebab-case, matching the preset file spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxStrictness {
    /// The `bash` child may read and execute anywhere, and write
    /// nowhere except `/dev/null` (a sink). No file creation,
    /// deletion, rename, or truncation outside the sandbox's own
    /// allowances — including inside the workspace.
    ReadOnly,
    /// Like `ReadOnly`, but the workspace root itself is writable
    /// (create / modify / delete / rename / truncate under it).
    WorkspaceWrite,
    /// The Landlock mechanism still runs (ruleset + `no_new_privs`),
    /// but the ruleset grants full filesystem access everywhere.
    /// For an operator who explicitly wants the OS sandbox engaged
    /// without filesystem limits.
    DangerFullAccess,
}

impl Default for SandboxStrictness {
    /// Fail-closed: a preset that declares `sandbox: "os"` without a
    /// `sandbox_mode` gets the strictest ruleset.
    fn default() -> Self {
        Self::ReadOnly
    }
}

impl SandboxStrictness {
    /// The preset-file spelling: `"read-only"`, `"workspace-write"`,
    /// `"danger-full-access"`.
    pub fn as_kebab_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }

    /// Parse a preset-file spelling. `None` for anything else — the
    /// caller must fail closed on it, never guess a looser mode.
    pub fn from_kebab(value: &str) -> Option<Self> {
        match value {
            "read-only" => Some(Self::ReadOnly),
            "workspace-write" => Some(Self::WorkspaceWrite),
            "danger-full-access" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool(&'static str);

    #[async_trait]
    impl Tool for DummyTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.0.to_string(),
                description: "Dummy tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
                tier: ApprovalTier::Read,
            }
        }

        async fn execute(
            &self,
            _arguments: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult> {
            Ok(ToolResult {
                output: "ok".to_string(),
                diff_preview: None,
                metadata: None,
            })
        }
    }

    #[test]
    fn test_tool_registry_register_retain_and_freeze() {
        let mut reg = ToolRegistry::new();
        assert!(reg.register(Arc::new(DummyTool("read"))).is_ok());
        assert!(reg.register(Arc::new(DummyTool("write"))).is_ok());
        assert_eq!(reg.list().len(), 2);

        reg.retain(|name| name == "read");
        assert!(reg.get("read").is_some());
        assert!(reg.get("write").is_none());

        reg.freeze();
        assert!(reg.is_frozen());
        assert!(reg.register(Arc::new(DummyTool("bash"))).is_err());
    }
}
