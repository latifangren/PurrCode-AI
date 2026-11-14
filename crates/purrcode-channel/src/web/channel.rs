// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::channel::{Channel, ChannelCapabilities};
use purrcode_api::error::Result;
use purrcode_api::event::AgentEvent;
use purrcode_api::tool::AuditSink;
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

    /// Register a pending approval and wait for its resolution. Shared
    /// by tool approvals (keyed by the loop's call id) and skill-draft
    /// approvals (keyed `skill-draft:<name>`).
    async fn register_pending(&self, key: &str) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending_approvals.write().await;
            map.insert(key.to_string(), tx);
        }
        rx
    }

    /// Build the requester the curator hook uses for staged skill
    /// drafts (F20). The hook runs outside the loop, so — unlike tool
    /// approvals — no bus event announces the draft; the requester
    /// broadcasts its own card (see `WebSkillDraftRequester`).
    /// Takes `&Arc<Self>` so the requester holds the same shared
    /// channel the server and the forwarder use.
    pub fn skill_draft_requester(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> WebSkillDraftRequester {
        WebSkillDraftRequester::new(self.clone(), session_id, audit_sink)
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
        _tool: &str,
        _description: &str,
        _preview: Option<&str>,
    ) -> Result<bool> {
        // NOTE (R3 deviation from the dev-claude reference): this does
        // NOT broadcast ToolCallApprovalRequested. The loop publishes
        // that event to the EventBus with the same call id *before*
        // calling the requester, and the `purrcode web` forwarder sends
        // every bus event to this channel — broadcasting here would
        // render the approval card twice. A caller that uses this
        // channel WITHOUT a bus-wired loop must announce the request
        // itself (see WebSkillDraftRequester, which does exactly that
        // for curator drafts).
        let rx = self.register_pending(call_id).await;

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

/// Key prefix for skill-draft approvals in `pending_approvals`, so one
/// map and one resolve endpoint serve both tool approvals (loop call
/// ids) and curator drafts. The UI posts the same `/api/approval`
/// body for both; `/api/skills/approve` is a convenience alias.
pub const SKILL_DRAFT_KEY_PREFIX: &str = "skill-draft:";

/// Approval requester for staged curator drafts (F20), mirroring
/// Telegram's `SkillDraftRequester`: 120s → deny, audited as
/// `web_skill_approval`. Unlike tool approvals it announces its own
/// card, because the curator hook runs outside the loop and no bus
/// event carries the draft.
pub struct WebSkillDraftRequester {
    channel: Arc<WebChannel>,
    session_id: String,
    timeout: Duration,
    audit_sink: Option<Arc<dyn AuditSink>>,
}

impl WebSkillDraftRequester {
    pub fn new(
        channel: Arc<WebChannel>,
        session_id: impl Into<String>,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        Self {
            channel,
            session_id: session_id.into(),
            timeout: Duration::from_secs(120),
            audit_sink,
        }
    }

    fn audit(&self, decision: &str, detail: &str) {
        if let Some(sink) = &self.audit_sink {
            sink.record_event(
                &self.session_id,
                "web",
                "web_skill_approval",
                Some("skill_draft"),
                decision,
                detail,
            );
        }
    }

    /// Ask the user to approve saving the staged draft `name`
    /// (`description` is the draft's one-line description, `preview`
    /// the SKILL.md body). Returns true on approve, false on deny,
    /// timeout, or an undeliverable card (fail-closed).
    pub async fn request_approval(
        &self,
        name: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool {
        let key = format!("{}{}", SKILL_DRAFT_KEY_PREFIX, name);
        let rx = self.channel.register_pending(&key).await;

        let ev = AgentEvent::ToolCallApprovalRequested {
            call_id: key.clone(),
            tool: "manage_skill".to_string(),
            description: description.to_string(),
            diff_preview: preview.map(String::from),
        };
        let _ = self.channel.send_event(&ev).await;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(decision)) => {
                self.audit(
                    if decision {
                        "approved_by_user"
                    } else {
                        "denied_by_user"
                    },
                    name,
                );
                decision
            }
            _ => {
                let mut map = self.channel.pending_approvals.write().await;
                map.remove(&key);
                self.audit("denied_timeout", name);
                false
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

    /// R3 invariant: `request_approval` must NOT broadcast
    /// ToolCallApprovalRequested — the loop already publishes that
    /// event to the EventBus with the same call id, and the `purrcode
    /// web` forwarder sends it to this channel. Broadcasting here
    /// would render the approval card twice in the WebUI.
    #[tokio::test]
    async fn test_web_channel_request_approval_does_not_broadcast() {
        let ch = Arc::new(WebChannel::new());
        let mut rx = ch.subscribe();

        let ch_clone = ch.clone();
        let waiter = tokio::spawn(async move {
            ch_clone
                .request_approval("call-abc", "bash", "execute command", None)
                .await
                .unwrap()
        });
        // Let the request register, then resolve it from the "UI" side.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ch.resolve_approval("call-abc", true).await);
        assert!(waiter.await.unwrap());

        // Nothing was broadcast: the approval card comes from the bus
        // event, not from the requester.
        assert!(rx.try_recv().is_err());
    }

    /// The skill-draft requester DOES announce its own card (the
    /// curator hook runs outside the loop, so no bus event carries
    /// the draft), keyed `skill-draft:<name>`, resolvable through the
    /// same `/api/approval` path as tool approvals.
    #[tokio::test]
    async fn test_web_skill_draft_requester_announces_and_resolves() {
        let ch = Arc::new(WebChannel::new());
        let mut rx = ch.subscribe();
        let requester = ch.skill_draft_requester("s_web", None);

        let ch_clone = ch.clone();
        let waiter = tokio::spawn(async move {
            requester
                .request_approval("my-skill", "A drafted skill", Some("body"))
                .await
        });

        // The card event arrives on the broadcast with the draft key.
        let ev = rx.recv().await.unwrap();
        match ev {
            AgentEvent::ToolCallApprovalRequested {
                call_id,
                tool,
                description,
                ..
            } => {
                assert_eq!(call_id, "skill-draft:my-skill");
                assert_eq!(tool, "manage_skill");
                assert_eq!(description, "A drafted skill");
            }
            other => panic!("Expected ToolCallApprovalRequested, got {other:?}"),
        }

        assert!(
            ch_clone
                .resolve_approval("skill-draft:my-skill", true)
                .await
        );
        assert!(waiter.await.unwrap());
    }
}
