use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use purrcode_api::error::{PurrError, Result};
use purrcode_config::{Config, ModelConfig, ProviderConfig};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn run_interactive_onboard() -> Result<()> {
    println!("🐾 Selamat datang di PurrCode AI Onboarding.");
    println!("(PurrCode AI tidak memiliki default model hardcode — mari konfigurasi provider Anda)");
    println!();

    println!("Tipe provider yang ingin Anda hubungkan:");
    println!("  [1] Custom Endpoint / Local Proxy (Magpie, OmniRoute, vLLM, OpenAI-compatible)");
    println!("  [2] Ollama (Lokal, offline)");
    println!("  [3] Cloud API (OpenRouter, DeepSeek, OpenAI-compatible)");
    print!("Pilih [1/2/3] (default 1): ");
    io::stdout().flush().unwrap();

    let choice = read_line_trimmed();
    let choice = if choice.is_empty() { "1" } else { &choice };

    let (provider_name, base_url, api_key, model_id) = match choice {
        "2" => {
            let p_name = "ollama".to_string();
            let b_url = "http://127.0.0.1:11434/v1".to_string();
            let key = String::new();
            print!("Nama model Ollama [default: qwen2.5-coder:7b]: ");
            io::stdout().flush().unwrap();
            let m_id = read_line_trimmed();
            let m_id = if m_id.is_empty() { "qwen2.5-coder:7b".to_string() } else { m_id };
            (p_name, b_url, key, m_id)
        }
        "3" => {
            let p_name = "openrouter".to_string();
            let b_url = "https://openrouter.ai/api/v1".to_string();
            let key = read_password_hidden("OpenRouter API Key: ");
            print!("Model ID [default: meta-llama/llama-3.3-70b-instruct:free]: ");
            io::stdout().flush().unwrap();
            let m_id = read_line_trimmed();
            let m_id = if m_id.is_empty() { "meta-llama/llama-3.3-70b-instruct:free".to_string() } else { m_id };
            (p_name, b_url, key, m_id)
        }
        _ => {
            let p_name = "local-proxy".to_string();
            print!("Base URL endpoint [default: http://127.0.0.1:3425/v1]: ");
            io::stdout().flush().unwrap();
            let b_url = read_line_trimmed();
            let b_url = if b_url.is_empty() { "http://127.0.0.1:3425/v1".to_string() } else { b_url };

            let key = read_password_hidden("API Key (kosongkan jika local proxy tanpa key): ");

            print!("Model ID [misal: antigravity/gemini-3.8-flash]: ");
            io::stdout().flush().unwrap();
            let m_id = read_line_trimmed();
            let m_id = if m_id.is_empty() { "antigravity/gemini-3.8-flash".to_string() } else { m_id };
            (p_name, b_url, key, m_id)
        }
    };

    print!("Preset mode perilaku? [1] Tabby (default supervised)  [2] Carrier (strict) [default 1]: ");
    io::stdout().flush().unwrap();
    let preset_choice = read_line_trimmed();
    let preset = if preset_choice == "2" { "carrier" } else { "tabby" };

    let purrcode_dir = get_purrcode_dir()?;
    std::fs::create_dir_all(&purrcode_dir).map_err(PurrError::Io)?;

    // Store API key safely in credentials file
    let env_var_name = format!("{}_API_KEY", provider_name.to_uppercase().replace('-', "_"));
    if !api_key.is_empty() {
        save_credential_safe(&purrcode_dir, &env_var_name, &api_key)?;
    }

    // Build structured Config object (properly serialized via serde, no raw string interpolation)
    let mut models_map = HashMap::new();
    models_map.insert(
        model_id.clone(),
        ModelConfig {
            name: Some(model_id.clone()),
            context_window: 131072,
            max_tokens: Some(8192),
            reasoning: true,
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
            models: models_map,
        },
    );

    let config = Config {
        preset: preset.to_string(),
        model_preset: Some("custom-endpoint".to_string()),
        model: Some(format!("{}:{}", provider_name, model_id)),
        providers: providers_map,
        mcp_servers: HashMap::new(),
    };

    let target_path = purrcode_dir.join("purrcode.jsonc");

    // Backup existing configuration if present
    if target_path.exists() {
        let backup_path = purrcode_dir.join("purrcode.jsonc.bak");
        let _ = std::fs::copy(&target_path, &backup_path);
        println!("ℹ️  Konfigurasi lama di-backup ke: {}", backup_path.display());
    }

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| PurrError::Config(format!("Failed to serialize config: {e}")))?;

    std::fs::write(&target_path, serialized.as_bytes()).map_err(PurrError::Io)?;

    println!();
    println!("✓ Konfigurasi aman tersimpan di: {}", target_path.display());
    println!("✓ Coba sekarang: purrcode-ai -p \"halo PurrCode\"");
    Ok(())
}

fn save_credential_safe(dir: &Path, key_name: &str, key_val: &str) -> Result<()> {
    let creds_file = dir.join("credentials");
    let mut content = std::fs::read_to_string(&creds_file).unwrap_or_default();

    // Check if key already exists, replace or append
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

    // Set 0600 file permission on Unix
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

    // If terminal supports raw mode, read with hidden characters
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
        // Fallback for non-TTY / piped stdin
        read_line_trimmed()
    }
}

fn read_line_trimmed() -> String {
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

fn get_purrcode_dir() -> Result<PathBuf> {
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

    #[test]
    fn test_save_credential_safe() {
        let temp_dir = std::env::temp_dir().join(format!("purr_test_cred_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);

        save_credential_safe(&temp_dir, "TEST_API_KEY", "sk-secret-12345").unwrap();
        let creds = std::fs::read_to_string(temp_dir.join("credentials")).unwrap();
        assert!(creds.contains("TEST_API_KEY=sk-secret-12345"));

        // Overwrite check
        save_credential_safe(&temp_dir, "TEST_API_KEY", "sk-secret-updated").unwrap();
        let creds2 = std::fs::read_to_string(temp_dir.join("credentials")).unwrap();
        assert!(creds2.contains("TEST_API_KEY=sk-secret-updated"));
        assert!(!creds2.contains("sk-secret-12345"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_config_serialization_escaping() {
        let mut providers = HashMap::new();
        providers.insert(
            "custom-provider".to_string(),
            ProviderConfig {
                api: "openai-completions".to_string(),
                base_url: Some("http://example.com/v1?token=\"quoted\"\\escaped".to_string()),
                api_key: Some("${CUSTOM_API_KEY}".to_string()),
                headers: HashMap::new(),
                models: HashMap::new(),
            },
        );

        let config = Config {
            preset: "tabby".to_string(),
            model_preset: None,
            model: Some("custom:model/test".to_string()),
            providers,
            mcp_servers: HashMap::new(),
        };

        let json_str = serde_json::to_string_pretty(&config).unwrap();
        assert!(json_str.contains(r#"\"quoted\""#));
        let deserialized: Config = serde_json::from_str(&json_str).unwrap();
        assert_eq!(
            deserialized.providers["custom-provider"].base_url.as_deref(),
            Some("http://example.com/v1?token=\"quoted\"\\escaped")
        );
    }
}
