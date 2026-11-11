// SPDX-License-Identifier: GPL-3.0-or-later

use crate::jsonc::clean_jsonc;
use purrcode_api::error::{PurrError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BUNDLED_MODEL_PRESETS: &[(&str, &str)] = &[
    (
        "custom-endpoint",
        include_str!("../../../model-presets/custom-endpoint.jsonc"),
    ),
    ("free", include_str!("../../../model-presets/free.jsonc")),
    ("local", include_str!("../../../model-presets/local.jsonc")),
    (
        "magpie",
        include_str!("../../../model-presets/magpie.jsonc"),
    ),
    (
        "premium",
        include_str!("../../../model-presets/premium.jsonc"),
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ModelTarget {
    Single(String),
    Chain(Vec<String>),
}

impl ModelTarget {
    pub fn candidates(&self) -> Vec<String> {
        match self {
            ModelTarget::Single(s) => vec![s.clone()],
            ModelTarget::Chain(v) => v.clone(),
        }
    }

    pub fn primary(&self) -> Option<&str> {
        match self {
            ModelTarget::Single(s) => Some(s.as_str()),
            ModelTarget::Chain(v) => v.first().map(|s| s.as_str()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModelSpec {
    pub model: ModelTarget,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    #[serde(default)]
    pub fallback: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreset {
    #[serde(
        rename = "$schema",
        alias = "schema",
        default = "default_model_preset_schema"
    )]
    pub schema: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub agents: HashMap<String, AgentModelSpec>,
}

fn default_model_preset_schema() -> String {
    "model-preset/v1".to_string()
}

pub struct ModelPresetLoader;

impl ModelPresetLoader {
    pub fn parse_str(raw: &str) -> Result<ModelPreset> {
        let cleaned = clean_jsonc(raw);
        let preset: ModelPreset = serde_json::from_str(&cleaned)
            .map_err(|e| PurrError::Config(format!("Invalid model-preset JSONC: {e}")))?;
        if preset.schema != "model-preset/v1" {
            return Err(PurrError::Config(format!(
                "Unsupported model-preset schema '{}' (expected 'model-preset/v1')",
                preset.schema
            )));
        }
        Ok(preset)
    }

    pub fn discover_all(workspace_root: &Path) -> HashMap<String, ModelPreset> {
        let mut raw_map = HashMap::new();

        for (_, raw) in BUNDLED_MODEL_PRESETS {
            if let Ok(preset) = Self::parse_str(raw) {
                raw_map.insert(preset.name.clone(), preset);
            }
        }

        Self::scan_dir(&workspace_root.join("model-presets"), &mut raw_map);
        if let Some(home) = crate::loader::dirs_home() {
            Self::scan_dir(&home.join(".purrcode").join("model-presets"), &mut raw_map);
        }
        Self::scan_dir(
            &workspace_root.join(".purrcode").join("model-presets"),
            &mut raw_map,
        );

        // Resolve single-parent `extends` without cycles
        let keys: Vec<String> = raw_map.keys().cloned().collect();
        let mut resolved = HashMap::new();
        for key in keys {
            if let Some(preset) = raw_map.get(&key) {
                let mut merged = preset.clone();
                if let Some(ref parent_name) = preset.extends {
                    if parent_name != &key {
                        if let Some(parent) = raw_map.get(parent_name) {
                            let mut combined_agents = parent.agents.clone();
                            for (agent_k, spec) in &preset.agents {
                                combined_agents.insert(agent_k.clone(), spec.clone());
                            }
                            merged.agents = combined_agents;
                        }
                    }
                }
                resolved.insert(key, merged);
            }
        }

        resolved
    }

    pub fn load_by_name(name: &str, workspace_root: &Path) -> Result<ModelPreset> {
        let mut all = Self::discover_all(workspace_root);
        all.remove(name)
            .ok_or_else(|| PurrError::Config(format!("Model preset '{}' not found", name)))
    }

    fn scan_dir(dir: &Path, map: &mut HashMap<String, ModelPreset>) {
        if !dir.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path: PathBuf = entry.path();
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| ext == "jsonc" || ext == "json")
                    .unwrap_or(false)
                {
                    if let Ok(raw) = std::fs::read_to_string(&path) {
                        if let Ok(mut preset) = Self::parse_str(&raw) {
                            if preset.name.is_empty() {
                                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                    preset.name = stem.to_string();
                                }
                            }
                            map.insert(preset.name.clone(), preset);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundled_model_presets_load() {
        let presets = ModelPresetLoader::discover_all(Path::new("."));
        assert!(presets.contains_key("free"));
        assert!(presets.contains_key("local"));
        assert!(presets.contains_key("magpie"));
        assert!(presets.contains_key("premium"));
        assert!(presets.contains_key("custom-endpoint"));
        assert!(presets["free"].agents.contains_key("sphinx"));
    }
}
