// SPDX-License-Identifier: GPL-3.0-or-later

use crate::jsonc::clean_jsonc;
use crate::model::{Config, McpServerConfig};
use crate::validator::validate_config;
use purrcode_api::error::{PurrError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Interpolates `${VAR}` and `$VAR` from environment variables and an optional credentials map
pub fn expand_env_vars(input: &str) -> String {
    let creds = read_credentials_map();
    expand_env_vars_with_map(input, &creds)
}

pub fn expand_env_vars_with_map(input: &str, creds: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' && i + 1 < len {
            if chars[i + 1] == '{' {
                let start = i + 2;
                let mut end = start;
                while end < len && chars[end] != '}' {
                    end += 1;
                }
                if end < len {
                    let var_name: String = chars[start..end].iter().collect();
                    if let Ok(val) = std::env::var(&var_name) {
                        result.push_str(&val);
                    } else if let Some(val) = creds.get(&var_name) {
                        result.push_str(val);
                    }
                    i = end + 1;
                    continue;
                }
            } else if chars[i + 1].is_ascii_alphanumeric() || chars[i + 1] == '_' {
                let start = i + 1;
                let mut end = start;
                while end < len && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let var_name: String = chars[start..end].iter().collect();
                if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
                } else if let Some(val) = creds.get(&var_name) {
                    result.push_str(val);
                }
                i = end;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

#[derive(Deserialize, Default)]
struct StandaloneMcpFile {
    #[serde(rename = "mcpServers", alias = "mcp_servers", default)]
    mcp_servers: HashMap<String, McpServerConfig>,
}

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn find_config_path() -> Option<PathBuf> {
        let local_sub_candidates = [
            ".purrcode/purrcode.yaml",
            ".purrcode/purrcode.yml",
            ".purrcode/purrcode.jsonc",
            ".purrcode/purrcode.json",
        ];
        for rel in &local_sub_candidates {
            let p = Path::new(rel);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }

        let local_root_candidates = [
            "purrcode.yaml",
            "purrcode.yml",
            "purrcode.jsonc",
            "purrcode.json",
        ];
        for rel in &local_root_candidates {
            let p = Path::new(rel);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }

        if let Some(home) = dirs_home() {
            let global_candidates = [
                home.join(".purrcode").join("purrcode.yaml"),
                home.join(".purrcode").join("purrcode.yml"),
                home.join(".purrcode").join("purrcode.jsonc"),
                home.join(".purrcode").join("purrcode.json"),
            ];
            for p in &global_candidates {
                if p.exists() {
                    return Some(p.clone());
                }
            }
        }

        None
    }

    pub fn load_from_yaml_str(content: &str) -> Result<Config> {
        let expanded = expand_env_vars(content);
        let cfg: Config = serde_yaml::from_str(&expanded)
            .map_err(|e| PurrError::Config(format!("Failed to parse config YAML: {e}")))?;
        validate_config(&cfg)?;
        Ok(cfg)
    }

    pub fn load_from_str(content: &str) -> Result<Config> {
        let trimmed = content.trim();
        // If content clearly looks like JSON (starts with '{'), prioritize JSONC parser.
        let cleaned = clean_jsonc(content);
        let expanded = expand_env_vars(&cleaned);

        if trimmed.starts_with('{') {
            let cfg: Config = serde_json::from_str(&expanded)
                .map_err(|e| PurrError::Config(format!("Failed to parse config JSONC: {e}")))?;
            validate_config(&cfg)?;
            return Ok(cfg);
        }

        // Otherwise try YAML first with environment variable interpolation
        let expanded_yaml = expand_env_vars(content);
        match serde_yaml::from_str::<Config>(&expanded_yaml) {
            Ok(cfg) => {
                validate_config(&cfg)?;
                Ok(cfg)
            }
            Err(yaml_err) => {
                // If YAML failed, fallback to JSONC parser
                match serde_json::from_str::<Config>(&expanded) {
                    Ok(cfg) => {
                        validate_config(&cfg)?;
                        Ok(cfg)
                    }
                    Err(json_err) => Err(PurrError::Config(format!(
                        "Failed to parse config (JSONC: {json_err}; YAML: {yaml_err})"
                    ))),
                }
            }
        }
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Config> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            PurrError::Config(format!("Cannot read config file '{}': {e}", path.display()))
        })?;
        let is_yaml = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
            .unwrap_or(false);

        if is_yaml {
            Self::load_from_yaml_str(&content)
        } else {
            Self::load_from_str(&content)
        }
    }

    /// Loads and hierarchically merges configuration layers:
    /// system -> global (`~/.purrcode/purrcode.yaml` / `.jsonc`) -> project (`purrcode.yaml` / `.jsonc` / `.purrcode/`)
    /// plus standalone `mcp.json` files (`~/.purrcode/mcp.json` and `.purrcode/mcp.json`).
    pub fn load_effective() -> Result<Config> {
        let mut effective = Config::default();

        let find_first = |candidates: &[PathBuf]| -> Option<PathBuf> {
            for p in candidates {
                if p.exists() {
                    return Some(p.clone());
                }
            }
            None
        };

        // 1. System layer (Linux/macOS `/etc/purrcode/purrcode.yaml` or `.jsonc`)
        #[cfg(not(windows))]
        {
            let sys_candidates = [
                PathBuf::from("/etc/purrcode/purrcode.yaml"),
                PathBuf::from("/etc/purrcode/purrcode.yml"),
                PathBuf::from("/etc/purrcode/purrcode.jsonc"),
                PathBuf::from("/etc/purrcode/purrcode.json"),
            ];
            if let Some(sys_path) = find_first(&sys_candidates) {
                if let Ok(sys_cfg) = Self::load_from_file(&sys_path) {
                    Self::merge_into(&mut effective, sys_cfg);
                }
            }
        }

        // 2. Global user layer (`~/.purrcode/purrcode.yaml` / `.jsonc` + `~/.purrcode/mcp.json`)
        if let Some(home) = dirs_home() {
            let global_candidates = [
                home.join(".purrcode").join("purrcode.yaml"),
                home.join(".purrcode").join("purrcode.yml"),
                home.join(".purrcode").join("purrcode.jsonc"),
                home.join(".purrcode").join("purrcode.json"),
            ];
            if let Some(global_path) = find_first(&global_candidates) {
                let global_cfg = Self::load_from_file(&global_path)?;
                Self::merge_into(&mut effective, global_cfg);
            }
            let global_mcp = home.join(".purrcode").join("mcp.json");
            Self::merge_mcp_file(&mut effective, &global_mcp);
        }

        // 3. Project root `purrcode.yaml` / `purrcode.yml` / `purrcode.jsonc` / `purrcode.json`
        let root_candidates = [
            PathBuf::from("purrcode.yaml"),
            PathBuf::from("purrcode.yml"),
            PathBuf::from("purrcode.jsonc"),
            PathBuf::from("purrcode.json"),
        ];
        if let Some(root_path) = find_first(&root_candidates) {
            let root_cfg = Self::load_from_file(&root_path)?;
            Self::merge_into(&mut effective, root_cfg);
        }

        // 4. Project `.purrcode/` (highest precedence)
        let proj_candidates = [
            PathBuf::from(".purrcode/purrcode.yaml"),
            PathBuf::from(".purrcode/purrcode.yml"),
            PathBuf::from(".purrcode/purrcode.jsonc"),
            PathBuf::from(".purrcode/purrcode.json"),
        ];
        if let Some(proj_path) = find_first(&proj_candidates) {
            let proj_cfg = Self::load_from_file(&proj_path)?;
            Self::merge_into(&mut effective, proj_cfg);
        }
        Self::merge_mcp_file(&mut effective, Path::new(".purrcode/mcp.json"));
        Self::merge_mcp_file(&mut effective, Path::new("mcp.json"));

        validate_config(&effective)?;
        Ok(effective)
    }

    pub fn merge_into(base: &mut Config, overlay: Config) {
        if overlay.preset != "tabby" || base.preset.is_empty() {
            base.preset = overlay.preset;
        }
        if overlay.model_preset.is_some() {
            base.model_preset = overlay.model_preset;
        }
        if overlay.model.is_some() {
            base.model = overlay.model;
        }
        if overlay.telegram.is_some() {
            base.telegram = overlay.telegram;
        }
        if overlay.cron.is_some() {
            base.cron = overlay.cron;
        }
        if overlay.heartbeat.is_some() {
            base.heartbeat = overlay.heartbeat;
        }
        for (p_name, p_cfg) in overlay.providers {
            base.providers
                .entry(p_name)
                .and_modify(|existing| {
                    if p_cfg.base_url.is_some() {
                        existing.base_url = p_cfg.base_url.clone();
                    }
                    if p_cfg.api_key.is_some() {
                        existing.api_key = p_cfg.api_key.clone();
                    }
                    for (hk, hv) in &p_cfg.headers {
                        existing.headers.insert(hk.clone(), hv.clone());
                    }
                    for (mk, mv) in &p_cfg.models {
                        existing.models.insert(mk.clone(), mv.clone());
                    }
                })
                .or_insert(p_cfg);
        }
        for (m_name, m_cfg) in overlay.mcp_servers {
            base.mcp_servers.insert(m_name, m_cfg);
        }
    }

    fn merge_mcp_file(base: &mut Config, path: &Path) {
        if !path.exists() {
            return;
        }
        if let Ok(raw) = std::fs::read_to_string(path) {
            let cleaned = clean_jsonc(&raw);
            let expanded = expand_env_vars(&cleaned);
            if let Ok(parsed) = serde_json::from_str::<StandaloneMcpFile>(&expanded) {
                for (k, v) in parsed.mcp_servers {
                    base.mcp_servers.insert(k, v);
                }
            }
        }
    }
}

