// SPDX-License-Identifier: GPL-3.0-or-later

//! Skill manager tests (split from `mod.rs` — fix pass B5 r1:
//! `mod.rs` hit 956 lines, over the ADR-011 800 cap).

use super::*;

struct TempTestDir(PathBuf);
impl TempTestDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("purrcode-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&p);
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_embedded_skills_always_available() {
    let mgr = SkillManager::new();
    assert_eq!(mgr.list().len(), 8);
    let body = mgr.read_skill_body("git").unwrap();
    assert!(body.contains("Git Safety Skill"));
}

#[test]
fn test_create_and_read_custom_skill() {
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    let (path, _) = mgr
        .create_skill(
            "docker-deploy",
            "Deployment guidelines for container environments",
            "# Docker Deploy\nRun compose up -d.",
            SkillScope::Project,
            tmp.path(),
        )
        .unwrap();

    assert!(path.exists());
    let meta = mgr.get("docker-deploy").unwrap();
    assert_eq!(meta.source, SkillSource::Project);
    assert_eq!(
        meta.description,
        "Deployment guidelines for container environments"
    );

    let body = mgr.read_skill_body("docker-deploy").unwrap();
    assert!(body.contains("Run compose up -d."));
    assert!(body.contains("name: docker-deploy"));
}

#[test]
fn test_patch_skill_fuzzy_matching() {
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    mgr.create_skill(
        "custom-test",
        "Initial custom test skill",
        "# Test Skill\nStep 1: run cargo test\nStep 2: finish",
        SkillScope::Project,
        tmp.path(),
    )
    .unwrap();

    let (_path, diff, new_body) = mgr
        .patch_skill(
            "custom-test",
            "Step 1: run cargo test",
            "Step 1: run cargo test --workspace",
            None,
            tmp.path(),
        )
        .unwrap();

    assert!(new_body.contains("run cargo test --workspace"));
    assert!(diff.contains("-Step 1: run cargo test"));
    assert!(diff.contains("+Step 1: run cargo test --workspace"));
}

#[test]
fn test_delete_custom_skill_and_embedded_protection() {
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();

    // Attempting to delete embedded bundled skill should fail
    let err = mgr.delete_skill("git", None, tmp.path()).unwrap_err();
    assert!(err
        .to_string()
        .contains("Cannot delete embedded bundled skill"));

    // Creating and deleting custom skill should succeed
    mgr.create_skill(
        "temp-skill",
        "Temporary skill description",
        "Temporary content",
        SkillScope::Project,
        tmp.path(),
    )
    .unwrap();
    assert!(mgr.get("temp-skill").is_some());

    let msg = mgr.delete_skill("temp-skill", None, tmp.path()).unwrap();
    assert!(msg.contains("Successfully deleted skill 'temp-skill'"));
    assert!(mgr.get("temp-skill").is_none());
}

#[test]
fn test_skill_name_and_description_validation() {
    assert!(validate_skill_name("valid-skill_1").is_ok());
    assert!(validate_skill_name("../escape").is_err());
    assert!(validate_skill_name("has/slash").is_err());
    assert!(validate_skill_name("").is_err());

    assert!(validate_skill_description("A concise routing contract").is_ok());
    assert!(validate_skill_description("").is_err());
    let too_long = "a".repeat(501);
    assert!(validate_skill_description(&too_long).is_err());
}

#[test]
fn test_delete_skill_scoped_delete_misses_other_scope() {
    // Fix pass B5 r1 P2-1: a scoped delete must not touch
    // the other scope's copy. The discriminating case needs
    // no fake HOME: deleting the global scope of a name
    // that only exists in the project must 404, not remove
    // the project copy (the old code ignored the scope).
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    mgr.create_skill(
        "purrcode-scope-test",
        "Scope test skill",
        "# Scope\n",
        SkillScope::Project,
        tmp.path(),
    )
    .unwrap();
    let project_file = tmp
        .path()
        .join(".purrcode")
        .join("skills")
        .join("purrcode-scope-test")
        .join("SKILL.md");
    assert!(project_file.exists());
    let err = mgr
        .delete_skill("purrcode-scope-test", Some(SkillScope::Global), tmp.path())
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
    assert!(project_file.exists());
    assert!(mgr.get("purrcode-scope-test").is_some());
}

#[test]
fn test_create_skill_normalizes_frontmatter_name() {
    // Fix pass B5 r1 P2-2: a body whose frontmatter
    // declares another name must not rename the skill —
    // the directory name is the identity.
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    mgr.create_skill(
        "fm-keep",
        "Form description",
        "---\nname: renamed-x\ndescription: Body description\n---\n\n# Body\n",
        SkillScope::Project,
        tmp.path(),
    )
    .unwrap();
    let body = mgr.read_skill_body("fm-keep").unwrap();
    assert!(body.contains("name: fm-keep"));
    assert!(!body.contains("renamed-x"));
    assert!(body.contains("description: Body description"));
    assert!(mgr.get("fm-keep").is_some());
    assert!(mgr.get("renamed-x").is_none());
}

#[test]
fn test_create_skill_rejects_newline_description() {
    // Fix pass B5 r1 P2-3: a newline in the description
    // would be interpolated into the frontmatter, where the
    // parser takes the last `name:`/`description:` line.
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    assert!(mgr
        .create_skill(
            "inj-test",
            "x\nname: pwned",
            "# Body\n",
            SkillScope::Project,
            tmp.path(),
        )
        .is_err());
    assert!(mgr
        .create_skill(
            "inj-test",
            "x\ndescription: INJECTED",
            "# Body\n",
            SkillScope::Project,
            tmp.path(),
        )
        .is_err());
    assert!(mgr.get("inj-test").is_none());
    assert!(validate_skill_description("one\ntwo").is_err());
    assert!(validate_skill_description("one\rtwo").is_err());
}

#[test]
fn test_create_skill_content_cap_counts_characters() {
    // Fix pass B5 r1 P3-5: the 100K cap counts characters,
    // not bytes — the error message already said so.
    let tmp = TempTestDir::new();
    let mut mgr = SkillManager::new();
    // 60,000 emoji = 240,000 bytes but only 60,000 chars.
    let emoji_body = "😀".repeat(60_000);
    assert!(mgr
        .create_skill(
            "emoji-cap",
            "Emoji cap test",
            &emoji_body,
            SkillScope::Project,
            tmp.path(),
        )
        .is_ok());
    let too_many = "é".repeat(100_001);
    let err = mgr
        .create_skill(
            "over-cap",
            "Over cap test",
            &too_many,
            SkillScope::Project,
            tmp.path(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("100,000 characters"));
    assert!(err.to_string().contains("100001"));
}
