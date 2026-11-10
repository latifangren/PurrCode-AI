pub mod tui;

pub use tui::{MessageItem, ToolStatus, TuiApp, TuiRunner};

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
