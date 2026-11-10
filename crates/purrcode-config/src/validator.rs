// SPDX-License-Identifier: GPL-3.0-or-later

use crate::model::Config;
use purrcode_api::error::{PurrError, Result};

/// Minimum required context window for multi-step agent loop (64K tokens)
pub const MIN_CONTEXT_WINDOW: u64 = 65_536;

/// Validates configuration constraints including the 64K minimum context window gate.
pub fn validate_config(config: &Config) -> Result<()> {
    for (p_name, p_cfg) in &config.providers {
        for (m_id, m_cfg) in &p_cfg.models {
            if m_cfg.context_window < MIN_CONTEXT_WINDOW {
                return Err(PurrError::Config(format!(
                    "Model '{}:{}' has context_window = {} tokens, which is below the required 64K ({}) minimum headroom for PurrCode's multi-step agent loop.",
                    p_name, m_id, m_cfg.context_window, MIN_CONTEXT_WINDOW
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelConfig, ProviderConfig};
    use std::collections::HashMap;

    #[test]
    fn test_rejects_model_under_64k() {
        let mut models = HashMap::new();
        models.insert(
            "tiny-model".to_string(),
            ModelConfig {
                name: Some("tiny".to_string()),
                context_window: 32_768,
                max_tokens: None,
                reasoning: false,
                reasoning_effort: None,
            },
        );
        let mut providers = HashMap::new();
        providers.insert(
            "test-provider".to_string(),
            ProviderConfig {
                api: "openai-completions".to_string(),
                base_url: None,
                api_key: None,
                headers: HashMap::new(),
                models,
            },
        );
        let cfg = Config {
            preset: "tabby".to_string(),
            model_preset: None,
            model: Some("test-provider:tiny-model".to_string()),
            providers,
            mcp_servers: HashMap::new(),
        };
        assert!(validate_config(&cfg).is_err());
    }
}
