// SPDX-License-Identifier: GPL-3.0-or-later

//! Build the compact tool-call log one turn leaves behind (v0.3 F20,
//! ADR-014 item 7). The log is the curator's only context (besides
//! its system prompt), so it stays small enough for a cheap model call.

use purrcode_api::provider::{ChatMessage, MessageRole};

/// Max characters for one entry's text, and for the whole log.
const ENTRY_CAP: usize = 300;
const LOG_CAP: usize = 4000;

fn trunc(s: &str, cap: usize) -> String {
    let taken: String = s.chars().take(cap).collect();
    if s.chars().count() > cap {
        format!("{taken}…")
    } else {
        taken
    }
}

/// The turn's new messages (from `from_idx`) as a compact log: the
/// user's request first, then each tool call (name + args) and each
/// tool result, in order. Pure and total — the curator prompt is built
/// from this alone. The system prompt is skipped (it is the same every
/// turn and says nothing about what the turn did).
pub fn build_tool_log(messages: &[ChatMessage], from_idx: usize) -> String {
    let mut out = String::new();
    for msg in messages.iter().skip(from_idx) {
        let entry = match msg.role {
            MessageRole::System => continue,
            MessageRole::User => {
                // A tool result arrives as a user-role message carrying
                // a tool_call_id in some providers.
                if msg.tool_call_id.is_some() {
                    format!("Result: {}", trunc(&msg.content, ENTRY_CAP))
                } else {
                    format!("User: {}", trunc(&msg.content, ENTRY_CAP))
                }
            }
            MessageRole::Assistant => match &msg.tool_calls {
                Some(calls) if !calls.is_empty() => calls
                    .iter()
                    .map(|c| format!("Call {}({})", c.name, trunc(&c.arguments, ENTRY_CAP)))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => format!("Assistant: {}", trunc(&msg.content, ENTRY_CAP)),
            },
            MessageRole::Tool => format!("Result: {}", trunc(&msg.content, ENTRY_CAP)),
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&entry);
        if out.len() >= LOG_CAP {
            break;
        }
    }
    trunc(&out, LOG_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_api::provider::ToolCallRequest;

    fn msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_build_tool_log_orders_calls_and_results() {
        let messages = vec![
            msg(MessageRole::System, "system prompt (skipped)"),
            msg(MessageRole::User, "fix the failing tests"),
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                tool_calls: Some(vec![
                    ToolCallRequest {
                        id: "c1".to_string(),
                        name: "bash".to_string(),
                        arguments: "{\"command\": \"cargo test\"}".to_string(),
                    },
                    ToolCallRequest {
                        id: "c2".to_string(),
                        name: "read".to_string(),
                        arguments: "{\"path\": \"src/lib.rs\"}".to_string(),
                    },
                ]),
                tool_call_id: None,
            },
            msg(MessageRole::Tool, "3 failed, 41 passed"),
            msg(MessageRole::Assistant, "Fixed. All green now."),
        ];
        let log = build_tool_log(&messages, 1);
        assert!(log.starts_with("User: fix the failing tests"), "{log}");
        assert!(
            log.contains("Call bash({\"command\": \"cargo test\"})"),
            "{log}"
        );
        assert!(log.contains("Call read("), "{log}");
        assert!(log.contains("Result: 3 failed, 41 passed"), "{log}");
        assert!(log.contains("Assistant: Fixed. All green now."), "{log}");
        assert!(!log.contains("system prompt"), "{log}");
    }

    #[test]
    fn test_build_tool_log_truncates_long_entries() {
        let long = "x".repeat(2000);
        let messages = vec![msg(MessageRole::User, &long)];
        let log = build_tool_log(&messages, 0);
        assert!(log.len() <= LOG_CAP + 3, "log len {}", log.len());
        assert!(log.ends_with('…'), "{log}");
    }
}
