// SPDX-License-Identifier: GPL-3.0-or-later

use crate::jsonc::clean_jsonc;
use purrcode_api::error::{PurrError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BUNDLED_MODE_PRESETS: &[(&str, &str)] = &[
    ("carrier", include_str!("../../../presets/carrier.jsonc")),
    ("tabby", include_str!("../../../presets/tabby.jsonc")),
    ("oyen", include_str!("../../../presets/oyen.jsonc")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    Manual,
    Supervised,
    Auto,
    Yolo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardrailMode {
    Strict,
    Default,
    God,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    Confinement,
    Os,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModePreset {
    #[serde(
        rename = "$schema",
        alias = "schema",
        default = "default_preset_schema"
    )]
    pub schema: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub approval: ApprovalMode,
    pub guardrail: GuardrailMode,
    pub sandbox: SandboxMode,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_preset_schema() -> String {
    "preset/v1".to_string()
}

pub struct ModePresetLoader;

impl ModePresetLoader {
    pub fn parse_str(raw: &str) -> Result<ModePreset> {
        let cleaned = clean_jsonc(raw);
        let preset: ModePreset = serde_json::from_str(&cleaned)
            .map_err(|e| PurrError::Config(format!("Invalid mode preset JSONC: {e}")))?;
        if preset.schema != "preset/v1" {
            return Err(PurrError::Config(format!(
                "Unsupported mode preset schema '{}' (expected 'preset/v1')",
                preset.schema
            )));
        }
        Ok(preset)
    }

    /// Discovers all mode presets in precedence order:
    /// bundled -> `~/.purrcode/presets/*.jsonc` -> `.purrcode/presets/*.jsonc`
    pub fn discover_all(workspace_root: &Path) -> HashMap<String, ModePreset> {
        let mut map = HashMap::new();

        for (_, raw) in BUNDLED_MODE_PRESETS {
            if let Ok(preset) = Self::parse_str(raw) {
                map.insert(preset.name.clone(), preset);
            }
        }

        Self::scan_dir(&workspace_root.join("presets"), &mut map);
        if let Some(home) = crate::loader::dirs_home() {
            Self::scan_dir(&home.join(".purrcode").join("presets"), &mut map);
        }
        Self::scan_dir(&workspace_root.join(".purrcode").join("presets"), &mut map);

        map
    }

    pub fn load_by_name(name: &str, workspace_root: &Path) -> Result<ModePreset> {
        let mut all = Self::discover_all(workspace_root);
        all.remove(name).ok_or_else(|| {
            PurrError::Config(format!(
                "Mode preset '{}' not found (available: carrier, tabby, oyen, or custom files in presets/)",
                name
            ))
        })
    }

    fn scan_dir(dir: &Path, map: &mut HashMap<String, ModePreset>) {
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
                        if let Ok(preset) = Self::parse_str(&raw) {
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
    fn test_bundled_mode_presets_load() {
        let presets = ModePresetLoader::discover_all(Path::new("."));
        assert!(presets.contains_key("carrier"));
        assert!(presets.contains_key("tabby"));
        assert!(presets.contains_key("oyen"));
        assert_eq!(presets["carrier"].approval, ApprovalMode::Manual);
        assert_eq!(presets["tabby"].approval, ApprovalMode::Supervised);
        assert_eq!(presets["oyen"].guardrail, GuardrailMode::God);
    }
}
