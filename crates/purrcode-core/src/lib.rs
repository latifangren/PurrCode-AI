use purrcode_api::event::AgentEvent;
use tokio::sync::broadcast;

pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: AgentEvent) -> Result<usize, broadcast::error::SendError<AgentEvent>> {
        self.sender.send(event)
    }
}
