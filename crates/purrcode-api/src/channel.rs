use crate::error::Result;
use crate::event::AgentEvent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    pub streaming: bool,
    pub edit_message: bool,
    pub inline_buttons: bool,
}

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> ChannelCapabilities;
    async fn send_event(&self, event: &AgentEvent) -> Result<()>;
    async fn request_approval(
        &self,
        call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> Result<bool>;
}
