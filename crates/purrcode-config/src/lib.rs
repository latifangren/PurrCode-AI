use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_preset")]
    pub preset: String,
    pub model_preset: Option<String>,
    #[serde(default)]
    pub providers: HashMap<String, ProviderEntry>,
}

fn default_preset() -> String {
    "tabby".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    #[serde(default = "default_api_type")]
    pub api: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: HashMap<String, ModelEntry>,
}

fn default_api_type() -> String {
    "openai-completions".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: Option<String>,
    #[serde(default = "default_context_window")]
    pub context_window: u64,
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: bool,
}

fn default_context_window() -> u64 {
    131072
}
