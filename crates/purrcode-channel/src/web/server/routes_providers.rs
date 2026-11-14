// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider model discovery endpoints.
//!
//! Implements `POST /api/providers/{id}/fetch-models`.
//! Queries the upstream provider's `GET /models` endpoint using reqwest
//! and returns a normalized list of available models.

use super::WebState;
use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    Json,
};
use purrcode_config::jsonc::clean_jsonc;
use purrcode_config::ConfigLoader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Loads raw provider entries/overrides from `cockpit_providers.json` without synthetic defaults.
pub(crate) fn load_cockpit_overrides(purrcode_dir: &Path) -> Vec<Value> {
    let file = purrcode_dir.join("cockpit_providers.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(&raw) {
            return v;
        }
    }
    Vec::new()
}

pub(crate) fn save_cockpit_overrides(purrcode_dir: &Path, providers: &[Value]) {
    let file = purrcode_dir.join("cockpit_providers.json");
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&json!(providers)) {
        let _ = std::fs::write(&file, text);
    }
}

/// Strips sensitive `apiKey` values before returning provider metadata to WebUI clients.
pub(crate) fn sanitize_provider_for_client(prov: &Value) -> Value {
    let mut copy = prov.clone();
    if let Some(obj) = copy.as_object_mut() {
        let has_inline_key = obj
            .get("apiKey")
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let existing_flag = obj
            .get("hasApiKey")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        obj.insert(
            "hasApiKey".to_string(),
            json!(has_inline_key || existing_flag),
        );
        obj.remove("apiKey");
    }
    copy
}

pub(crate) fn sanitize_providers_for_client(providers: &[Value]) -> Vec<Value> {
    providers.iter().map(sanitize_provider_for_client).collect()
}

