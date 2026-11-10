use std::io::{self, Write};
use std::path::PathBuf;

pub fn run_interactive_onboard() -> purrcode_api::error::Result<()> {
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
            print!("OpenRouter API Key: ");
            io::stdout().flush().unwrap();
            let key = read_line_trimmed();
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

            print!("API Key (kosongkan jika local proxy tanpa key): ");
            io::stdout().flush().unwrap();
            let key = read_line_trimmed();

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

    // Prepare JSONC content
    let key_line = if api_key.is_empty() {
        String::new()
    } else {
        format!("      \"api_key\": \"{}\",\n", api_key)
    };

    let config_content = format!(
        r#"{{
  // PurrCode AI Configuration
  "preset": "{}",
  "model": "{}:{}",

  "providers": {{
    "{}": {{
      "api": "openai-completions",
      "base_url": "{}",
{}      "models": {{
        "{}": {{
          "context_window": 131072,
          "reasoning": true
        }}
      }}
    }}
  }}
}}
"#,
        preset, provider_name, model_id, provider_name, base_url, key_line, model_id
    );

    // Save location
    let target_path = if let Some(home) = dirs_home() {
        let dir = home.join(".purrcode");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("purrcode.jsonc")
    } else {
        PathBuf::from("purrcode.jsonc")
    };

    std::fs::write(&target_path, config_content.as_bytes()).map_err(purrcode_api::PurrError::Io)?;

    println!();
    println!("✓ Konfigurasi berhasil disimpan di: {}", target_path.display());
    println!("✓ Coba sekarang: purrcode-ai -p \"halo PurrCode\"");
    Ok(())
}

fn read_line_trimmed() -> String {
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
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
