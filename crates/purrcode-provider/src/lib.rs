// SPDX-License-Identifier: GPL-3.0-or-later

pub mod openai;
pub mod router;

pub use openai::OpenAiCompatibleProvider;
use purrcode_api::provider::ModelProvider;
use purrcode_config::Config;
pub use router::FallbackRouter;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: String, provider: Arc<dyn ModelProvider>) {
        self.providers.insert(name, provider);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.get(name).cloned()
    }

    /// Builds a ProviderRegistry dynamically from loaded PurrCode configuration.
    /// Zero hardcoded URLs: every provider endpoint comes from `purrcode.jsonc`.
    pub fn from_config(config: &Config) -> Self {
        let mut registry = Self::new();

        for (provider_name, provider_cfg) in &config.providers {
            if let Some(ref base_url) = provider_cfg.base_url {
                let provider = OpenAiCompatibleProvider::new(
                    provider_name.clone(),
                    base_url.clone(),
                    provider_cfg.api_key.clone(),
                    provider_cfg.headers.clone(),
                    provider_cfg.user_agent.clone(),
                );
                registry.register(provider_name.clone(), Arc::new(provider));
            }
        }

        registry
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
