use crate::jsonc::clean_jsonc;
use crate::model::Config;
use purrcode_api::error::{PurrError, Result};
use std::path::{Path, PathBuf};

/// Interpolates `${VAR}` and `$VAR` from environment variables
pub fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' && i + 1 < len {
            if chars[i + 1] == '{' {
                // ${VAR_NAME}
                let start = i + 2;
                let mut end = start;
                while end < len && chars[end] != '}' {
                    end += 1;
                }
                if end < len {
                    let var_name: String = chars[start..end].iter().collect();
                    if let Ok(val) = std::env::var(&var_name) {
                        result.push_str(&val);
                    }
                    i = end + 1;
                    continue;
                }
            } else if chars[i + 1].is_ascii_alphanumeric() || chars[i + 1] == '_' {
                // $VAR_NAME
                let start = i + 1;
                let mut end = start;
                while end < len && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let var_name: String = chars[start..end].iter().collect();
                if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
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

pub struct ConfigLoader;

impl ConfigLoader {
    /// Discovers default config path in order:
    /// 1. `.purrcode/purrcode.jsonc` in current directory
    /// 2. `purrcode.jsonc` in current directory
    /// 3. `~/.purrcode/purrcode.jsonc` in user home directory
    pub fn find_config_path() -> Option<PathBuf> {
        let local_sub = Path::new(".purrcode/purrcode.jsonc");
        if local_sub.exists() {
            return Some(local_sub.to_path_buf());
        }

        let local_root = Path::new("purrcode.jsonc");
        if local_root.exists() {
            return Some(local_root.to_path_buf());
        }

        if let Some(home) = dirs_home() {
            let global = home.join(".purrcode").join("purrcode.jsonc");
            if global.exists() {
                return Some(global);
            }
        }

        None
    }

    pub fn load_from_str(content: &str) -> Result<Config> {
        let cleaned = clean_jsonc(content);
        let expanded = expand_env_vars(&cleaned);
        serde_json::from_str(&expanded).map_err(|e| {
            PurrError::Config(format!("Failed to parse config JSONC: {e}"))
        })
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Config> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            PurrError::Config(format!("Cannot read config file '{}': {e}", path.display()))
        })?;
        Self::load_from_str(&content)
    }

    pub fn load_effective() -> Result<Config> {
        if let Some(path) = Self::find_config_path() {
            Self::load_from_file(path)
        } else {
            Ok(Config::default())
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
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
        std::env::set_var("TEST_PURR_KEY", "secret_key_123");
        let raw = "Bearer ${TEST_PURR_KEY}";
        let expanded = expand_env_vars(raw);
        assert_eq!(expanded, "Bearer secret_key_123");
    }

    #[test]
    fn test_parse_example_file() {
        let example_content = include_str!("../../../purrcode.jsonc.example");
        let config = ConfigLoader::load_from_str(example_content).expect("purrcode.jsonc.example must parse cleanly");
        assert_eq!(config.preset, "tabby");
        assert_eq!(config.model_preset.as_deref(), Some("custom-endpoint"));
        assert!(config.providers.contains_key("magpie"));
        assert!(config.providers.contains_key("ollama"));
    }
}
