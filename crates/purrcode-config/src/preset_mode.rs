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
        // ADR-014 item 8: `sandbox: "os"` is parsed but unenforced until
        // the F21 OS sandbox lands — today it silently WEAKENS confinement
        // (the outside-workspace denial only fires for `confinement`).
        // Reject it loudly at load time instead of letting a preset claim
        // protection it does not have.
        if preset.sandbox == SandboxMode::Os {
            return Err(PurrError::Config(format!(
                "Mode preset '{}' declares sandbox 'os', which is not enforced yet (v0.3 F21 OS sandbox). Use sandbox 'confinement' until then.",
                preset.name
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
        if let Some(preset) = all.remove(name) {
            return Ok(preset);
        }
        // The preset may exist on disk but fail to parse (e.g. it declares
        // sandbox "os", rejected until the F21 OS sandbox lands). Surface
        // the REAL parse error instead of a misleading "not found".
        let mut candidate_dirs = vec![workspace_root.join("presets")];
        if let Some(home) = crate::loader::dirs_home() {
            candidate_dirs.push(home.join(".purrcode").join("presets"));
        }
        candidate_dirs.push(workspace_root.join(".purrcode").join("presets"));
        for dir in candidate_dirs {
            for ext in ["jsonc", "json"] {
                let path = dir.join(format!("{name}.{ext}"));
                if path.is_file() {
                    let raw = std::fs::read_to_string(&path).map_err(|e| {
                        PurrError::Config(format!("Failed to read preset {}: {e}", path.display()))
                    })?;
                    return Self::parse_str(&raw);
                }
            }
        }
        Err(PurrError::Config(format!(
            "Mode preset '{name}' not found (available: carrier, tabby, oyen, or custom files in presets/)"
        )))
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

    #[test]
    fn test_sandbox_os_preset_is_rejected_until_f21() {
        // ADR-014 item 8: `sandbox: "os"` parses but is unenforced today —
        // it must fail loudly at load time, never silently weaken
        // confinement. A preset with `confinement` still loads fine.
        let os_raw = r#"{
            "$schema": "preset/v1", "name": "os-test",
            "approval": "supervised", "guardrail": "default",
            "sandbox": "os", "tools": ["read"]
        }"#;
        let err = ModePresetLoader::parse_str(os_raw)
            .expect_err("sandbox 'os' must be rejected until F21 lands");
        assert!(err.to_string().contains("not enforced yet"));

        let ok_raw = os_raw.replace("\"os\"", "\"confinement\"");
        let preset = ModePresetLoader::parse_str(&ok_raw).expect("confinement preset loads");
        assert_eq!(preset.sandbox, SandboxMode::Confinement);
    }

    #[test]
    fn test_load_by_name_surfaces_os_rejection() {
        // `discover_all` silently skips unparseable presets, so a preset
        // file declaring sandbox "os" would look "not found" from
        // `load_by_name` — the real rejection must surface instead.
        let dir = std::env::temp_dir().join(format!(
            "purr-preset-os-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let presets_dir = dir.join("presets");
        std::fs::create_dir_all(&presets_dir).expect("create presets dir");
        std::fs::write(
            presets_dir.join("os-test.jsonc"),
            r#"{
                "$schema": "preset/v1", "name": "os-test",
                "approval": "supervised", "guardrail": "default",
                "sandbox": "os", "tools": ["read"]
            }"#,
        )
        .expect("write preset file");
        let err = ModePresetLoader::load_by_name("os-test", &dir)
            .expect_err("os preset must be rejected with the real reason");
        assert!(
            err.to_string().contains("not enforced yet"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
