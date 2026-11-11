// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};
pub use purrcode_api::tool::ApprovalDecision;
use purrcode_api::tool::{ApprovalTier, AuditSink, PolicyGate};
use purrcode_config::{ApprovalMode, GuardrailMode, ModePreset, SandboxMode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub session_id: String,
    pub agent: String,
    pub event: String,
    pub tool: Option<String>,
    pub decision: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct AuditLogger {
    audit_file: PathBuf,
    max_bytes: u64,
}

impl AuditLogger {
    pub fn new(purrcode_dir: impl Into<PathBuf>) -> Self {
        let dir = purrcode_dir.into();
        Self {
            audit_file: dir.join("audit.jsonl"),
            max_bytes: 5 * 1024 * 1024, // 5 MB rotation threshold
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn audit_file_path(&self) -> &Path {
        &self.audit_file
    }

    pub fn log_entry(&self, mut entry: AuditEntry) -> Result<()> {
        entry.detail = scrub_secrets(&entry.detail);
        if let Some(parent) = self.audit_file.parent() {
            std::fs::create_dir_all(parent).map_err(PurrError::Io)?;
        }

        self.rotate_if_needed()?;

        let line = serde_json::to_string(&entry)? + "\n";
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_file)
            .map_err(PurrError::Io)?;
        file.write_all(line.as_bytes()).map_err(PurrError::Io)?;
        Ok(())
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if let Ok(meta) = std::fs::metadata(&self.audit_file) {
            if meta.len() >= self.max_bytes {
                let date_tag = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                let rotated = self
                    .audit_file
                    .with_file_name(format!("audit-{}.jsonl", date_tag));
                let _ = std::fs::rename(&self.audit_file, rotated);
            }
        }
        Ok(())
    }
}

impl AuditSink for AuditLogger {
    fn record_event(
        &self,
        session_id: &str,
        agent: &str,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    ) {
        let _ = self.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            event: event.to_string(),
            tool: tool.map(|t| t.to_string()),
            decision: decision.to_string(),
            detail: detail.to_string(),
        });
    }
}

/// Scrubs credentials, bearer tokens, and `*_API_KEY` values from logs and LLM payloads.
pub fn scrub_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for marker in ["sk-", "Bearer ", "api_key=", "API_KEY="] {
        while let Some(pos) = out.find(marker) {
            let start = pos + marker.len();
            let rest = &out[start..];
            let end_offset = rest
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == '}')
                .unwrap_or(rest.len());
            if end_offset > 0 && &rest[..end_offset] != "[FILTERED]" {
                out.replace_range(start..start + end_offset, "[FILTERED]");
            } else {
                break;
            }
        }
    }
    out
}

#[derive(Clone)]
pub struct ApprovalGate {
    pub mode: ApprovalMode,
    pub guardrail: GuardrailMode,
    pub sandbox: SandboxMode,
    deny_rules: HashSet<String>,
    ask_rules: HashSet<String>,
    allow_rules: HashSet<String>,
}

impl ApprovalGate {
    pub fn new(mode_str: impl AsRef<str>) -> Self {
        let m = mode_str.as_ref();
        let (mode, guardrail) = match m {
            "yolo" => (ApprovalMode::Yolo, GuardrailMode::Default),
            "manual" => (ApprovalMode::Manual, GuardrailMode::Default),
            "carrier" | "strict" => (ApprovalMode::Manual, GuardrailMode::Strict),
            "oyen" | "god" => (ApprovalMode::Supervised, GuardrailMode::God),
            "auto" => (ApprovalMode::Auto, GuardrailMode::Default),
            _ => (ApprovalMode::Supervised, GuardrailMode::Default),
        };
        Self {
            mode,
            guardrail,
            sandbox: SandboxMode::Confinement,
            deny_rules: HashSet::new(),
            ask_rules: HashSet::new(),
            allow_rules: HashSet::new(),
        }
    }

    pub fn from_preset(preset: &ModePreset, yolo_override: bool) -> Self {
        Self {
            mode: if yolo_override {
                ApprovalMode::Yolo
            } else {
                preset.approval
            },
            guardrail: preset.guardrail,
            sandbox: preset.sandbox,
            deny_rules: HashSet::new(),
            ask_rules: HashSet::new(),
            allow_rules: preset.tools.iter().cloned().collect(),
        }
    }

    pub fn add_deny_rule(&mut self, rule: impl Into<String>) {
        self.deny_rules.insert(rule.into());
    }

    pub fn add_ask_rule(&mut self, rule: impl Into<String>) {
        self.ask_rules.insert(rule.into());
    }

    pub fn add_allow_rule(&mut self, rule: impl Into<String>) {
        self.allow_rules.insert(rule.into());
    }

