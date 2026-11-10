// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Thread-safe FIFO steering queue for follow-up user messages sent mid-turn.
#[derive(Clone, Default)]
pub struct SteeringQueue {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl SteeringQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn push(&self, message: impl Into<String>) {
        if let Ok(mut q) = self.inner.lock() {
            q.push_back(message.into());
        }
    }

    pub fn pop(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|mut q| q.pop_front())
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().map(|q| q.is_empty()).unwrap_or(true)
    }
}
