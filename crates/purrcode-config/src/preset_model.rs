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

    /// Directories a model-preset file may live in, in discovery order
    /// (same user dirs as `discover_all`: workspace, home, workspace
    /// `.purrcode`). Bundled presets have no file.
    fn preset_dirs(workspace_root: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![workspace_root.join("model-presets")];
        if let Some(home) = crate::loader::dirs_home() {
            dirs.push(home.join(".purrcode").join("model-presets"));
        }
        dirs.push(workspace_root.join(".purrcode").join("model-presets"));
        dirs
    }

    /// Whether a model-preset file with this name exists in any
    /// discovery directory, regardless of whether it parses.
    pub fn preset_file_exists(name: &str, workspace_root: &Path) -> bool {
        Self::preset_dirs(workspace_root).iter().any(|dir| {
            ["jsonc", "json"]
                .iter()
                .any(|ext| dir.join(format!("{name}.{ext}")).is_file())
        })
    }

    /// Load a model preset by name, returning `None` only when no
    /// preset file with that name exists anywhere. A file that exists
    /// but fails to parse is a hard error: `discover_all` silently
    /// skips unparseable files, so plain `load_by_name(...).ok()`
    /// turned a broken preset into "no preset" and the agent ran with
    /// the wrong (or no) per-agent model settings — the same
    /// silent-swallow class the F21 fix closed for mode presets
    /// (`load_by_name_or_tabby`). The error carries the real parse
    /// failure from the file itself, so the user can fix it.
    pub fn load_by_name_or_none(name: &str, workspace_root: &Path) -> Result<Option<ModelPreset>> {
        match Self::load_by_name(name, workspace_root) {
            Ok(preset) => Ok(Some(preset)),
            Err(e) => {
                if !Self::preset_file_exists(name, workspace_root) {
                    return Ok(None);
                }
                // A file with this name exists but did not load: surface
                // the real parse error from the file, not the generic
                // "not found" that discover_all's silent skip produces.
                for dir in Self::preset_dirs(workspace_root) {
                    for ext in ["jsonc", "json"] {
                        let path = dir.join(format!("{name}.{ext}"));
                        if let Ok(raw) = std::fs::read_to_string(&path) {
                            match Self::parse_str(&raw) {
                                Err(parse_err) => {
                                    return Err(PurrError::Config(format!(
                                        "Model preset '{name}' exists at {} but failed to load: {parse_err}",
                                        path.display()
                                    )));
                                }
                                Ok(preset) => {
                                    // Parses fine but declares a
                                    // different name, so it is not
                                    // loadable as `name` — name the
                                    // mismatch instead of the bare "not
                                    // found", or the user cannot tell a
                                    // wrong name from a missing file
                                    // (re-checker P3, v0.3 polish fix
                                    // pass).
                                    let declared = if preset.name.is_empty() {
                                        path.file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or(name)
                                            .to_string()
                                    } else {
                                        preset.name
                                    };
                                    return Err(PurrError::Config(format!(
                                        "Model preset '{name}' not found; {} declares name '{declared}' (rename the file to {declared}.{ext} or fix its name field)",
                                        path.display()
                                    )));
                                }
                            }
                        }
                    }
                }
                Err(e)
            }
        }
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

    fn model_preset_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "purr-model-preset-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("model-presets")).expect("create model-presets dir");
        dir
    }

    #[test]
    fn test_load_by_name_or_none_missing_is_none() {
        let dir = model_preset_test_dir("missing");
        let found = ModelPresetLoader::load_by_name_or_none("no-such-preset", &dir)
            .expect("a missing preset is None, not an error");
        assert!(found.is_none());
        // Bundled presets need no file: they still load.
        let free =
            ModelPresetLoader::load_by_name_or_none("free", &dir).expect("bundled preset loads");
        assert_eq!(free.map(|p| p.name), Some("free".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_by_name_or_none_broken_file_is_hard_error() {
        // v0.3 polish: a preset file that EXISTS but fails to parse must
        // be a hard error with the real parse failure attached — never a
        // silent "no preset" (the old `.ok()` swallowed it and the agent
        // ran with the wrong per-agent model settings).
        let dir = model_preset_test_dir("broken");
        std::fs::write(
            dir.join("model-presets").join("broken.jsonc"),
            r#"{"$schema": "model-preset/v1", "name": "broken", "agents": {"#,
        )
        .expect("write broken preset file");
        let err = ModelPresetLoader::load_by_name_or_none("broken", &dir)
            .expect_err("an unparseable preset file must fail hard");
        let message = err.to_string();
        assert!(message.contains("failed to load"), "{message}");
        assert!(message.contains("Invalid model-preset JSONC"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_by_name_or_none_name_mismatch_names_the_declared_name() {
        // v0.3 polish fix pass (re-checker P3): a file that parses
        // but declares a different name must say so — the bare "not
        // found" is indistinguishable from a missing file.
        let dir = model_preset_test_dir("mismatch");
        std::fs::write(
            dir.join("model-presets").join("odd.jsonc"),
            r#"{"$schema": "model-preset/v1", "name": "other", "agents": {}}"#,
        )
        .expect("write mismatched preset file");
        let err = ModelPresetLoader::load_by_name_or_none("odd", &dir)
            .expect_err("a preset file declaring another name must fail");
        let message = err.to_string();
        assert!(message.contains("declares name 'other'"), "{message}");
        assert!(message.contains("odd.jsonc"), "{message}");
        // And the declared name still loads under its own name.
        let found = ModelPresetLoader::load_by_name_or_none("other", &dir)
            .expect("the declared name loads");
        assert_eq!(found.map(|p| p.name), Some("other".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_by_name_or_none_valid_custom_loads() {
        let dir = model_preset_test_dir("custom");
        std::fs::write(
            dir.join("model-presets").join("custom-test.jsonc"),
            r#"{"$schema": "model-preset/v1", "name": "custom-test", "agents": {}}"#,
        )
        .expect("write custom preset file");
        let found = ModelPresetLoader::load_by_name_or_none("custom-test", &dir)
            .expect("a valid custom preset loads");
        assert_eq!(found.map(|p| p.name), Some("custom-test".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