pub fn read_credentials_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(home) = dirs_home() {
        let creds_path = home.join(".purrcode").join("credentials");
        if let Ok(content) = std::fs::read_to_string(creds_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    map.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
    }
    map
}

pub fn dirs_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars() {
        let mut map = HashMap::new();
        map.insert("TEST_PURR_VAR".to_string(), "expanded_val_42".to_string());
        let raw = "Header ${TEST_PURR_VAR}";
        let expanded = expand_env_vars_with_map(raw, &map);
        assert_eq!(expanded, "Header expanded_val_42");
    }

    #[test]
    fn test_parse_example_file() {
        let example_content = include_str!("../../../purrcode.jsonc.example");
        let config = ConfigLoader::load_from_str(example_content)
            .expect("purrcode.jsonc.example must parse cleanly");
        assert_eq!(config.preset, "tabby");
        assert_eq!(config.model_preset.as_deref(), Some("custom-endpoint"));
        assert!(!config.providers.is_empty());
    }

    #[test]
    fn test_hierarchical_merge() {
        let mut base = ConfigLoader::load_from_str(
            r#"{
                "preset": "tabby",
                "model": "provider-a:model-1",
                "providers": {
                    "provider-a": {
                        "base_url": "${PROVIDER_A_URL}",
                        "models": {
                            "model-1": { "context_window": 131072 }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let overlay = ConfigLoader::load_from_str(
            r#"{
                "preset": "carrier",
                "model": "provider-b:model-2",
                "providers": {
                    "provider-b": {
                        "base_url": "${PROVIDER_B_URL}",
                        "models": {
                            "model-2": { "context_window": 262144 }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        ConfigLoader::merge_into(&mut base, overlay);
        assert_eq!(base.preset, "carrier");
        assert_eq!(base.model.as_deref(), Some("provider-b:model-2"));
        assert!(base.providers.contains_key("provider-a"));
        assert!(base.providers.contains_key("provider-b"));
    }

    #[test]
    fn test_merge_into_keeps_telegram() {
        let mut base = Config::default();
        let overlay = ConfigLoader::load_from_str(
            r#"{
                "telegram": {
                    "bot_token": "${TELEGRAM_BOT_TOKEN}",
                    "allow_from": [100],
                    "api_base": "http://127.0.0.1:3481",
                    "approval_timeout_secs": 120,
                    "poll_timeout_secs": 5
                }
            }"#,
        )
        .unwrap();
        assert!(overlay.telegram.is_some());
        ConfigLoader::merge_into(&mut base, overlay);
        let tg = base
            .telegram
            .expect("merge_into must keep the telegram section");
        assert_eq!(tg.allow_from, vec![100]);
        assert_eq!(tg.api_base.as_deref(), Some("http://127.0.0.1:3481"));
        assert_eq!(tg.approval_timeout_secs(), 120);
        assert_eq!(tg.poll_timeout_secs(), 5);
    }

    #[test]
    fn test_merge_into_keeps_cron() {
        // Regression test for the merge trap (ADR-014 item 9): a new
        // section must survive merge_into, or cron settings defined in
        // ~/.purrcode/purrcode.jsonc vanish under a project config.
        let mut base = Config::default();
        let overlay =
            ConfigLoader::load_from_str(r#"{"cron": {"enabled": false, "tick_secs": 30}}"#)
                .unwrap();
        assert!(overlay.cron.is_some());
        ConfigLoader::merge_into(&mut base, overlay);
        let cron = base.cron.expect("merge_into must keep the cron section");
        assert!(!cron.enabled);
        assert_eq!(cron.tick_secs(), 30);
    }

    #[test]
    fn test_merge_into_keeps_heartbeat() {
        // Same merge-trap regression test, for the heartbeat section
        // (ADR-014 item 9): without its own arm, a heartbeat section
        // defined in ~/.purrcode/purrcode.jsonc would vanish under a
        // project config.
        let mut base = Config::default();
        let overlay = ConfigLoader::load_from_str(
            r#"{"heartbeat": {"enabled": false, "interval_mins": 45}}"#,
        )
        .unwrap();
        assert!(overlay.heartbeat.is_some());
        ConfigLoader::merge_into(&mut base, overlay);
        let hb = base
            .heartbeat
            .expect("merge_into must keep the heartbeat section");
        assert!(!hb.enabled);
        assert_eq!(hb.interval_mins(), 45);
    }

    #[test]
    fn test_heartbeat_config_defaults() {
        let cfg = crate::model::HeartbeatConfig {
            enabled: true,
            interval_mins: None,
        };
        assert_eq!(cfg.interval_mins(), 30);
        let cfg = crate::model::HeartbeatConfig {
            enabled: true,
            interval_mins: Some(2),
        };
        assert_eq!(cfg.interval_mins(), 5);
    }

    #[test]
    fn test_yaml_config_parsing_and_env_interpolation() {
        let mut map = HashMap::new();
        map.insert(
            "TEST_PROVIDER_KEY".to_string(),
            "secret_api_key_123".to_string(),
        );
        let yaml_raw = r#"
preset: tabby
model: "magpie:antigravity/gemini-3.8-flash-tiered"
providers:
  magpie:
    base_url: "https://magpie.raevtar.dev/v1"
    api_key: "${TEST_PROVIDER_KEY}"
"#;
        let expanded = expand_env_vars_with_map(yaml_raw, &map);
        let cfg = ConfigLoader::load_from_yaml_str(&expanded).expect("YAML must parse cleanly");
        assert_eq!(cfg.preset, "tabby");
        assert_eq!(
            cfg.model.as_deref(),
            Some("magpie:antigravity/gemini-3.8-flash-tiered")
        );
        let magpie = cfg
            .providers
            .get("magpie")
            .expect("provider magpie must exist");
        assert_eq!(magpie.api_key.as_deref(), Some("secret_api_key_123"));
        assert_eq!(
            magpie.base_url.as_deref(),
            Some("https://magpie.raevtar.dev/v1")
        );

        // Also test load_from_str with YAML content
        let cfg2 = ConfigLoader::load_from_str(yaml_raw).expect("load_from_str handles YAML");
        assert_eq!(cfg2.preset, "tabby");
        assert_eq!(
            cfg2.model.as_deref(),
            Some("magpie:antigravity/gemini-3.8-flash-tiered")
        );
    }
}
