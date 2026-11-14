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

    /// Cron daemon settings (v0.3, ADR-014). Absent = cron disabled.
    /// Job definitions live in a dedicated store, NOT here:
    /// `~/.purrcode/cron/jobs.jsonc` (see `purrcode-cli/src/cron/`).
    /// This section only tunes the daemon (tick interval, on/off).
    #[serde(default)]
    pub cron: Option<CronConfig>,

    /// Heartbeat daemon settings (v0.3 F22, ADR-014 item 6). Absent =
    /// enabled (the tick is a no-op without a `HEARTBEAT.md` task
    /// file, so an absent section costs no LLM call). The task file
    /// itself lives outside config: `.purrcode/HEARTBEAT.md` in the
    /// workspace, else `~/.purrcode/HEARTBEAT.md` (see
    /// `purrcode-cli/src/heartbeat/`).
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
}

/// Cron daemon settings (PRD F19, ADR-014).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    /// Master switch for the cron tick inside the gateway daemon.
    /// Default true when the section is present (jobs still need to exist
    /// and be enabled individually to fire).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Seconds between due-checks in the gateway tick. Default 60
    /// (minute granularity — cron expressions are minute-resolved).
    /// Minimum 5 (ZeroClaw's MIN_POLL_SECONDS).
    #[serde(default)]
    pub tick_secs: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl CronConfig {
    pub fn tick_secs(&self) -> u64 {
        self.tick_secs.unwrap_or(60).max(5)
    }
}

/// Heartbeat daemon settings (PRD F22, ADR-014 item 6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Master switch for the heartbeat tick inside the gateway daemon.
    /// Default true when the section is present (and when it is absent:
    /// the tick is a no-op without a `HEARTBEAT.md` task file, so an
    /// absent section costs no LLM call).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Minutes between heartbeat ticks. Default 30 (PicoClaw/ZeroClaw
    /// default). Minimum 5 (PicoClaw's minimum).
    #[serde(default)]
    pub interval_mins: Option<u64>,
}

impl HeartbeatConfig {
    pub fn interval_mins(&self) -> u64 {
        self.interval_mins.unwrap_or(30).max(5)
    }
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
            cron: None,
            heartbeat: None,
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

static DEFAULT_MODEL: ModelConfig = ModelConfig {
    name: None,
    context_window: 131_072,
    max_tokens: None,
    reasoning: false,
    reasoning_effort: None,
};

impl Config {
    /// Find provider and model details for a given model string (format: "provider:model" or "provider/model")
    pub fn resolve_model<'a>(
        &'a self,
        target_model: &'a str,
    ) -> Option<(&'a str, &'a str, &'a ProviderConfig, &'a ModelConfig)> {
        // 1. Try matching provider:model prefix
        if let Some(colon_idx) = target_model.find(':') {
            let p_key = &target_model[..colon_idx];
            let suffix_model = &target_model[colon_idx + 1..];
            if let Some((p_name, provider)) = self.providers.get_key_value(p_key) {
                if let Some((m_key, model)) = provider.models.get_key_value(suffix_model) {
                    return Some((p_name.as_str(), m_key.as_str(), provider, model));
                }
                return Some((p_name.as_str(), suffix_model, provider, &DEFAULT_MODEL));
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
                return Some((p_name.as_str(), suffix_model, provider, &DEFAULT_MODEL));
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

        // 4. If single provider configured, fallback for bare model name
        if self.providers.len() == 1 {
            let (p_name, provider) = self.providers.iter().next().unwrap();
            return Some((p_name.as_str(), target_model, provider, &DEFAULT_MODEL));
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

    #[test]
    fn test_cron_config_defaults() {
        // No cron section at all -> cron disabled.
        let cfg: Config = serde_json::from_str("{}").expect("empty config parses");
        assert!(cfg.cron.is_none());

        // Explicit values win over defaults; tick clamps to a 5s minimum.
        let cfg: Config = serde_json::from_str(r#"{"cron": {"enabled": true, "tick_secs": 30}}"#)
            .expect("cron section parses");
        let cron = cfg.cron.expect("cron present");
        assert!(cron.enabled);
        assert_eq!(cron.tick_secs(), 30);

        let cfg: Config = serde_json::from_str(r#"{"cron": {"enabled": true, "tick_secs": 1}}"#)
            .expect("cron section parses");
        assert_eq!(cfg.cron.expect("cron present").tick_secs(), 5);

        // Defaults: enabled true when the section exists, tick 60s.
        let cfg: Config = serde_json::from_str(r#"{"cron": {}}"#).expect("cron parses");
        let cron = cfg.cron.expect("cron present");
        assert!(cron.enabled);
        assert_eq!(cron.tick_secs(), 60);
    }

    #[test]
    fn test_resolve_model_fallback_to_default_model_when_undeclared() {
        let mut cfg = Config::default();
        let provider = ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: Some("https://example.com/v1".to_string()),
            api_key: None,
            headers: HashMap::new(),
            user_agent: None,
            models: HashMap::new(), // Empty models!
        };
        cfg.providers.insert("magpie".to_string(), provider);

        // provider:model syntax with undeclared model
        let (p_name, m_name, _, m_cfg) = cfg
            .resolve_model("magpie:antigravity/gemini-3.8-flash-tiered")
            .expect("should resolve with fallback");
        assert_eq!(p_name, "magpie");
        assert_eq!(m_name, "antigravity/gemini-3.8-flash-tiered");
        assert_eq!(m_cfg.context_window, 131_072);

        // provider/model syntax with undeclared model
        let (p_name, m_name, _, m_cfg) = cfg
            .resolve_model("magpie/custom-model")
            .expect("should resolve with fallback");
        assert_eq!(p_name, "magpie");
        assert_eq!(m_name, "custom-model");
        assert_eq!(m_cfg.context_window, 131_072);

        // bare model syntax with single provider
        let (p_name, m_name, _, m_cfg) = cfg
            .resolve_model("bare-model")
            .expect("single provider should resolve bare model");
        assert_eq!(p_name, "magpie");
        assert_eq!(m_name, "bare-model");
        assert_eq!(m_cfg.context_window, 131_072);
    }
}
