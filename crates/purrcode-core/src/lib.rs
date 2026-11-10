// SPDX-License-Identifier: GPL-3.0-or-later

pub mod loop_engine;
pub mod session;
pub mod steering;

pub use loop_engine::{AgentEventListener, AgentLoop, ApprovalRequester};
pub use session::{SessionRecord, SessionStore};
pub use steering::SteeringQueue;

use purrcode_api::event::AgentEvent;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
    history: Arc<Mutex<VecDeque<AgentEvent>>>,
    max_history: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            history: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            max_history: capacity,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }

    pub fn publish(
        &self,
        event: AgentEvent,
    ) -> Result<usize, broadcast::error::SendError<AgentEvent>> {
        if let Ok(mut hist) = self.history.lock() {
            if hist.len() >= self.max_history {
                hist.pop_front();
            }
            hist.push_back(event.clone());
        }
        self.sender.send(event)
    }

    pub fn replay(&self) -> Vec<AgentEvent> {
        self.history
            .lock()
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }
}
