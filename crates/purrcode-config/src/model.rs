// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_preset")]
    pub preset: String,

    pub model_preset: Option<String>,

    pub model: Option<String>,

    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    #[serde(default, alias = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

fn default_preset() -> String {
    "tabby".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: default_preset(),
            model_preset: None,
            model: None,
            providers: HashMap::new(),
            mcp_servers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_api_type")]
    pub api: String,

    pub base_url: Option<String>,

    pub api_key: Option<String>,

    #[serde(default)]
    pub headers: HashMap<String, String>,

    #[serde(default)]
    pub models: HashMap<String, ModelConfig>,
}

fn default_api_type() -> String {
    "openai-completions".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: Option<String>,

    #[serde(default = "default_context_window")]
    pub context_window: u64,

    pub max_tokens: Option<u64>,

    #[serde(default)]
    pub reasoning: bool,

    pub reasoning_effort: Option<String>,
}

fn default_context_window() -> u64 {
    131072
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            name: None,
            context_window: default_context_window(),
            max_tokens: None,
            reasoning: false,
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: Option<String>,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    pub url: Option<String>,

    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl Config {
    /// Find provider and model details for a given model string (format: "provider:model" or "provider/model")
    pub fn resolve_model<'a>(
        &'a self,
        target_model: &str,
    ) -> Option<(&'a str, &'a str, &'a ProviderConfig, &'a ModelConfig)> {
        // 1. Try matching provider:model prefix
        if let Some(colon_idx) = target_model.find(':') {
            let p_key = &target_model[..colon_idx];
            let suffix_model = &target_model[colon_idx + 1..];
            if let Some((p_name, provider)) = self.providers.get_key_value(p_key) {
                if let Some((m_key, model)) = provider.models.get_key_value(suffix_model) {
                    return Some((p_name.as_str(), m_key.as_str(), provider, model));
                }
            }
        }

        // 2. Try matching provider/model prefix
        if let Some(slash_idx) = target_model.find('/') {
            let p_key = &target_model[..slash_idx];
            let suffix_model = &target_model[slash_idx + 1..];
            if let Some((p_name, provider)) = self.providers.get_key_value(p_key) {
                if let Some((m_key, model)) = provider.models.get_key_value(suffix_model) {
                    return Some((p_name.as_str(), m_key.as_str(), provider, model));
                }
            }
        }

        // 3. Search through all providers for matching model key, in
        // deterministic (lexicographic) provider order. A bare model name
        // shared by several providers always resolves to the same one.
        let mut provider_names: Vec<&String> = self.providers.keys().collect();
        provider_names.sort();
        for provider_name in provider_names {
            let provider = &self.providers[provider_name];
            if let Some((m_key, model)) = provider.models.get_key_value(target_model) {
                return Some((provider_name.as_str(), m_key.as_str(), provider, model));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_model(model_key: &str) -> ProviderConfig {
        let mut models = HashMap::new();
        models.insert(model_key.to_string(), ModelConfig::default());
        ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: None,
            api_key: None,
            headers: HashMap::new(),
            models,
        }
    }

    #[test]
    fn test_resolve_model_bare_name_is_deterministic_across_providers() {
        let mut cfg = Config::default();
        cfg.providers
            .insert("zeta".to_string(), provider_with_model("shared-model"));
        cfg.providers
            .insert("alpha".to_string(), provider_with_model("shared-model"));
        // HashMap iteration order is randomized per process; the same
        // lookup must resolve to the same provider every time.
        for _ in 0..20 {
            let (provider, model, _, _) = cfg
                .resolve_model("shared-model")
                .expect("shared model must resolve");
            assert_eq!(provider, "alpha");
            assert_eq!(model, "shared-model");
        }
    }
}
