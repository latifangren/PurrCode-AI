// SPDX-License-Identifier: GPL-3.0-or-later

use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub enum MessageItem {
    User(String),
    Assistant {
        text: String,
        reasoning: Option<String>,
    },
    ToolCall {
        name: String,
        args: String,
        status: ToolStatus,
    },
    ToolResult {
        output: String,
        success: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
}

pub struct ApprovalRequest {
    pub tool_name: String,
    pub description: String,
    pub diff_preview: Option<String>,
    pub tx: oneshot::Sender<bool>,
}

#[derive(Debug, Clone)]
pub struct AskOptionItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

pub struct AskPromptRequest {
    pub question: String,
    pub options: Vec<AskOptionItem>,
    pub selected_idx: usize,
    pub tx: oneshot::Sender<Option<String>>,
}

pub enum TuiEvent {
    TextChunk(String),
    ReasoningChunk(String),
    ToolCallStarted {
        name: String,
        args: String,
    },
    ToolCallFinished {
        name: String,
        output: String,
        success: bool,
    },
    ApprovalRequested(ApprovalRequest),
    AskRequested(AskPromptRequest),
    SubagentStatus {
        id: String,
        role: String,
        status: String,
    },
    TurnFinished,
}

pub struct TuiApp {
    pub input: String,
    pub cursor_pos: usize,
    pub items: Vec<MessageItem>,
    pub model_name: String,
    pub preset_name: String,
    pub model_preset_name: String,
    pub session_id: String,
    pub token_usage: u64,
    pub tool_call_count: usize,
    pub is_generating: bool,
    pub should_quit: bool,
    pub scroll_offset: usize,
    pub pending_approval: Option<ApprovalRequest>,
    pub pending_ask: Option<AskPromptRequest>,
    pub active_subagents: Vec<(String, String, String)>,
}

impl TuiApp {
    pub fn new(
        model_name: impl Into<String>,
        preset_name: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            items: Vec::new(),
            model_name: model_name.into(),
            preset_name: preset_name.into(),
            model_preset_name: "free".to_string(),
            session_id: session_id.into(),
            token_usage: 0,
            tool_call_count: 0,
            is_generating: false,
            should_quit: false,
            scroll_offset: 0,
            pending_approval: None,
            pending_ask: None,
            active_subagents: Vec::new(),
        }
    }

    pub fn with_model_preset(mut self, model_preset: impl Into<String>) -> Self {
        self.model_preset_name = model_preset.into();
        self
    }

    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let mut prev = self.cursor_pos - 1;
            while prev > 0 && !self.input.is_char_boundary(prev) {
                prev -= 1;
            }
            self.input.remove(prev);
            self.cursor_pos = prev;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let mut prev = self.cursor_pos - 1;
            while prev > 0 && !self.input.is_char_boundary(prev) {
                prev -= 1;
            }
            self.cursor_pos = prev;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            let mut next = self.cursor_pos + 1;
            while next < self.input.len() && !self.input.is_char_boundary(next) {
                next += 1;
            }
            self.cursor_pos = next;
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        let trimmed = self.input.trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub fn handle_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::TextChunk(delta) => {
                if let Some(MessageItem::Assistant { text, .. }) = self.items.last_mut() {
                    text.push_str(&delta);
                } else {
                    self.items.push(MessageItem::Assistant {
                        text: delta,
                        reasoning: None,
                    });
                }
            }
            TuiEvent::ReasoningChunk(delta) => {
                if let Some(MessageItem::Assistant { reasoning, .. }) = self.items.last_mut() {
                    if let Some(r) = reasoning {
                        r.push_str(&delta);
                    } else {
                        *reasoning = Some(delta);
                    }
                } else {
                    self.items.push(MessageItem::Assistant {
                        text: String::new(),
                        reasoning: Some(delta),
                    });
                }
            }
            TuiEvent::ToolCallStarted { name, args } => {
                self.tool_call_count += 1;
                self.items.push(MessageItem::ToolCall {
                    name,
                    args,
                    status: ToolStatus::Running,
                });
            }
            TuiEvent::ToolCallFinished {
                name,
                output,
                success,
            } => {
                for item in self.items.iter_mut().rev() {
                    if let MessageItem::ToolCall {
                        name: tname,
                        status,
                        ..
                    } = item
                    {
                        if tname == &name && *status == ToolStatus::Running {
                            *status = if success {
                                ToolStatus::Done
                            } else {
                                ToolStatus::Failed
                            };
                            break;
                        }
                    }
                }
                self.items.push(MessageItem::ToolResult { output, success });
            }
            TuiEvent::ApprovalRequested(req) => {
                self.pending_approval = Some(req);
            }
            TuiEvent::AskRequested(req) => {
                self.pending_ask = Some(req);
            }
            TuiEvent::SubagentStatus { id, role, status } => {
                if let Some(existing) = self.active_subagents.iter_mut().find(|(i, _, _)| i == &id)
                {
                    existing.2 = status;
                } else {
                    self.active_subagents.push((id, role, status));
                }
            }
            TuiEvent::TurnFinished => {
                self.is_generating = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_app_utf8_editing_and_events() {
        let mut app = TuiApp::new("model-1", "tabby", "purr_123456").with_model_preset("free");
        app.insert_char('🐾');
        app.insert_char('a');
        assert_eq!(app.input, "🐾a");
        app.move_cursor_left();
        app.backspace();
        assert_eq!(app.input, "a");
        app.move_cursor_right();
        assert_eq!(app.submit_input().as_deref(), Some("a"));
        assert!(app.input.is_empty());

        app.handle_event(TuiEvent::ReasoningChunk("thinking...".to_string()));
        app.handle_event(TuiEvent::TextChunk("Hello".to_string()));
        app.handle_event(TuiEvent::ToolCallStarted {
            name: "read".to_string(),
            args: "{}".to_string(),
        });
        assert_eq!(app.tool_call_count, 1);
        app.handle_event(TuiEvent::ToolCallFinished {
            name: "read".to_string(),
            output: "1|line".to_string(),
            success: true,
        });
        app.handle_event(TuiEvent::TurnFinished);
        assert!(!app.is_generating);
        assert_eq!(app.items.len(), 3);
    }

    #[tokio::test]
    async fn test_tui_app_approval_flow() {
        let mut app = TuiApp::new("test-model", "tabby", "sess-123");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = ApprovalRequest {
            tool_name: "bash".to_string(),
            description: "rm -rf temp".to_string(),
            diff_preview: Some("$ rm -rf temp".to_string()),
            tx,
        };

        app.handle_event(TuiEvent::ApprovalRequested(req));
        assert!(app.pending_approval.is_some());

        // Resolve approval affirmatively
        let pending = app.pending_approval.take().unwrap();
        let _ = pending.tx.send(true);
        let result = rx.await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_tui_app_ask_prompt_flow() {
        let mut app = TuiApp::new("test-model", "tabby", "sess-123");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = AskPromptRequest {
            question: "Which approach?".to_string(),
            options: vec![
                AskOptionItem {
                    id: "1".to_string(),
                    label: "Option A".to_string(),
                    description: Some("First option".to_string()),
                },
                AskOptionItem {
                    id: "2".to_string(),
                    label: "Option B".to_string(),
                    description: Some("Second option".to_string()),
                },
            ],
            selected_idx: 0,
            tx,
        };

        app.handle_event(TuiEvent::AskRequested(req));
        assert!(app.pending_ask.is_some());

        let pending = app.pending_ask.take().unwrap();
        let _ = pending.tx.send(Some("Option A".to_string()));
        let result = rx.await.unwrap();
        assert_eq!(result, Some("Option A".to_string()));
    }

    #[test]
    fn test_tui_app_cursor_movement_and_editing() {
        let mut app = TuiApp::new("test-model", "tabby", "sess-123");
        app.insert_char('a');
        app.insert_char('b');
        app.insert_char('c');
        assert_eq!(app.input, "abc");
        assert_eq!(app.cursor_pos, 3);

        app.move_cursor_left();
        assert_eq!(app.cursor_pos, 2);

        app.backspace();
        assert_eq!(app.input, "ac");
        assert_eq!(app.cursor_pos, 1);

        app.delete();
        assert_eq!(app.input, "a");
        assert_eq!(app.cursor_pos, 1);
    }
}
