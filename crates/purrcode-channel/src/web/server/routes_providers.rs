// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider model discovery endpoints.
//!
//! Implements `POST /api/providers/{id}/fetch-models`.
//! Queries the upstream provider's `GET /models` endpoint using reqwest
//! and returns a normalized list of available models.

use super::WebState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use purrcode_config::ConfigLoader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

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
    Path(id): Path<String>,
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
    let provider = config
        .providers
        .get(clean_id)
        .or_else(|| config.providers.get(&clean_id.to_ascii_lowercase()));

    let provider = match provider {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    json!({ "error": format!("Provider '{clean_id}' not found in configuration") }),
                ),
            );
        }
    };

    let base_url = match &provider.base_url {
        Some(u) if !u.trim().is_empty() => u.trim(),
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
    if let Some(key) = &provider.api_key {
        let key_trimmed = key.trim();
        if !key_trimmed.is_empty() {
            req = req.header("Authorization", format!("Bearer {key_trimmed}"));
        }
    }

    if let Some(ua) = &provider.user_agent {
        req = req.header("User-Agent", ua);
    } else {
        req = req.header("User-Agent", "PurrCode-AI/0.1");
    }

    for (k, v) in &provider.headers {
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
}
