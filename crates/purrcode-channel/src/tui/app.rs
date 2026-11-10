use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub enum MessageItem {
    User(String),
    Assistant { text: String, reasoning: Option<String> },
    ToolCall { name: String, args: String, status: ToolStatus },
    ToolResult { output: String, success: bool },
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

pub enum TuiEvent {
    TextChunk(String),
    ReasoningChunk(String),
    ToolCallStarted { name: String, args: String },
    ToolCallFinished { name: String, output: String, success: bool },
    ApprovalRequested(ApprovalRequest),
    TurnFinished,
}

pub struct TuiApp {
    pub input: String,
    pub cursor_pos: usize,
    pub items: Vec<MessageItem>,
    pub model_name: String,
    pub preset_name: String,
    pub session_id: String,
    pub token_usage: u64,
    pub is_generating: bool,
    pub should_quit: bool,
    pub scroll_offset: usize,
    pub pending_approval: Option<ApprovalRequest>,
}

impl TuiApp {
    pub fn new(model_name: impl Into<String>, preset_name: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            items: Vec::new(),
            model_name: model_name.into(),
            preset_name: preset_name.into(),
            session_id: session_id.into(),
            token_usage: 0,
            is_generating: false,
            should_quit: false,
            scroll_offset: 0,
            pending_approval: None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.input.remove(self.cursor_pos);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
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
                self.items.push(MessageItem::ToolCall {
                    name,
                    args,
                    status: ToolStatus::Running,
                });
            }
            TuiEvent::ToolCallFinished { name, output, success } => {
                // Update status of matching tool call
                for item in self.items.iter_mut().rev() {
                    if let MessageItem::ToolCall { name: tname, status, .. } = item {
                        if tname == &name && *status == ToolStatus::Running {
                            *status = if success { ToolStatus::Done } else { ToolStatus::Failed };
                            break;
                        }
                    }
                }
                self.items.push(MessageItem::ToolResult { output, success });
            }
            TuiEvent::ApprovalRequested(req) => {
                self.pending_approval = Some(req);
            }
            TuiEvent::TurnFinished => {
                self.is_generating = false;
            }
        }
    }
}
