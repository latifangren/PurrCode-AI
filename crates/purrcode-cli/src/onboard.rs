// SPDX-License-Identifier: GPL-3.0-or-later

use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use purrcode_api::error::{PurrError, Result};
use purrcode_config::{Config, ModelConfig, ProviderConfig, MIN_CONTEXT_WINDOW};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Validates that `base_url` is a valid HTTP(S) URL and is reachable over the network.
pub async fn validate_endpoint_connectivity(base_url: &str, api_key: Option<&str>) -> Result<()> {
    let mut normalized = base_url.trim().to_string();
    if normalized.ends_with('/') {
        normalized.pop();
    }

    let parsed = reqwest::Url::parse(&normalized).map_err(|e| {
        PurrError::Config(format!("Invalid endpoint base_url '{}': {}", base_url, e))
    })?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(PurrError::Config(format!(
            "Unsupported scheme '{}' in base_url (expected http or https)",
            parsed.scheme()
        )));
    }

    let probe_url = if normalized.ends_with("/v1") {
        format!("{}/models", normalized)
    } else {
        format!("{}/v1/models", normalized)
    };

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| PurrError::Config(format!("Failed to build HTTP client: {e}")))?;

    let mut req = client.get(&probe_url);
    if let Some(key) = api_key {
        if !key.trim().is_empty() {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
    }

    let resp = req.send().await.map_err(|e| {
        PurrError::Config(format!(
            "Endpoint connectivity validation failed for '{}': {}",
            base_url, e
        ))
    })?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(PurrError::Config(format!(
            "Endpoint '{}' rejected authentication with HTTP {} (check your API key)",
            base_url,
            resp.status()
        )));
    }

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(PurrError::Config(format!(
            "Endpoint '{}' not found with HTTP 404 (check your base URL path, expected /v1)",
            base_url
        )));
    }

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(PurrError::Config(format!(
            "Endpoint '{}' returned HTTP 429 Too Many Requests (rate limit or quota exceeded)",
            base_url
        )));
    }

    if resp.status().is_client_error() {
        return Err(PurrError::Config(format!(
            "Endpoint '{}' returned client error HTTP {}",
            base_url,
            resp.status()
        )));
    }

    if resp.status().is_server_error() {
        return Err(PurrError::Config(format!(
            "Endpoint '{}' returned server error HTTP {}",
            base_url,
            resp.status()
        )));
    }

    Ok(())
}

