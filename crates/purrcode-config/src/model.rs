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

    /// Telegram gateway settings (v0.2, ADR-013). Absent = gateway disabled.
    /// The bot token must come from the environment / `~/.purrcode/credentials`
    /// via `${TELEGRAM_BOT_TOKEN}` — never commit a literal token here.
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

/// Telegram gateway configuration (PRD F15–F18, ADR-013).
/// `allow_from` is fail-closed: an empty list authorizes NOBODY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot token, normally `"${TELEGRAM_BOT_TOKEN}"` (expanded from env or
    /// `~/.purrcode/credentials` by `expand_env_vars`).
    #[serde(default)]
    pub bot_token: Option<String>,

    /// Chat IDs allowed to talk to the bot. Empty = nobody allowed.
    #[serde(default)]
    pub allow_from: Vec<i64>,

    /// Bot API base URL. Default `https://api.telegram.org`. Overridable
    /// for local mock/testing servers. NOTE: SSRF IP-pinning deliberately
    /// does NOT apply here (ADR-013): no model-controlled URL ever
    /// reaches the client, and pinning would break self-hosted Bot API
    /// servers on LAN IPs and the local mock. Only the scheme
    /// (http/https) is validated by the client.
    #[serde(default)]
    pub api_base: Option<String>,

    /// Approval card timeout in seconds. Default 120; expiry denies
    /// (fail-closed, ADR-013).
    #[serde(default)]
    pub approval_timeout_secs: Option<u64>,

    /// Long-poll timeout in seconds for `getUpdates`. Default 30.
    #[serde(default)]
    pub poll_timeout_secs: Option<u32>,
}

impl TelegramConfig {
    pub fn api_base(&self) -> &str {
        self.api_base
            .as_deref()
            .unwrap_or("https://api.telegram.org")
    }

    pub fn approval_timeout_secs(&self) -> u64 {
        self.approval_timeout_secs.unwrap_or(120)
    }

    pub fn poll_timeout_secs(&self) -> u32 {
        self.poll_timeout_secs.unwrap_or(30)
    }
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
            telegram: None,
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

    /// Optional HTTP User-Agent sent to this provider. When absent, the
    /// PurrCode default (DEFAULT_USER_AGENT in purrcode-provider) is used.
    /// Some gateways (e.g. behind a strict Cloudflare WAF) reject
    /// non-browser user agents — set a browser UA here for those endpoints.
    #[serde(default)]
    pub user_agent: Option<String>,

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
            user_agent: None,
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

    #[test]
    fn test_telegram_config_defaults_and_fail_closed_allowlist() {
        // No telegram section at all -> gateway disabled.
        let cfg: Config = serde_json::from_str("{}").expect("empty config parses");
        assert!(cfg.telegram.is_none());

        // Empty allow_from deserializes fine and means NOBODY (fail-closed,
        // ADR-013) — it must never be read as "allow everyone".
        let cfg: Config = serde_json::from_str(
            r#"{"telegram": {"bot_token": "${TELEGRAM_BOT_TOKEN}", "allow_from": []}}"#,
        )
        .expect("telegram section parses");
        let tg = cfg.telegram.expect("telegram present");
        assert_eq!(tg.allow_from.len(), 0);
        assert_eq!(tg.bot_token.as_deref(), Some("${TELEGRAM_BOT_TOKEN}"));
        assert_eq!(tg.api_base(), "https://api.telegram.org");
        assert_eq!(tg.approval_timeout_secs(), 120);
        assert_eq!(tg.poll_timeout_secs(), 30);

        // Explicit values win over defaults.
        let cfg: Config = serde_json::from_str(
            r#"{"telegram": {"bot_token": "x", "allow_from": [123, -456], "api_base": "http://127.0.0.1:9", "approval_timeout_secs": 60, "poll_timeout_secs": 10}}"#,
        )
        .expect("explicit telegram section parses");
        let tg = cfg.telegram.expect("telegram present");
        assert_eq!(tg.allow_from, vec![123, -456]);
        assert_eq!(tg.api_base(), "http://127.0.0.1:9");
        assert_eq!(tg.approval_timeout_secs(), 60);
        assert_eq!(tg.poll_timeout_secs(), 10);
    }
}
