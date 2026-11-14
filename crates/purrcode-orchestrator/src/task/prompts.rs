// SPDX-License-Identifier: GPL-3.0-or-later

//! Role prompts for PurrCode specialized subagents (The Clowder).
//!
//! Single source of truth: built-in defaults are embedded from `agents/*.md`
//! at compile time (stripping YAML frontmatter) and can be overridden per
//! workspace via `.purrcode/agents/<role>.md`.

use std::path::Path;

const WHISKER_MD: &str = include_str!("../../../../agents/whisker.md");
const SPHINX_MD: &str = include_str!("../../../../agents/sphinx.md");
const CALICO_MD: &str = include_str!("../../../../agents/calico.md");
const TINKER_MD: &str = include_str!("../../../../agents/tinker.md");
const TUXESHADOW_MD: &str = include_str!("../../../../agents/tuxeshadow.md");
const CLOWDER_MD: &str = include_str!("../../../../agents/clowder.md");

const GENERIC_SUBAGENT_PROMPT: &str = r#"<identity>
You are a specialized subagent in PurrCode AI.
</identity>

<rules>
1. Read target files and tests before editing; never fabricate file paths, symbols, or outputs.
2. Make minimal, root-cause changes that preserve existing worktree state and invariants.
3. Verify your work with concrete checks and report changed `path:line`, verification results, and any blockers honestly.
</rules>"#;

/// Strips optional leading YAML frontmatter (`--- ... ---`) from a Markdown agent spec.
pub fn strip_yaml_frontmatter(raw: &str) -> &str {
    let trimmed_start = raw.trim_start_matches('\u{feff}');
    if let Some(after_open) = trimmed_start
        .strip_prefix("---\n")
        .or_else(|| trimmed_start.strip_prefix("---\r\n"))
    {
        if let Some(end_idx) = after_open.find("\n---") {
            let after_close = &after_open[end_idx + 4..];
            return after_close
                .strip_prefix("\r\n")
                .or_else(|| after_close.strip_prefix('\n'))
                .unwrap_or(after_close)
                .trim_start_matches(['\r', '\n']);
        }
    }
    trimmed_start
}

/// Builds the specialized system prompt for a Clowder subagent role using embedded defaults.
pub fn build_role_system_prompt(role: &str, subagent_id: &str) -> String {
    build_role_system_prompt_in(role, subagent_id, None)
}

/// Builds the specialized system prompt for a Clowder subagent role, honoring
/// `.purrcode/agents/<role>.md` project overrides when `workspace_root` is provided.
pub fn build_role_system_prompt_in(
    role: &str,
    subagent_id: &str,
    workspace_root: Option<&Path>,
) -> String {
    let role_lower = role.to_ascii_lowercase();

    let override_body = workspace_root.and_then(|ws| {
        let candidate = ws
            .join(".purrcode")
            .join("agents")
            .join(format!("{role_lower}.md"));
        std::fs::read_to_string(candidate).ok().and_then(|raw| {
            let stripped = strip_yaml_frontmatter(&raw).trim().to_string();
            if stripped.is_empty() {
                None
            } else {
                Some(stripped)
            }
        })
    });

    let body = match override_body {
        Some(custom) => custom,
        None => {
            let raw = match role_lower.as_str() {
                "whisker" => Some(WHISKER_MD),
                "sphinx" => Some(SPHINX_MD),
                "calico" => Some(CALICO_MD),
                "tinker" => Some(TINKER_MD),
                "tuxeshadow" => Some(TUXESHADOW_MD),
                "clowder" => Some(CLOWDER_MD),
                _ => None,
            };
            match raw {
                Some(md) => strip_yaml_frontmatter(md).trim().to_string(),
                None => GENERIC_SUBAGENT_PROMPT.to_string(),
            }
        }
    };

    let env_block = purrcode_core::format_environment_block(workspace_root);
    format!(
        "You are {role}, a specialized subagent in PurrCode AI.\n\
         Continuation ID: '{subagent_id}'\n\n\
         {body}\n\n\
         {harness}\n\n\
         {env_block}",
        harness = purrcode_core::CORE_HARNESS_CONTRACT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_frontmatter_removes_yaml_header() {
        let md = "---\nname: tinker\ntools:\n  - read\n---\n\n<identity>Hi</identity>\n";
        assert_eq!(strip_yaml_frontmatter(md).trim(), "<identity>Hi</identity>");
    }

    #[test]
    fn embedded_prompts_are_clean_and_aligned() {
        let tinker = build_role_system_prompt("tinker", "purr_1");
        assert!(!tinker.contains("permissionMode:"));
        assert!(tinker.contains("Continuation ID: 'purr_1'"));
        assert!(tinker.contains("<verification>"));

        let tux = build_role_system_prompt("tuxeshadow", "purr_2");
        assert!(tux.contains("APPROVE"));
        assert!(tux.contains("APPROVE_WITH_NOTES"));
        assert!(tux.contains("OBJECT"));

        let calico = build_role_system_prompt("calico", "purr_3");
        assert!(!calico.contains("Catppuccin Mocha"));
    }

    #[test]
    fn workspace_override_takes_precedence() {
        let dir = std::env::temp_dir().join(format!("purr_prompt_ov_{}", uuid::Uuid::new_v4()));
        let agents_dir = dir.join(".purrcode").join("agents");
        std::fs::create_dir_all(&agents_dir).expect("create agents override dir");
        std::fs::write(
            agents_dir.join("tinker.md"),
            "---\nname: tinker\n---\nCustom project Tinker prompt.",
        )
        .expect("write override");

        let prompt = build_role_system_prompt_in("tinker", "purr_ov", Some(&dir));
        assert!(prompt.contains("Custom project Tinker prompt."));
        let _ = std::fs::remove_dir_all(dir);
    }
}
