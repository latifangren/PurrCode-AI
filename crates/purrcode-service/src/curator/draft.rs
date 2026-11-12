// SPDX-License-Identifier: GPL-3.0-or-later

//! Parse + validate a skill draft the curator model produced (v0.3
//! F20, ADR-014 item 7). The same name/description rules as
//! `manage_skill action=create` apply here — a draft that would fail
//! the tool is rejected before it is ever shown to the user.

use purrcode_approval::scrub_secrets;
use purrcode_tools::skill::{
    parse_skill_frontmatter, validate_skill_description, validate_skill_name, SkillManager,
};

/// A validated draft, ready to stage.
#[derive(Debug, Clone)]
pub struct DraftMeta {
    pub name: String,
    pub description: String,
    pub content: String,
}

/// Strip one optional ``` fence (bare, or with a language tag) from
/// around the model's reply. A reply with no fence is returned trimmed.
fn strip_fence(t: &str) -> &str {
    let mut s = t.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start();
        // Drop a one-word language tag on the fence's first line
        // (e.g. ```markdown). The tag must look like a language name
        // — a leading `---` is the SKILL.md frontmatter, not a tag.
        let rest = match rest.split_once('\n') {
            Some((first, tail))
                if !first.is_empty()
                    && first
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && first
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic()) =>
            {
                tail
            }
            _ => rest,
        };
        s = rest.trim_end();
        if let Some(inner) = s.strip_suffix("```") {
            s = inner.trim_end();
        }
    }
    s
}

/// The curator answers either exactly `NO_SKILL` (nothing repeatable)
/// or a complete SKILL.md file. Returns `None` for NO_SKILL (and for
/// an empty reply — treated as "nothing to draft"), `Some(content)`
/// otherwise. Content is the model's raw output: the caller validates
/// it before staging.
pub fn parse_draft_response(text: &str) -> Option<String> {
    let t = strip_fence(text.trim());
    if t.is_empty() {
        return None;
    }
    // "NO_SKILL" — also matches "NO_SKILL — nothing repeatable".
    if t.len() >= 8 && t[..8].eq_ignore_ascii_case("NO_SKILL") {
        return None;
    }
    Some(t.to_string())
}

/// Validate a candidate draft. Rejects: no `---` frontmatter, an
/// unclosed frontmatter, an invalid name/description (same rules as
/// `manage_skill`), an empty body, a name that already exists as a
/// live skill, content over the 100,000-char cap, and anything the
/// audit's secret scanner redacts (a draft must never carry a secret).
/// True when the frontmatter block contains a `name:` line with a
/// non-empty value. Mirrors the `name:` scan in
/// `parse_skill_frontmatter` (same prefix, same trimming) so the two
/// never disagree about what counts as a name. A draft without one
/// would silently become the skill literally named "draft".
fn frontmatter_has_name(content: &str) -> bool {
    let Some(rest) = content.strip_prefix("---") else {
        return false;
    };
    let Some(end) = rest.find("---") else {
        return false;
    };
    rest[..end].lines().any(|line| {
        line.trim()
            .strip_prefix("name:")
            .is_some_and(|v| !v.trim().is_empty())
    })
}

