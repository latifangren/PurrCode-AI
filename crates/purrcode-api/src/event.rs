// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Strongly typed events broadcasted via EventBus across all surfaces (TUI, headless, Telegram, WebUI)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AgentEvent {
    TurnStarted {
        session_id: String,
        turn_id: String,
        timestamp: DateTime<Utc>,
    },
    TextChunk {
        text: String,
    },
    ReasoningChunk {
        thinking: String,
    },
    ToolCallStarted {
        call_id: String,
        tool: String,
        arguments: serde_json::Value,
    },
    ToolCallApprovalRequested {
        call_id: String,
        tool: String,
        description: String,
        diff_preview: Option<String>,
    },
    ToolCallFinished {
        call_id: String,
        tool: String,
        result: serde_json::Value,
        success: bool,
    },
    SubagentSpawned {
        subagent_id: String,
        parent_id: String,
        role: String,
        brief: String,
    },
    SubagentStatusChanged {
        subagent_id: String,
        status: SubagentStatus,
        summary: Option<String>,
    },
    TurnFinished {
        session_id: String,
        turn_id: String,
        tokens_used: Option<u64>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentStatus {
    Pending,
    Running,
    Done,
    Failed,
    Interrupted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_event_serialization_roundtrip() {
        let event = AgentEvent::ToolCallStarted {
            call_id: "call_123".to_string(),
            tool: "edit".to_string(),
            arguments: serde_json::json!({"path": "sample.rs"}),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"ToolCallStarted\""));
        assert!(json.contains("\"tool\":\"edit\""));

        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            AgentEvent::ToolCallStarted { call_id, tool, .. } => {
                assert_eq!(call_id, "call_123");
                assert_eq!(tool, "edit");
            }
            _ => panic!("Expected ToolCallStarted event"),
        }
    }

    #[test]
    fn test_subagent_status_serde() {
        let status = SubagentStatus::Running;
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, "\"running\"");
        let deserialized: SubagentStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, SubagentStatus::Running);
    }
}
