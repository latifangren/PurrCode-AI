// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::SkillManager;

pub struct ReadSkillTool {
    manager: Arc<Mutex<SkillManager>>,
}

impl ReadSkillTool {
    pub fn new(manager: Arc<Mutex<SkillManager>>) -> Self {
        Self { manager }
    }
}

#[derive(Deserialize)]
struct ReadSkillArgs {
    name: String,
}

#[async_trait]
impl Tool for ReadSkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_skill".to_string(),
            description:
                "Load the full instructions and guidelines for one available skill by name"
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill name to load" }
                },
                "required": ["name"]
            }),
            tier: ApprovalTier::Read,
        }
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let args: ReadSkillArgs =
            serde_json::from_value(arguments).map_err(|e| PurrError::Tool {
                tool: "read_skill".to_string(),
                message: format!("Invalid read_skill arguments: {e}"),
            })?;

        let mgr = self.manager.lock().await;
        let content = mgr.read_skill_body(&args.name)?;

        // Record skill usage in .purrcode/skills_telemetry.json for the Curator engine
        let telemetry_file = _ctx
            .workspace_root
            .join(".purrcode")
            .join("skills_telemetry.json");
        let _ = record_skill_usage(&telemetry_file, &args.name);

        Ok(ToolResult {
            output: content,
            diff_preview: None,
            metadata: Some(serde_json::json!({
                "skill_name": args.name
            })),
        })
    }
}

fn record_skill_usage(path: &std::path::Path, skill_name: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut store: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({ "records": [] }))
    } else {
        serde_json::json!({ "records": [] })
    };

    let new_rec = serde_json::json!({
        "skill_name": skill_name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "success": true,
        "duration_ms": 0
    });

    if let Some(recs) = store.get_mut("records").and_then(|r| r.as_array_mut()) {
        recs.push(new_rec);
        if recs.len() > 10_000 {
            let excess = recs.len() - 10_000;
            recs.drain(0..excess);
        }
    }

    let serialized = serde_json::to_string_pretty(&store).map_err(std::io::Error::other)?;
    std::fs::write(path, serialized)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_skill_usage_appends_records() {
        let temp_dir =
            std::env::temp_dir().join(format!("purr_telemetry_test_{}", uuid::Uuid::new_v4()));
        let telemetry_file = temp_dir.join("skills_telemetry.json");

        assert!(record_skill_usage(&telemetry_file, "debug").is_ok());
        assert!(record_skill_usage(&telemetry_file, "review").is_ok());

        let content = std::fs::read_to_string(&telemetry_file).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        let records = val["records"].as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["skill_name"], "debug");
        assert_eq!(records[1]["skill_name"], "review");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
