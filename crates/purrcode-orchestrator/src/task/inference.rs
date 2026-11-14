// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_config::ModelPreset;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRoleInference {
    pub model: String,
    pub fallback_candidates: Vec<String>,
    pub temperature: Option<f32>,
    pub reasoning_effort: Option<String>,
}

/// Resolves the inference settings (model, fallback candidates, temperature, reasoning effort)
/// for a subagent role according to ARCHITECTURE.md §3.5 precedence:
/// 1. Explicit `model` in `task()` (disables fallback chain).
/// 2. Session `ModelPreset` entry for `role`.
/// 3. Frontmatter `model:` in `.purrcode/agents/<role>.md` or `agents/<role>.md`.
/// 4. Session model (with `reasoning_effort = Some("high")` for `tuxeshadow` and `sphinx` per ADR-012).
pub fn resolve_role_model_spec(
    role: &str,
    explicit_model: Option<&str>,
    model_preset: Option<&ModelPreset>,
    session_model: &str,
    workspace_root: &Path,
) -> ResolvedRoleInference {
    let default_effort = if role == "tuxeshadow" || role == "sphinx" {
        Some("high".to_string())
    } else {
        None
    };

    // 1. Explicit model override in task()
    if let Some(exp) = explicit_model {
        let trimmed = exp.trim();
        if !trimmed.is_empty() {
            return ResolvedRoleInference {
                model: trimmed.to_string(),
                fallback_candidates: vec![trimmed.to_string()],
                temperature: None,
                reasoning_effort: default_effort,
            };
        }
    }

    // 2. Session ModelPreset entry for role
    if let Some(preset) = model_preset {
        let role_lower = role.to_ascii_lowercase();
        if let Some(spec) = preset
            .agents
            .get(role)
            .or_else(|| preset.agents.get(&role_lower))
        {
            let mut candidates = spec.model.candidates();
            for fb in &spec.fallback {
                if !candidates.contains(fb) {
                    candidates.push(fb.clone());
                }
            }
            let primary = candidates
                .first()
                .cloned()
                .unwrap_or_else(|| session_model.to_string());
            return ResolvedRoleInference {
                model: primary,
                fallback_candidates: candidates,
                temperature: spec.temperature,
                reasoning_effort: spec.variant.clone().or(default_effort),
            };
        }
    }

    // 3. Agent Markdown frontmatter (`model:`)
    if let Some(fm_model) = read_agent_frontmatter_model(role, workspace_root) {
        return ResolvedRoleInference {
            model: fm_model.clone(),
            fallback_candidates: vec![fm_model],
            temperature: None,
            reasoning_effort: default_effort,
        };
    }

    // 4. Session model fallback
    ResolvedRoleInference {
        model: session_model.to_string(),
        fallback_candidates: vec![session_model.to_string()],
        temperature: None,
        reasoning_effort: default_effort,
    }
}

fn read_agent_frontmatter_model(role: &str, workspace_root: &Path) -> Option<String> {
    let candidates = [
        workspace_root
            .join(".purrcode")
            .join("agents")
            .join(format!("{}.md", role)),
        workspace_root.join("agents").join(format!("{}.md", role)),
    ];
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Some(rest) = content.strip_prefix("---") {
                if let Some(end_idx) = rest.find("---") {
                    for line in rest[..end_idx].lines() {
                        if let Some(m) = line.trim().strip_prefix("model:") {
                            let val = m.trim().trim_matches('"').trim_matches('\'');
                            if !val.is_empty() {
                                return Some(val.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrcode_config::{AgentModelSpec, ModelPreset, ModelTarget};
    use std::collections::HashMap;

    #[test]
    fn test_inheritance_without_preset() {
        let dummy_root = Path::new("/tmp");
        let res = resolve_role_model_spec(
            "tinker",
            None,
            None,
            "magpie:gemini-3.8-flash-tiered",
            dummy_root,
        );
        assert_eq!(res.model, "magpie:gemini-3.8-flash-tiered");
        assert_eq!(res.temperature, None);
        assert_eq!(res.reasoning_effort, None);

        let tux = resolve_role_model_spec(
            "tuxeshadow",
            None,
            None,
            "magpie:gemini-3.8-flash-tiered",
            dummy_root,
        );
        assert_eq!(tux.model, "magpie:gemini-3.8-flash-tiered");
        assert_eq!(tux.temperature, None);
        assert_eq!(tux.reasoning_effort, Some("high".to_string()));
    }

    #[test]
    fn test_preset_inheritance_and_temperature() {
        let dummy_root = Path::new("/tmp");
        let mut agents = HashMap::new();
        agents.insert(
            "calico".to_string(),
            AgentModelSpec {
                model: ModelTarget::Single("claude-3-5-sonnet".to_string()),
                variant: None,
                temperature: Some(0.5),
                options: None,
                fallback: vec![],
            },
        );
        let preset = ModelPreset {
            schema: "model-preset/v1".to_string(),
            name: "test".to_string(),
            description: "Test preset".to_string(),
            extends: None,
            agents,
        };

        // Calico is specified with temperature
        let res_calico = resolve_role_model_spec(
            "Calico", // testing case-insensitivity
            None,
            Some(&preset),
            "magpie:gemini-3.8-flash-tiered",
            dummy_root,
        );
        assert_eq!(res_calico.model, "claude-3-5-sonnet");
        assert_eq!(res_calico.temperature, Some(0.5));

        // Tinker is not in preset, inherits session_model with None temperature
        let res_tinker = resolve_role_model_spec(
            "Tinker",
            None,
            Some(&preset),
            "magpie:gemini-3.8-flash-tiered",
            dummy_root,
        );
        assert_eq!(res_tinker.model, "magpie:gemini-3.8-flash-tiered");
        assert_eq!(res_tinker.temperature, None);
    }
}