pub(crate) fn load_config_json_sync(workspace_root: &Path, purrcode_dir: &Path) -> Option<Value> {
    let candidates = [
        workspace_root.join(".purrcode").join("purrcode.yaml"),
        workspace_root.join(".purrcode").join("purrcode.yml"),
        workspace_root.join(".purrcode").join("purrcode.jsonc"),
        workspace_root.join(".purrcode").join("purrcode.json"),
        workspace_root.join("purrcode.yaml"),
        workspace_root.join("purrcode.yml"),
        workspace_root.join("purrcode.jsonc"),
        workspace_root.join("purrcode.json"),
        purrcode_dir.join("purrcode.yaml"),
        purrcode_dir.join("purrcode.yml"),
        purrcode_dir.join("purrcode.jsonc"),
        purrcode_dir.join("purrcode.json"),
    ];
    for path in &candidates {
        if path.is_file() {
            if let Ok(raw) = std::fs::read_to_string(path) {
                let is_yaml = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
                    .unwrap_or(false);
                if is_yaml {
                    if let Ok(val) = serde_yaml::from_str::<Value>(&raw) {
                        return Some(val);
                    }
                } else {
                    let cleaned = clean_jsonc(&raw);
                    if let Ok(val) = serde_json::from_str::<Value>(&cleaned) {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

/// Merges providers declared in `purrcode.yaml`/`purrcode.jsonc` with overrides and custom entries
/// persisted in `cockpit_providers.json`. Returns `(providers, active_preset, primary_model)`.
pub(crate) fn extract_providers_merged(
    config: Option<&Value>,
    purrcode_dir: &Path,
) -> (Vec<Value>, String, String) {
    let mut providers = Vec::new();
    let mut active_preset = "tabby".to_string();
    let mut primary_model = String::new();

    if let Some(cfg) = config {
        if let Some(p) = cfg.get("preset").and_then(|v| v.as_str()) {
            active_preset = p.to_string();
        }
        if let Some(m) = cfg.get("model").and_then(|v| v.as_str()) {
            primary_model = m.split_once(':').map(|(_, b)| b).unwrap_or(m).to_string();
        }

        if let Some(prov_map) = cfg.get("providers").and_then(|v| v.as_object()) {
            for (prov_id, prov_val) in prov_map {
                let api_type = prov_val
                    .get("api")
                    .and_then(|v| v.as_str())
                    .unwrap_or("openai");
                let protocol = if api_type.contains("gemini") {
                    "gemini"
                } else if api_type.contains("anthropic") {
                    "anthropic"
                } else if api_type.contains("deepseek") {
                    "deepseek"
                } else if api_type.contains("ollama") {
                    "ollama"
                } else if prov_id == "custom" {
                    "custom"
                } else {
                    "openai"
                };

                let base_url = prov_val
                    .get("base_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let has_api_key = prov_val
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);

                let mut models = Vec::new();
                let mut max_context = 131_072u64;
                if let Some(models_obj) = prov_val.get("models").and_then(|v| v.as_object()) {
                    for (m_name, m_cfg) in models_obj {
                        models.push(m_name.clone());
                        if let Some(cw) = m_cfg.get("context_window").and_then(|v| v.as_u64()) {
                            if cw > max_context {
                                max_context = cw;
                            }
                        }
                    }
                }
                if models.is_empty() && !primary_model.is_empty() {
                    models.push(primary_model.clone());
                }

                let default_model = if !primary_model.is_empty() && models.contains(&primary_model)
                {
                    primary_model.clone()
                } else {
                    models.first().cloned().unwrap_or_default()
                };

                let prov_name = match prov_id.as_str() {
                    "magpie" => "Local Magpie / OpenAI-Compatible".to_string(),
                    "custom" => "Custom Gateway Endpoint".to_string(),
                    _ => format!("{prov_id} Gateway"),
                };

                providers.push(json!({
                    "id": format!("prov-{prov_id}"),
                    "configKey": prov_id,
                    "source": "config",
                    "name": prov_name,
                    "protocol": protocol,
                    "baseUrl": base_url,
                    "defaultModel": default_model,
                    "models": models,
                    "contextWindow": max_context,
                    "hasApiKey": has_api_key,
                    "enabled": true,
                    "lastLatencyMs": Value::Null,
                    "lastTestedAt": "Configured",
                }));
            }
        }
    }

    let overrides = load_cockpit_overrides(purrcode_dir);
    for item in overrides {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if let Some(existing) = providers
            .iter_mut()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
        {
            if let (Some(dst), Some(src)) = (existing.as_object_mut(), item.as_object()) {
                for key in [
                    "name",
                    "protocol",
                    "baseUrl",
                    "defaultModel",
                    "models",
                    "contextWindow",
                    "enabled",
                    "lastLatencyMs",
                    "lastTestedAt",
                    "apiKey",
                ] {
                    if let Some(val) = src.get(key) {
                        dst.insert(key.to_string(), val.clone());
                    }
                }
            }
        } else {
            let mut added = item.clone();
            if let Some(obj) = added.as_object_mut() {
                obj.entry("source".to_string())
                    .or_insert_with(|| json!("cockpit"));
                let fallback_key = id.strip_prefix("prov-").unwrap_or(id).to_string();
                obj.entry("configKey".to_string())
                    .or_insert_with(|| json!(fallback_key));
            }
            providers.push(added);
        }
    }

    if primary_model.is_empty() {
        primary_model = providers
            .iter()
            .find(|p| p.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true))
            .and_then(|p| p.get("defaultModel").and_then(|m| m.as_str()))
            .unwrap_or("")
            .to_string();
    }

    (providers, active_preset, primary_model)
}

pub(crate) fn load_merged_providers(workspace_root: &Path, purrcode_dir: &Path) -> Vec<Value> {
    let cfg = load_config_json_sync(workspace_root, purrcode_dir);
    extract_providers_merged(cfg.as_ref(), purrcode_dir).0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FetchedModel {
    pub id: String,
    pub name: String,
    pub context_window: u64,
}

/// Parses an upstream models response into a standardized list of `FetchedModel`.
/// Supports standard OpenAI `{"data": [...]}`, alternative `{"models": [...]}`,
/// or bare arrays `[...]`.
pub fn parse_models_from_json(json_body: &Value) -> Vec<FetchedModel> {
    let mut models = Vec::new();

    let raw_list = if let Some(arr) = json_body.get("data").and_then(|v| v.as_array()) {
        Some(arr)
    } else if let Some(arr) = json_body.get("models").and_then(|v| v.as_array()) {
        Some(arr)
    } else {
        json_body.as_array()
    };

    if let Some(items) = raw_list {
        for item in items {
            let m_id = if let Some(id_str) = item.get("id").and_then(|v| v.as_str()) {
                id_str.to_string()
            } else if let Some(name_str) = item.get("name").and_then(|v| v.as_str()) {
                name_str.to_string()
            } else {
                continue;
            };

            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&m_id)
                .to_string();

            let context_window = item
                .get("context_window")
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("max_tokens").and_then(|v| v.as_u64()))
                .or_else(|| item.get("context_length").and_then(|v| v.as_u64()))
                .unwrap_or(131072);

            models.push(FetchedModel {
                id: m_id,
                name,
                context_window,
            });
        }
    }

    models
}

pub async fn fetch_models_handler(
    State(state): State<Arc<WebState>>,
    AxumPath(id): AxumPath<String>,
) -> (StatusCode, Json<Value>) {
    let candidates = [
        state.workspace_root.join(".purrcode").join("purrcode.yaml"),
        state.workspace_root.join(".purrcode").join("purrcode.yml"),
        state
            .workspace_root
            .join(".purrcode")
            .join("purrcode.jsonc"),
        state.workspace_root.join(".purrcode").join("purrcode.json"),
        state.workspace_root.join("purrcode.yaml"),
        state.workspace_root.join("purrcode.yml"),
        state.workspace_root.join("purrcode.jsonc"),
        state.workspace_root.join("purrcode.json"),
        state.purrcode_dir.join("purrcode.yaml"),
        state.purrcode_dir.join("purrcode.yml"),
        state.purrcode_dir.join("purrcode.jsonc"),
        state.purrcode_dir.join("purrcode.json"),
    ];

    let mut found_cfg = None;
    for path in &candidates {
        if path.is_file() {
            if let Ok(cfg) = ConfigLoader::load_from_file(path) {
                found_cfg = Some(cfg);
                break;
            }
        }
    }

    let config = match found_cfg {
        Some(cfg) => cfg,
        None => match ConfigLoader::load_effective() {
            Ok(cfg) => cfg,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("Failed to load configuration: {e}") })),
                );
            }
        },
    };

    let clean_id = id.strip_prefix("prov-").unwrap_or(&id);
    let mut base_url_opt = config
        .providers
        .get(clean_id)
        .or_else(|| config.providers.get(&clean_id.to_ascii_lowercase()))
        .and_then(|p| p.base_url.clone())
        .map(|u| u.trim().to_string());
    let mut api_key = config
        .providers
        .get(clean_id)
        .or_else(|| config.providers.get(&clean_id.to_ascii_lowercase()))
        .and_then(|p| p.api_key.clone());
    let user_agent_opt = config
        .providers
        .get(clean_id)
        .or_else(|| config.providers.get(&clean_id.to_ascii_lowercase()))
        .and_then(|p| p.user_agent.clone());
    let extra_headers = config
        .providers
        .get(clean_id)
        .or_else(|| config.providers.get(&clean_id.to_ascii_lowercase()))
        .map(|p| p.headers.clone())
        .unwrap_or_default();

    // Overlay any saved WebUI overrides/custom providers from cockpit_providers.json.
    let overrides = load_cockpit_overrides(&state.purrcode_dir);
    let override_entry = overrides.iter().find(|p| {
        let pid = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
        pid == id || pid.strip_prefix("prov-") == Some(clean_id)
    });
    if let Some(found) = override_entry {
        if let Some(u) = found.get("baseUrl").and_then(|v| v.as_str()) {
            let trimmed = u.trim();
            if !trimmed.is_empty() {
                base_url_opt = Some(trimmed.to_string());
            }
        }
        if let Some(k) = found.get("apiKey").and_then(|v| v.as_str()) {
            let trimmed = k.trim();
            if !trimmed.is_empty() {
                api_key = Some(trimmed.to_string());
            }
        }
    } else if base_url_opt.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Provider '{clean_id}' not found in configuration") })),
        );
    }

    let base_url = match base_url_opt {
        Some(ref u) if !u.is_empty() => {
            if !u.starts_with("http://") && !u.starts_with("https://") {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Provider base_url must use http:// or https://" })),
                );
            }
            u.as_str()
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "error": format!("Provider '{clean_id}' has no base_url configured") }),
                ),
            );
        }
    };

    let url = format!("{}/models", base_url.trim_end_matches('/'));

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed to create HTTP client: {e}") })),
            );
        }
    };

    let mut req = client.get(&url);
    if let Some(key) = &api_key {
        let key_trimmed = key.trim();
        if !key_trimmed.is_empty() {
            req = req.header("Authorization", format!("Bearer {key_trimmed}"));
        }
    }

    if let Some(ua) = &user_agent_opt {
        req = req.header("User-Agent", ua);
    } else {
        req = req.header("User-Agent", "PurrCode-AI/0.1");
    }

    for (k, v) in &extra_headers {
        req = req.header(k, v);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Failed to connect to provider endpoint: {e}") })),
            );
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("Provider returned HTTP status {status}: {err_text}")
            })),
        );
    }

    let json_body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("Failed to parse JSON from provider: {e}") })),
            );
        }
    };

    let models = parse_models_from_json(&json_body);
    (StatusCode::OK, Json(json!(models)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_models_openai_format() {
        let json_data = json!({
            "object": "list",
            "data": [
                {
                    "id": "gpt-4o",
                    "object": "model",
                    "created": 1715368132,
                    "owned_by": "system"
                },
                {
                    "id": "claude-3-5-sonnet",
                    "name": "Claude 3.5 Sonnet",
                    "context_window": 200000
                }
            ]
        });

        let parsed = parse_models_from_json(&json_data);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "gpt-4o");
        assert_eq!(parsed[0].name, "gpt-4o");
        assert_eq!(parsed[0].context_window, 131072);
        assert_eq!(parsed[1].id, "claude-3-5-sonnet");
        assert_eq!(parsed[1].name, "Claude 3.5 Sonnet");
        assert_eq!(parsed[1].context_window, 200000);
    }

    #[test]
    fn test_parse_models_array_format() {
        let json_data = json!([
            { "id": "model-1", "context_length": 65536 },
            { "name": "model-2" }
        ]);

        let parsed = parse_models_from_json(&json_data);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "model-1");
        assert_eq!(parsed[0].context_window, 65536);
        assert_eq!(parsed[1].id, "model-2");
        assert_eq!(parsed[1].name, "model-2");
        assert_eq!(parsed[1].context_window, 131072);
    }

    #[test]
    fn test_extract_providers_merged_and_sanitized() {
        let dir = std::env::temp_dir().join(format!("purr_prov_merge_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let cfg = json!({
            "preset": "tabby",
            "model": "magpie:gemini-flash",
            "providers": {
                "magpie": {
                    "api": "openai-completions",
                    "base_url": "http://127.0.0.1:3425/v1",
                    "api_key": "sk-secret",
                    "models": {
                        "gemini-flash": { "context_window": 256000 }
                    }
                }
            }
        });
        save_cockpit_overrides(
            &dir,
            &[json!({
                "id": "prov-magpie",
                "enabled": false,
                "models": ["gemini-flash", "claude-sonnet"]
            })],
        );

        let (merged, preset, primary) = extract_providers_merged(Some(&cfg), &dir);
        assert_eq!(preset, "tabby");
        assert_eq!(primary, "gemini-flash");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["enabled"], false);
        assert_eq!(
            merged[0]["models"],
            json!(["gemini-flash", "claude-sonnet"])
        );
        let sanitized = sanitize_providers_for_client(&merged);
        assert_eq!(sanitized[0]["hasApiKey"], true);
        assert!(sanitized[0].get("apiKey").is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
