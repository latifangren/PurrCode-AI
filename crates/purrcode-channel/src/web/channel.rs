// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::channel::{Channel, ChannelCapabilities};
use purrcode_api::error::Result;
use purrcode_api::event::AgentEvent;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot, RwLock};

pub struct WebChannel {
    event_tx: broadcast::Sender<AgentEvent>,
    pending_approvals: Arc<RwLock<HashMap<String, oneshot::Sender<bool>>>>,
}

impl WebChannel {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            event_tx,
            pending_approvals: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    pub async fn resolve_approval(&self, call_id: &str, approved: bool) -> bool {
        let mut map = self.pending_approvals.write().await;
        if let Some(tx) = map.remove(call_id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }
}

impl Default for WebChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for WebChannel {
    fn name(&self) -> &str {
        "web"
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            streaming: true,
            edit_message: true,
            inline_buttons: true,
        }
    }

    async fn send_event(&self, event: &AgentEvent) -> Result<()> {
        let _ = self.event_tx.send(event.clone());
        Ok(())
    }

    async fn request_approval(
        &self,
        call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> Result<bool> {
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending_approvals.write().await;
            map.insert(call_id.to_string(), tx);
        }

        // Broadcast approval requested event
        let ev = AgentEvent::ToolCallApprovalRequested {
            call_id: call_id.to_string(),
            tool: tool.to_string(),
            description: description.to_string(),
            diff_preview: preview.map(String::from),
        };
        let _ = self.event_tx.send(ev);

        // Fail-closed timeout after 180 seconds
        match tokio::time::timeout(Duration::from_secs(180), rx).await {
            Ok(Ok(decision)) => Ok(decision),
            _ => {
                // Timeout or dropped channel -> Fail-closed deny
                let mut map = self.pending_approvals.write().await;
                map.remove(call_id);
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_web_channel_event_broadcast_and_approval() {
        let ch = WebChannel::new();
        let mut rx = ch.subscribe();

        let ev = AgentEvent::TextChunk {
            text: "Hello Web".to_string(),
        };
        ch.send_event(&ev).await.unwrap();

        let received = rx.recv().await.unwrap();
        if let AgentEvent::TextChunk { text } = received {
            assert_eq!(text, "Hello Web");
        } else {
            panic!("Expected TextChunk");
        }

        // Test approval resolution
        let ch_arc = Arc::new(ch);
        let ch_clone = ch_arc.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(ch_clone.resolve_approval("call-123", true).await);
        });

        let approved = ch_arc
            .request_approval("call-123", "bash", "execute command", None)
            .await
            .unwrap();
        assert!(approved);
    }
}