    /// Evaluates `deny > ask > allow` over tool name, tier, and workspace confinement.
    pub fn evaluate_tool(
        &self,
        tool_name: &str,
        tier: ApprovalTier,
        is_outside_workspace: bool,
    ) -> ApprovalDecision {
        // 1. Explicit deny rules always win (deny > ask > allow)
        if self.deny_rules.contains(tool_name) || self.deny_rules.contains("*") {
            return ApprovalDecision::Deny;
        }

        // 2. Strict guardrail denies outside-workspace writes/execs unconditionally
        if self.guardrail == GuardrailMode::Strict && is_outside_workspace {
            return ApprovalDecision::Deny;
        }

        // 3. Explicit ask rules win over allow
        if self.ask_rules.contains(tool_name) {
            return ApprovalDecision::Ask;
        }

        // 4. Outside workspace requires at least Ask in confinement mode (or Deny in strict)
        if is_outside_workspace && self.sandbox == SandboxMode::Confinement {
            return if self.mode == ApprovalMode::Yolo {
                ApprovalDecision::Ask
            } else {
                ApprovalDecision::Deny
            };
        }

        // 5. Evaluate approval axis
        match self.mode {
            ApprovalMode::Yolo => ApprovalDecision::Allow,
            ApprovalMode::Manual => ApprovalDecision::Ask,
            ApprovalMode::Auto => match tier {
                ApprovalTier::Read | ApprovalTier::Write => ApprovalDecision::Allow,
                ApprovalTier::Exec => ApprovalDecision::Ask,
            },
            ApprovalMode::Supervised => match tier {
                ApprovalTier::Read => ApprovalDecision::Allow,
                ApprovalTier::Write | ApprovalTier::Exec => ApprovalDecision::Ask,
            },
        }
    }

    pub fn evaluate(&self, tier: ApprovalTier, is_outside_workspace: bool) -> ApprovalDecision {
        self.evaluate_tool("", tier, is_outside_workspace)
    }

    /// Enforces child policy narrowing: a child cannot escalate beyond its parent's mode.
    pub fn narrow_for_child(&self, requested_mode: ApprovalMode) -> ApprovalMode {
        let rank = |m: ApprovalMode| match m {
            ApprovalMode::Manual => 0,
            ApprovalMode::Supervised => 1,
            ApprovalMode::Auto => 2,
            ApprovalMode::Yolo => 3,
        };
        if rank(requested_mode) <= rank(self.mode) {
            requested_mode
        } else {
            self.mode
        }
    }

    /// Creates a child `ApprovalGate` narrowed to at most the parent's policy and never exceeding `requested_mode`.
    pub fn narrowed_child_gate(&self, requested_mode: ApprovalMode) -> Self {
        let mut child = self.clone();
        child.mode = self.narrow_for_child(requested_mode);
        child
    }
}

impl PolicyGate for ApprovalGate {
    fn evaluate_tool(
        &self,
        tool_name: &str,
        tier: ApprovalTier,
        is_outside_workspace: bool,
    ) -> ApprovalDecision {
        ApprovalGate::evaluate_tool(self, tool_name, tier, is_outside_workspace)
    }
}

pub fn check_workspace_confinement(target: &Path, workspace_root: &Path) -> bool {
    target.starts_with(workspace_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deny_beats_ask_beats_allow_and_narrowing() {
        let mut gate = ApprovalGate::new("yolo");
        gate.add_allow_rule("bash");
        gate.add_ask_rule("bash");
        assert_eq!(
            gate.evaluate_tool("bash", ApprovalTier::Exec, false),
            ApprovalDecision::Ask
        );

        gate.add_deny_rule("bash");
        assert_eq!(
            gate.evaluate_tool("bash", ApprovalTier::Exec, false),
            ApprovalDecision::Deny
        );

        let strict_parent = ApprovalGate::new("carrier");
        let child = strict_parent.narrowed_child_gate(ApprovalMode::Yolo);
        assert_eq!(child.mode, ApprovalMode::Manual);
    }

    #[test]
    fn test_secret_scrubbing_and_audit_rotation() {
        let raw_line = format!(
            "Header Authorization: Bearer {} and sk-999999",
            "tok_abc123"
        );
        let scrubbed = scrub_secrets(&raw_line);
        assert!(scrubbed.contains("Bearer [FILTERED]"));
        assert!(scrubbed.contains("sk-[FILTERED]"));
        assert!(!scrubbed.contains("tok_abc123"));

        let temp_dir = std::env::temp_dir().join(format!(
            "purr_audit_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(1)
        ));
        let logger = AuditLogger::new(&temp_dir).with_max_bytes(100);
        logger
            .log_entry(AuditEntry {
                timestamp: "2026-11-11T00:00:00Z".to_string(),
                session_id: "s1".to_string(),
                agent: "meowstro".to_string(),
                event: "tool_call".to_string(),
                tool: Some("read".to_string()),
                decision: "allow".to_string(),
                detail: format!("Bearer {}", "dummy_val"),
            })
            .unwrap();
        logger
            .log_entry(AuditEntry {
                timestamp: "2026-11-11T00:00:01Z".to_string(),
                session_id: "s1".to_string(),
                agent: "meowstro".to_string(),
                event: "tool_call".to_string(),
                tool: Some("read".to_string()),
                decision: "allow".to_string(),
                detail: "second entry triggers rotation".to_string(),
            })
            .unwrap();

        let files: Vec<_> = std::fs::read_dir(&temp_dir).unwrap().flatten().collect();
        assert!(files.len() >= 2);
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
