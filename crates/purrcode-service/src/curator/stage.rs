// SPDX-License-Identifier: GPL-3.0-or-later

//! Stage a validated skill draft outside the live skill dirs (v0.3
//! F20, ADR-014 item 7). A staged draft is never a live skill until
//! the user approves it and Tinker saves it with `manage_skill`.

use purrcode_api::error::{PurrError, Result};
use std::path::{Path, PathBuf};

/// Drafts live here — one flat file per draft. `discover_all` never
/// reads this directory: it only reads `SKILL.md` inside a skill dir
/// or a top-level `.md` file in `skills/` itself, and `.drafts/` has
/// neither. A staged draft can therefore never leak into the prompt
/// index before the user approves it.
pub fn drafts_dir(purrcode_dir: &Path) -> PathBuf {
    purrcode_dir.join("skills").join(".drafts")
}

/// The staged file for one draft, keyed by session id as well as
/// name: two sessions drafting a skill of the same name must not
/// overwrite each other's staged file. The save turn carries the
/// content inline from the in-memory meta (so a clobbered file could
/// not corrupt a save), but the bookkeeping file is per-session so a
/// discard in one session never deletes another session's draft.
pub fn draft_path(purrcode_dir: &Path, session_id: &str, name: &str) -> PathBuf {
    drafts_dir(purrcode_dir).join(format!("{name}.{session_id}.md"))
}

/// Write the draft atomically (tmp + rename, like the cron store).
/// The tmp file is session-keyed too, so concurrent stages of the
/// same name cannot clobber each other's rename.
pub fn stage_draft(
    purrcode_dir: &Path,
    session_id: &str,
    name: &str,
    content: &str,
) -> Result<PathBuf> {
    let dir = drafts_dir(purrcode_dir);
    std::fs::create_dir_all(&dir).map_err(PurrError::Io)?;
    let path = draft_path(purrcode_dir, session_id, name);
    let tmp = dir.join(format!(".{name}.{session_id}.tmp"));
    std::fs::write(&tmp, content).map_err(PurrError::Io)?;
    std::fs::rename(&tmp, &path).map_err(PurrError::Io)?;
    Ok(path)
}

/// Delete a staged draft (on deny, or after a successful save).
/// Missing = ok.
pub fn discard_draft(purrcode_dir: &Path, session_id: &str, name: &str) -> Result<()> {
    let path = draft_path(purrcode_dir, session_id, name);
    if path.exists() {
        std::fs::remove_file(&path).map_err(PurrError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "purr-curator-stage-test-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_stage_and_discard_roundtrip() {
        let home = tmp_home();
        let sid = "purr_stage_a1b2c3";
        let path = stage_draft(&home, sid, "my-draft", "---\nname: my-draft\n---\n\nbody\n")
            .expect("stage");
        assert_eq!(path, draft_path(&home, sid, "my-draft"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\nname: my-draft\n---\n\nbody\n"
        );
        // No temp file left behind after the atomic rename.
        assert!(!drafts_dir(&home)
            .join(".my-draft.purr_stage_a1b2c3.tmp")
            .exists());
        // A second session staging the same name gets its own file.
        let other = stage_draft(
            &home,
            "purr_stage_d4e5f6",
            "my-draft",
            "---\nname: my-draft\n---\n\nother body\n",
        )
        .expect("stage second session");
        assert_ne!(other, path);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\nname: my-draft\n---\n\nbody\n"
        );
        discard_draft(&home, sid, "my-draft").expect("discard");
        assert!(!path.exists());
        // The other session's draft survives this discard.
        assert!(other.exists());
        // Discard is idempotent.
        discard_draft(&home, sid, "my-draft").expect("discard twice");
        let _ = std::fs::remove_dir_all(&home);
    }
}
