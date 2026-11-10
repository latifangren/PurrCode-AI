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
}
