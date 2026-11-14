// SPDX-License-Identifier: GPL-3.0-or-later

pub mod telegram;
pub mod tui;
pub mod web;

pub use tui::{
    ApprovalRequest, AskPromptRequest, MessageItem, ToolStatus, TuiApp, TuiEvent, TuiRunner,
};
pub use web::{
    is_valid_web_session_id, new_web_session_id, web_index_html, PromptDispatcher, WebChannel,
    WebServer, WebSession, WebSessionInfo, WebSessionRegistry, WebSkillDraftRequester,
    DEFAULT_WEB_SESSION_ID,
};

use purrcode_api::channel::Channel;
use std::sync::Arc;

pub struct ChannelRegistry {
    channels: Vec<Arc<dyn Channel>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }

    pub fn register(&mut self, channel: Arc<dyn Channel>) {
        self.channels.push(channel);
    }

    pub fn channels(&self) -> &[Arc<dyn Channel>] {
        &self.channels
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}
