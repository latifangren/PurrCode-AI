// SPDX-License-Identifier: GPL-3.0-or-later

pub mod telegram;
pub mod tui;
pub mod web;

pub use telegram::{
    split_message, InlineKeyboardButton, InlineKeyboardMarkup, TelegramChannel,
    TelegramMessageHandler, TgClient, TgMessage, TgUpdate, TgUser,
};
pub use tui::{
    ApprovalRequest, AskPromptRequest, MessageItem, ToolStatus, TuiApp, TuiEvent, TuiRunner,
};
pub use web::{WebChannel, WebServer, WEB_INDEX_HTML};

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
