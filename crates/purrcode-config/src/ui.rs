// SPDX-License-Identifier: GPL-3.0-or-later

//! User-customizable UI, chat presentation, and Telegram rendering configuration.
//! Configured via the `ui:` section in `purrcode.yaml` or `"ui": { ... }` in `purrcode.jsonc`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "warm-paper".to_string()
}

fn default_density() -> String {
    "comfortable".to_string()
}

fn default_tool_preview_lines() -> usize {
    4
}

fn default_streaming_mode() -> String {
    "live".to_string()
}

fn default_companion_name() -> String {
    "Oyen".to_string()
}

fn default_companion_personality() -> String {
    "curious".to_string()
}

fn default_statusline_fields() -> Vec<String> {
    vec![
        "model".to_string(),
        "preset".to_string(),
        "tokens".to_string(),
        "tools".to_string(),
    ]
}

fn default_auto_compact_threshold() -> f32 {
    0.85
}

fn default_parse_mode() -> String {
    "html".to_string()
}

fn default_locale() -> String {
    "en".to_string()
}

fn default_tool_verbosity() -> String {
    "normal".to_string()
}

fn default_stream_edit_ms() -> u64 {
    1200
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickCommandConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramUiConfig {
    /// `"html"` (Markdown converted to Telegram HTML) or `"plain"`.
    #[serde(default = "default_parse_mode")]
    pub parse_mode: String,

    /// `"en"` or `"id"`.
    #[serde(default = "default_locale")]
    pub locale: String,

    /// `"minimal"` (hide tool details), `"normal"` (compact one-liners), `"verbose"` (args + output).
    #[serde(default = "default_tool_verbosity")]
    pub tool_verbosity: String,

    /// Whether reasoning/thinking blocks are included in Telegram replies.
    #[serde(default)]
    pub show_reasoning: bool,

    /// Reply directly to the user's message in group chats.
    #[serde(default = "default_true")]
    pub reply_in_groups: bool,

    /// Minimum interval between progressive `editMessageText` stream updates (ms).
    #[serde(default = "default_stream_edit_ms")]
    pub stream_edit_ms: u64,

    /// Custom template overrides keyed by `"greeting"`, `"thinking"`, `"cancelled"`, `"compacted"`, etc.
    #[serde(default)]
    pub templates: HashMap<String, String>,
}

impl Default for TelegramUiConfig {
    fn default() -> Self {
        Self {
            parse_mode: default_parse_mode(),
            locale: default_locale(),
            tool_verbosity: default_tool_verbosity(),
            show_reasoning: false,
            reply_in_groups: true,
            stream_edit_ms: default_stream_edit_ms(),
            templates: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default = "default_density")]
    pub density: String,

    #[serde(default = "default_true")]
    pub show_reasoning: bool,

    #[serde(default = "default_true")]
    pub reasoning_collapsed: bool,

    #[serde(default = "default_true")]
    pub show_tool_args: bool,

    #[serde(default = "default_tool_preview_lines")]
    pub tool_preview_lines: usize,

    #[serde(default = "default_streaming_mode")]
    pub streaming_mode: String,

    #[serde(default = "default_true")]
    pub show_timestamps: bool,

    #[serde(default = "default_true")]
    pub show_token_usage: bool,

    #[serde(default = "default_companion_name")]
    pub companion_name: String,

    #[serde(default = "default_companion_personality")]
    pub companion_personality: String,

    #[serde(default = "default_statusline_fields")]
    pub statusline_fields: Vec<String>,

    #[serde(default)]
    pub pinned_files: Vec<String>,

    #[serde(default)]
    pub quick_commands: Vec<QuickCommandConfig>,

    #[serde(default = "default_auto_compact_threshold")]
    pub auto_compact_threshold: f32,

    #[serde(default)]
    pub custom_theme_tokens: HashMap<String, String>,

    #[serde(default)]
    pub telegram: TelegramUiConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            density: default_density(),
            show_reasoning: true,
            reasoning_collapsed: true,
            show_tool_args: true,
            tool_preview_lines: default_tool_preview_lines(),
            streaming_mode: default_streaming_mode(),
            show_timestamps: true,
            show_token_usage: true,
            companion_name: default_companion_name(),
            companion_personality: default_companion_personality(),
            statusline_fields: default_statusline_fields(),
            pinned_files: Vec::new(),
            quick_commands: Vec::new(),
            auto_compact_threshold: default_auto_compact_threshold(),
            custom_theme_tokens: HashMap::new(),
            telegram: TelegramUiConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Config;

    #[test]
    fn ui_config_defaults_and_yaml_overrides() {
        let yaml = r#"
preset: tabby
ui:
  theme: catppuccin-mocha
  density: compact
  show_reasoning: false
  tool_preview_lines: 12
  companion_name: Mochi
  quick_commands:
    - name: review
      description: Audit staged diff
      template: "Review git diff carefully"
  telegram:
    parse_mode: html
    locale: id
    tool_verbosity: minimal
    show_reasoning: true
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.ui.theme, "catppuccin-mocha");
        assert_eq!(cfg.ui.density, "compact");
        assert!(!cfg.ui.show_reasoning);
        assert_eq!(cfg.ui.tool_preview_lines, 12);
        assert_eq!(cfg.ui.companion_name, "Mochi");
        assert_eq!(cfg.ui.quick_commands.len(), 1);
        assert_eq!(cfg.ui.quick_commands[0].name, "review");
        assert_eq!(cfg.ui.telegram.locale, "id");
        assert_eq!(cfg.ui.telegram.tool_verbosity, "minimal");
        assert!(cfg.ui.telegram.show_reasoning);
        assert!(cfg.ui.telegram.reply_in_groups);
    }
}
