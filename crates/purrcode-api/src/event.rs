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