pub fn validate_draft(content: &str, skill_mgr: &SkillManager) -> Result<DraftMeta, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("draft is empty".to_string());
    }
    if trimmed.len() > 100_000 {
        return Err(format!(
            "draft exceeds 100,000 characters (was {})",
            trimmed.len()
        ));
    }
    if !trimmed.starts_with("---") {
        return Err("draft has no SKILL.md frontmatter (must start with '---')".to_string());
    }
    let Some(rest) = trimmed.strip_prefix("---") else {
        return Err("draft has no SKILL.md frontmatter (must start with '---')".to_string());
    };
    let Some(end) = rest.find("---") else {
        return Err("draft frontmatter is not closed with '---'".to_string());
    };
    let body = &rest[end + 3..];
    if body.trim().is_empty() {
        return Err("draft has no body after the frontmatter".to_string());
    }
    if !frontmatter_has_name(trimmed) {
        return Err(
            "draft frontmatter has no 'name:' field (a draft must name its skill explicitly)"
                .to_string(),
        );
    }
    let (name, description) = parse_skill_frontmatter(trimmed, "draft");
    let name = validate_skill_name(&name).map_err(|m| format!("invalid draft name: {m}"))?;
    let description = validate_skill_description(&description)
        .map_err(|m| format!("invalid draft description: {m}"))?;
    if skill_mgr.get(&name).is_some() {
        return Err(format!("a live skill named '{name}' already exists"));
    }
    if scrub_secrets(trimmed) != trimmed {
        return Err("draft contains a secret-like value (refused by the secret scan)".to_string());
    }
    Ok(DraftMeta {
        name,
        description,
        content: trimmed.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> SkillManager {
        SkillManager::new()
    }

    const GOOD: &str = "---\nname: cargo-test-loop\ndescription: Run tests, fix failures, re-run until green.\n---\n\n# Cargo test loop\n\n## When to use\nWhen tests fail.\n\n## Steps\n1. Run `cargo test`.\n2. Fix the first failure.\n3. Re-run until green.\n";

    #[test]
    fn test_parse_draft_response_no_skill_forms() {
        assert_eq!(parse_draft_response("NO_SKILL"), None);
        assert_eq!(parse_draft_response("no_skill"), None);
        assert_eq!(
            parse_draft_response("NO_SKILL — nothing repeatable here."),
            None
        );
        assert_eq!(parse_draft_response(""), None);
        assert_eq!(parse_draft_response("   \n"), None);
    }

    #[test]
    fn test_parse_draft_response_strips_fence() {
        // The parser trims the reply, so the trailing newline of GOOD
        // is not preserved — the draft content is what matters.
        let want = GOOD.trim_end();
        let fenced = format!("```markdown\n{GOOD}\n```");
        assert_eq!(parse_draft_response(&fenced).as_deref(), Some(want));
        let bare = format!("```\n{GOOD}\n```");
        assert_eq!(parse_draft_response(&bare).as_deref(), Some(want));
        assert_eq!(parse_draft_response(GOOD).as_deref(), Some(want));
    }

    #[test]
    fn test_validate_draft_good() {
        let meta = validate_draft(GOOD, &mgr()).expect("good draft validates");
        assert_eq!(meta.name, "cargo-test-loop");
        assert_eq!(
            meta.description,
            "Run tests, fix failures, re-run until green."
        );
    }

    #[test]
    fn test_validate_draft_rejects_no_frontmatter_and_empty_body() {
        assert!(validate_draft("# No frontmatter", &mgr()).is_err());
        let no_body = "---\nname: empty-body\ndescription: Nothing after the fence.\n---\n";
        assert!(validate_draft(no_body, &mgr()).is_err());
        let unclosed = "---\nname: unclosed\ndescription: No closing fence.\n";
        assert!(validate_draft(unclosed, &mgr()).is_err());
    }

    #[test]
    fn test_validate_draft_rejects_bad_name_and_duplicate() {
        let bad_name = GOOD.replace("cargo-test-loop", "Bad Name With Spaces");
        assert!(validate_draft(&bad_name, &mgr()).is_err());
        let mut m = mgr();
        // A live skill with this name exists: the draft is a duplicate.
        let dir = std::env::temp_dir().join(format!(
            "purr-curator-test-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let _ = std::fs::remove_dir_all(&dir);
        m.create_skill(
            "cargo-test-loop",
            "Run tests, fix failures, re-run until green.",
            GOOD,
            purrcode_tools::skill::SkillScope::Project,
            &dir,
        )
        .expect("seed live skill");
        assert!(validate_draft(GOOD, &m).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_draft_rejects_missing_name_field() {
        // No `name:` line at all: the frontmatter parser would
        // default the name to "draft" — the draft is rejected
        // instead of silently becoming a skill named "draft".
        let no_name = "---\ndescription: No name here.\n---\n\n# Body\n";
        assert!(validate_draft(no_name, &mgr()).is_err());
        // An empty `name:` value is as good as missing.
        let empty_name = "---\nname:   \ndescription: Empty name.\n---\n\n# Body\n";
        assert!(validate_draft(empty_name, &mgr()).is_err());
        // An explicit `name: draft` is a deliberate choice and
        // validates (the duplicate-name check catches a real clash).
        let named_draft = "---\nname: draft\ndescription: Explicitly named draft.\n---\n\n# Body\n";
        assert!(validate_draft(named_draft, &mgr()).is_ok());
    }

    #[test]
    fn test_validate_draft_rejects_secrets() {
        let with_secret = GOOD.replace(
            "Run `cargo test`.",
            "Run `cargo test` with API_KEY=sk-live-1234567890abcdef.",
        );
        assert!(validate_draft(&with_secret, &mgr()).is_err());
    }
}
