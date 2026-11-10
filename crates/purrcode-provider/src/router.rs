// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{CompletionRequest, ModelProvider, StreamChunk};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wraps a primary provider and fallback chain.
/// Technical errors (timeout, 5xx, 429 quota) trigger cooldown and advance to the next candidate.
/// Policy refusals (`PurrError::ProviderRefusal`) immediately halt the chain without retry!
pub struct FallbackRouter {
    name: String,
    candidates: Vec<(Arc<dyn ModelProvider>, String)>,
    cooldowns: Arc<Mutex<HashMap<usize, Instant>>>,
    cooldown_duration: Duration,
}

impl FallbackRouter {
    pub fn new(name: impl Into<String>, candidates: Vec<(Arc<dyn ModelProvider>, String)>) -> Self {
        Self {
            name: name.into(),
            candidates,
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            cooldown_duration: Duration::from_secs(30),
        }
    }

    pub fn with_cooldown(mut self, duration: Duration) -> Self {
        self.cooldown_duration = duration;
        self
    }

    fn is_in_cooldown(&self, idx: usize) -> bool {
        if let Ok(mut map) = self.cooldowns.lock() {
            if let Some(until) = map.get(&idx) {
                if Instant::now() < *until {
                    return true;
                }
                map.remove(&idx);
            }
        }
        false
    }

    fn mark_cooldown(&self, idx: usize) {
        if let Ok(mut map) = self.cooldowns.lock() {
            map.insert(idx, Instant::now() + self.cooldown_duration);
        }
    }
}

#[async_trait]
impl ModelProvider for FallbackRouter {
    fn provider_name(&self) -> &str {
        &self.name
    }

    fn is_healthy(&self) -> bool {
        !self.candidates.is_empty()
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        let mut last_err: Option<PurrError> = None;

        for (idx, (provider, wire_model)) in self.candidates.iter().enumerate() {
            if self.is_in_cooldown(idx) {
                continue;
            }

            let mut candidate_req = request.clone();
            if !wire_model.is_empty() {
                candidate_req.model = wire_model.clone();
            }

            match provider.stream_completion(candidate_req).await {
                Ok(rx) => return Ok(rx),
                Err(err @ PurrError::ProviderRefusal { .. }) => {
                    // NEVER fallback on a policy refusal (ARCHITECTURE §7.3)
                    return Err(err);
                }
                Err(err) => {
                    self.mark_cooldown(idx);
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| PurrError::Provider {
            provider: self.name.clone(),
            message: "All fallback providers exhausted or in cooldown".to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockRefusalProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ModelProvider for MockRefusalProvider {
        fn provider_name(&self) -> &str {
            "mock-refusal"
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stream_completion(
            &self,
            _req: CompletionRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(PurrError::ProviderRefusal {
                provider: "mock-refusal".to_string(),
                reason: "Content policy violation".to_string(),
            })
        }
    }

    struct MockOkProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ModelProvider for MockOkProvider {
        fn provider_name(&self) -> &str {
            "mock-ok"
        }
        fn is_healthy(&self) -> bool {
            true
        }
        async fn stream_completion(
            &self,
            _req: CompletionRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            let _ = tx.try_send(StreamChunk {
                delta: "ok".to_string(),
                reasoning_delta: None,
                tool_call_delta: None,
                is_finished: true,
                usage: None,
            });
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn test_policy_refusal_stops_fallback_chain() {
        let c1 = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::new(AtomicUsize::new(0));

        let p1: Arc<dyn ModelProvider> = Arc::new(MockRefusalProvider { calls: c1.clone() });
        let p2: Arc<dyn ModelProvider> = Arc::new(MockOkProvider { calls: c2.clone() });

        let router = FallbackRouter::new("chain", vec![(p1, "m1".into()), (p2, "m2".into())]);
        let req = CompletionRequest {
            model: "m1".to_string(),
            messages: Vec::new(),
            tools: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            stop: None,
        };

        let res = router.stream_completion(req).await;
        assert!(matches!(res, Err(PurrError::ProviderRefusal { .. })));
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        // Second provider MUST NOT be called when first returns ProviderRefusal
        assert_eq!(c2.load(Ordering::SeqCst), 0);
    }
}
