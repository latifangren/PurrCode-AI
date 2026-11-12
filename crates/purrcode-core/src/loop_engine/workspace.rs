// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

pub(crate) fn is_args_outside_workspace(args: &serde_json::Value, workspace_root: &Path) -> bool {
    let candidate_str = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("file"))
        .or_else(|| args.get("workdir"))
        .or_else(|| args.get("target"))
        .or_else(|| args.get("dest"))
        .or_else(|| args.get("source"))
        .or_else(|| args.get("dir"))
        .and_then(|v| v.as_str());

    let Some(raw) = candidate_str else {
        return false;
    };

    let abs_root = if workspace_root.is_absolute() {
        workspace_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace_root)
    };
    let canon_root =
        std::fs::canonicalize(&abs_root).unwrap_or_else(|_| normalize_lexical(&abs_root));
    let p = Path::new(raw);
    let raw_candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        canon_root.join(p)
    };

    let normalized = normalize_lexical(&raw_candidate);
    if !normalized.starts_with(&canon_root) {
        return true;
    }

    // Resolve symlinks on existing target or parent directory to prevent symlink escape
    if normalized.exists() {
        if let Ok(canon_target) = std::fs::canonicalize(&normalized) {
            if !canon_target.starts_with(&canon_root) {
                return true;
            }
        }
    } else if let Some(parent) = normalized.parent() {
        if parent.exists() {
            if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                if !canon_parent.starts_with(&canon_root) {
                    return true;
                }
            }
        }
    }

    false
}