pub async fn run_interactive_onboard() -> Result<()> {
    println!("🐾 Welcome to PurrCode AI Onboarding.");
    println!(
        "(PurrCode AI has zero hardcoded endpoints or models — configure your provider below)"
    );
    println!();

    print!("Provider name (e.g. custom, ollama, openrouter): ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let provider_name = read_line_trimmed();
    if provider_name.is_empty() {
        return Err(PurrError::Config(
            "Provider name is required (no hardcoded default)".to_string(),
        ));
    }

    print!("Endpoint Base URL (required, no hardcoded default): ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let base_url = read_line_trimmed();
    if base_url.is_empty() {
        return Err(PurrError::Config(
            "Endpoint base_url is required (no hardcoded default)".to_string(),
        ));
    }

    let api_key = read_password_hidden("API Key (leave empty if local endpoint requires no key): ");

    print!("Primary Model ID (required, no hardcoded default): ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let model_id = read_line_trimmed();
    if model_id.is_empty() {
        return Err(PurrError::Config(
            "Model ID is required (no hardcoded default)".to_string(),
        ));
    }

    print!(
        "Context window in tokens (required, min {}): ",
        MIN_CONTEXT_WINDOW
    );
    io::stdout().flush().map_err(PurrError::Io)?;
    let ctx_raw = read_line_trimmed();
    let context_window: u64 = ctx_raw.parse().map_err(|_| {
        PurrError::Config(format!(
            "Valid numeric context_window (>= {}) is required",
            MIN_CONTEXT_WINDOW
        ))
    })?;
    if context_window < MIN_CONTEXT_WINDOW {
        return Err(PurrError::Config(format!(
            "context_window ({}) is below the required 64K ({}) minimum",
            context_window, MIN_CONTEXT_WINDOW
        )));
    }

    print!("Max output tokens (optional, press Enter to leave unset): ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let max_tok_raw = read_line_trimmed();
    let max_tokens: Option<u64> =
        if max_tok_raw.is_empty() {
            None
        } else {
            Some(max_tok_raw.parse().map_err(|_| {
                PurrError::Config("Invalid numeric value for max_tokens".to_string())
            })?)
        };

    print!("Does this model emit reasoning/thinking blocks? [y/N]: ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let reasoning_raw = read_line_trimmed();
    let reasoning =
        reasoning_raw.eq_ignore_ascii_case("y") || reasoning_raw.eq_ignore_ascii_case("yes");

    print!("Mode preset? [1] tabby (default supervised)  [2] carrier (strict) [default 1]: ");
    io::stdout().flush().map_err(PurrError::Io)?;
    let preset_choice = read_line_trimmed();
    let preset = if preset_choice == "2" {
        "carrier"
    } else {
        "tabby"
    };

    // Validate connectivity to the endpoint before writing configuration
    validate_endpoint_connectivity(
        &base_url,
        if api_key.is_empty() {
            None
        } else {
            Some(api_key.as_str())
        },
    )
    .await?;

    let purrcode_dir = get_purrcode_dir()?;
    std::fs::create_dir_all(&purrcode_dir).map_err(PurrError::Io)?;

    // Store API key safely in credentials file (0600 on Unix)
    let env_var_name = format!("{}_API_KEY", provider_name.to_uppercase().replace('-', "_"));
    if !api_key.is_empty() {
        save_credential_safe(&purrcode_dir, &env_var_name, &api_key)?;
    }

    let mut models_map = HashMap::new();
    models_map.insert(
        model_id.clone(),
        ModelConfig {
            name: Some(model_id.clone()),
            context_window,
            max_tokens,
            reasoning,
            reasoning_effort: None,
        },
    );

    let mut providers_map = HashMap::new();
    providers_map.insert(
        provider_name.clone(),
        ProviderConfig {
            api: "openai-completions".to_string(),
            base_url: Some(base_url),
            api_key: if api_key.is_empty() {
                None
            } else {
                Some(format!("${{{}}}", env_var_name))
            },
            headers: HashMap::new(),
            user_agent: None,
            models: models_map,
        },
    );

    let config = Config {
        preset: preset.to_string(),
        model_preset: Some("custom-endpoint".to_string()),
        model: Some(format!("{}:{}", provider_name, model_id)),
        providers: providers_map,
        mcp_servers: HashMap::new(),
        telegram: None,
        cron: None,
    };

    let target_path = purrcode_dir.join("purrcode.jsonc");

    if target_path.exists() {
        let backup_path = purrcode_dir.join("purrcode.jsonc.bak");
        let _ = std::fs::copy(&target_path, &backup_path);
        println!(
            "ℹ️  Existing configuration backed up to: {}",
            backup_path.display()
        );
    }

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| PurrError::Config(format!("Failed to serialize config: {e}")))?;

    std::fs::write(&target_path, serialized.as_bytes()).map_err(PurrError::Io)?;

    println!();
    println!("✓ Configuration saved to: {}", target_path.display());
    println!("✓ Try now: purrcode-ai -p \"hello PurrCode\"");
    Ok(())
}

fn save_credential_safe(dir: &Path, key_name: &str, key_val: &str) -> Result<()> {
    let creds_file = dir.join("credentials");
    let mut content = std::fs::read_to_string(&creds_file).unwrap_or_default();

    let mut replaced = false;
    let mut new_lines = Vec::new();

    for line in content.lines() {
        if line.starts_with(&format!("{}=", key_name)) {
            new_lines.push(format!("{}={}", key_name, key_val));
            replaced = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !replaced {
        new_lines.push(format!("{}={}", key_name, key_val));
    }

    content = new_lines.join("\n") + "\n";
    std::fs::write(&creds_file, content.as_bytes()).map_err(PurrError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&creds_file, perms);
    }

    Ok(())
}

fn read_password_hidden(prompt: &str) -> String {
    print!("{}", prompt);
    let _ = io::stdout().flush();

    if enable_raw_mode().is_ok() {
        let mut pass = String::new();
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Enter => break,
                    KeyCode::Char(c) => pass.push(c),
                    KeyCode::Backspace => {
                        pass.pop();
                    }
                    KeyCode::Esc => {
                        pass.clear();
                        break;
                    }
                    _ => {}
                }
            }
        }
        let _ = disable_raw_mode();
        println!();
        pass.trim().to_string()
    } else {
        read_line_trimmed()
    }
}

pub fn read_line_trimmed() -> String {
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

pub fn get_purrcode_dir() -> Result<PathBuf> {
    if let Some(home) = dirs_home() {
        Ok(home.join(".purrcode"))
    } else {
        Ok(PathBuf::from(".purrcode"))
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

    #[tokio::test]
    async fn test_validate_endpoint_connectivity_rejects_dead_endpoint() {
        // Unreachable port 9 must fail connectivity validation
        let res = validate_endpoint_connectivity("http://127.0.0.1:9/v1", None).await;
        assert!(res.is_err());

        // Malformed URL or non-HTTP scheme must fail
        let bad_scheme = validate_endpoint_connectivity("ftp://example.com/v1", None).await;
        assert!(bad_scheme.is_err());
    }

    #[test]
    fn test_save_credential_safe() {
        let temp_dir =
            std::env::temp_dir().join(format!("purr_test_cred_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);

        save_credential_safe(&temp_dir, "TEST_API_KEY", "val_one").unwrap();
        let creds = std::fs::read_to_string(temp_dir.join("credentials")).unwrap();
        assert!(creds.contains("TEST_API_KEY=val_one"));

        save_credential_safe(&temp_dir, "TEST_API_KEY", "val_two").unwrap();
        let creds2 = std::fs::read_to_string(temp_dir.join("credentials")).unwrap();
        assert!(creds2.contains("TEST_API_KEY=val_two"));
        assert!(!creds2.contains("val_one"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_config_serialization_escaping() {
        let mut providers = HashMap::new();
        providers.insert(
            "custom-provider".to_string(),
            ProviderConfig {
                api: "openai-completions".to_string(),
                base_url: Some("https://endpoint.invalid/v1?param=\"quoted\"\\escaped".to_string()),
                api_key: Some("${CUSTOM_API_KEY}".to_string()),
                headers: HashMap::new(),
                user_agent: None,
                models: HashMap::new(),
            },
        );

        let config = Config {
            preset: "tabby".to_string(),
            model_preset: None,
            model: Some("custom:model/test".to_string()),
            providers,
            mcp_servers: HashMap::new(),
            telegram: None,
            cron: None,
        };

        let json_str = serde_json::to_string_pretty(&config).unwrap();
        assert!(json_str.contains(r#"\"quoted\""#));
        let deserialized: Config = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            deserialized.providers["custom-provider"]
                .base_url
                .as_deref(),
            Some("https://endpoint.invalid/v1?param=\"quoted\"\\escaped")
        );
    }
}
