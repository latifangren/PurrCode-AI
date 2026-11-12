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

/// The staged file for one draft.
pub fn draft_path(purrcode_dir: &Path, name: &str) -> PathBuf {
    drafts_dir(purrcode_dir).join(format!("{name}.md"))
}

/// Write the draft atomically (tmp + rename, like the cron store).
/// Overwrites a staged draft of the same name — the curator only ever
/// stages one draft per name at a time.
pub fn stage_draft(purrcode_dir: &Path, name: &str, content: &str) -> Result<PathBuf> {
    let dir = drafts_dir(purrcode_dir);
    std::fs::create_dir_all(&dir).map_err(PurrError::Io)?;
    let path = draft_path(purrcode_dir, name);
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, content).map_err(PurrError::Io)?;
    std::fs::rename(&tmp, &path).map_err(PurrError::Io)?;
    Ok(path)
}

/// Delete a staged draft (on deny, or after a successful save).
/// Missing = ok.
pub fn discard_draft(purrcode_dir: &Path, name: &str) -> Result<()> {
    let path = draft_path(purrcode_dir, name);
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
        let path =
            stage_draft(&home, "my-draft", "---\nname: my-draft\n---\n\nbody\n").expect("stage");
        assert_eq!(path, draft_path(&home, "my-draft"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\nname: my-draft\n---\n\nbody\n"
        );
        // No temp file left behind after the atomic rename.
        assert!(!drafts_dir(&home).join(".my-draft.tmp").exists());
        discard_draft(&home, "my-draft").expect("discard");
        assert!(!path.exists());
        // Discard is idempotent.
        discard_draft(&home, "my-draft").expect("discard twice");
        let _ = std::fs::remove_dir_all(&home);
    }
}
