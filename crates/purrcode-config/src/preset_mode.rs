// SPDX-License-Identifier: GPL-3.0-or-later

use crate::jsonc::clean_jsonc;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::SandboxStrictness;
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
    /// Strictness of the OS sandbox, only meaningful when
    /// `sandbox == Os` (ADR-014 item 8, v0.3 F21). Defaults to
    /// `ReadOnly` (fail-closed) when a preset declares `"os"` without
    /// naming a mode. Ignored for `confinement` presets.
    #[serde(default)]
    pub sandbox_mode: SandboxStrictness,
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
        // ADR-014 item 8 / v0.3 F21: `sandbox: "os"` is now enforced
        // (Landlock ruleset on every `bash` child, via `ToolContext`).
        // The pre-F21 loader rejected it; that rejection is gone.
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
        // The preset may exist on disk but fail to parse (e.g. an
        // unknown sandbox mode). Surface the REAL parse error instead
        // of a misleading "not found".
        for dir in Self::preset_dirs(workspace_root) {
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

    /// Directories a preset file may live in, in discovery order
    /// (same order as `discover_all` / `load_by_name`).
    fn preset_dirs(workspace_root: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![workspace_root.join("presets")];
        if let Some(home) = crate::loader::dirs_home() {
            dirs.push(home.join(".purrcode").join("presets"));
        }
        dirs.push(workspace_root.join(".purrcode").join("presets"));
        dirs
    }

    /// Whether a preset file with this name exists in any discovery
    /// directory, regardless of whether it parses.
    pub fn preset_file_exists(name: &str, workspace_root: &Path) -> bool {
        Self::preset_dirs(workspace_root).iter().any(|dir| {
            ["jsonc", "json"]
                .iter()
                .any(|ext| dir.join(format!("{name}.{ext}")).is_file())
        })
    }

    /// Load a preset by name, falling back to bundled tabby ONLY when
    /// no preset file with that name exists anywhere. A preset file
    /// that exists but fails to parse (e.g. an unknown `sandbox_mode`)
    /// is a hard error: silently falling back would drop a protection
    /// the user explicitly declared — and run the turn without the OS
    /// sandbox they asked for (v0.3 F21, re-checker P1-1). Same pattern
    /// the heartbeat tick fixed in F22 (a load failure must never
    /// silently become a different preset).
    pub fn load_by_name_or_tabby(name: &str, workspace_root: &Path) -> Result<ModePreset> {
        match Self::load_by_name(name, workspace_root) {
            Ok(preset) => Ok(preset),
            Err(e) => {
                if Self::preset_file_exists(name, workspace_root) {
                    Err(e)
                } else {
                    Self::parse_str(include_str!("../../../presets/tabby.jsonc"))
                }
            }
        }
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
    fn test_sandbox_os_preset_is_accepted_after_f21() {
        // ADR-014 item 8 / v0.3 F21: `sandbox: "os"` is now enforced
        // (Landlock on every `bash` child). A preset declaring it loads,
        // and without an explicit `sandbox_mode` it defaults to the
        // strictest ruleset (`read-only`, fail-closed).
        let os_raw = r#"{
            "$schema": "preset/v1", "name": "os-test",
            "approval": "supervised", "guardrail": "default",
            "sandbox": "os", "tools": ["read"]
        }"#;
        let preset = ModePresetLoader::parse_str(os_raw).expect("os preset loads after F21");
        assert_eq!(preset.sandbox, SandboxMode::Os);
        assert_eq!(preset.sandbox_mode, SandboxStrictness::ReadOnly);

        let write_raw = os_raw.replace(
            "\"sandbox\": \"os\"",
            "\"sandbox\": \"os\", \"sandbox_mode\": \"workspace-write\"",
        );
        let preset = ModePresetLoader::parse_str(&write_raw).expect("explicit sandbox_mode loads");
        assert_eq!(preset.sandbox_mode, SandboxStrictness::WorkspaceWrite);
        assert_eq!(preset.sandbox_mode.as_kebab_str(), "workspace-write");
        assert_eq!(
            SandboxStrictness::from_kebab("danger-full-access"),
            Some(SandboxStrictness::DangerFullAccess)
        );
        assert_eq!(SandboxStrictness::from_kebab("bogus"), None);
    }

    #[test]
    fn test_sandbox_os_unknown_mode_is_rejected() {
        // A misspelled `sandbox_mode` must fail loudly at load time,
        // never silently fall back to a looser ruleset.
        let raw = r#"{
            "$schema": "preset/v1", "name": "os-test",
            "approval": "supervised", "guardrail": "default",
            "sandbox": "os", "sandbox_mode": "everything",
            "tools": ["read"]
        }"#;
        let err =
            ModePresetLoader::parse_str(raw).expect_err("unknown sandbox_mode must be rejected");
        assert!(err.to_string().contains("Invalid mode preset JSONC"));
    }

    #[test]
    fn test_load_by_name_loads_os_preset_after_f21() {
        // `discover_all` silently skips unparseable presets, so a preset
        // file declaring sandbox "os" must load through `load_by_name`
        // like any other preset — with its explicit mode intact.
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
                "sandbox": "os", "sandbox_mode": "workspace-write",
                "tools": ["read"]
            }"#,
        )
        .expect("write preset file");
        let preset = ModePresetLoader::load_by_name("os-test", &dir)
            .expect("os preset loads through load_by_name after F21");
        assert_eq!(preset.sandbox, SandboxMode::Os);
        assert_eq!(preset.sandbox_mode, SandboxStrictness::WorkspaceWrite);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_preset_file_exists_distinguishes_present_from_absent() {
        let dir = std::env::temp_dir().join(format!(
            "purr-preset-exists-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let presets_dir = dir.join("presets");
        std::fs::create_dir_all(&presets_dir).expect("create presets dir");
        assert!(!ModePresetLoader::preset_file_exists("os-bad", &dir));
        std::fs::write(presets_dir.join("os-bad.jsonc"), "{}").expect("write preset file");
        assert!(ModePresetLoader::preset_file_exists("os-bad", &dir));
        assert!(!ModePresetLoader::preset_file_exists("other", &dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_by_name_or_tabby_fails_hard_on_unparseable_preset() {
        // F21 re-checker P1-1: a preset file that EXISTS but fails to
        // parse (unknown sandbox_mode) must be a hard error, never a
        // silent fallback to tabby — the fallback would run the turn
        // without the OS sandbox the user declared.
        let dir = std::env::temp_dir().join(format!(
            "purr-preset-or-tabby-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let presets_dir = dir.join("presets");
        std::fs::create_dir_all(&presets_dir).expect("create presets dir");
        std::fs::write(
            presets_dir.join("os-bad.jsonc"),
            r#"{
                "$schema": "preset/v1", "name": "os-bad",
                "approval": "supervised", "guardrail": "default",
                "sandbox": "os", "sandbox_mode": "everything",
                "tools": ["read"]
            }"#,
        )
        .expect("write preset file");
        let err = ModePresetLoader::load_by_name_or_tabby("os-bad", &dir)
            .expect_err("unparseable preset fails hard");
        assert!(err.to_string().contains("Invalid mode preset JSONC"));
        // A name with no preset file anywhere still falls back to tabby
        // (unchanged pre-F21 behaviour for a typo / missing preset).
        let fallback = ModePresetLoader::load_by_name_or_tabby("missing-preset", &dir)
            .expect("missing preset falls back to tabby");
        assert_eq!(fallback.name, "tabby");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
