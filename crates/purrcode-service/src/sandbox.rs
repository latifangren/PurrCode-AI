// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OsSandboxMode {
    ReadOnly,
    #[default]
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsSandboxRuntime {
    Bubblewrap(PathBuf),
    Seatbelt(PathBuf),
    Confinement,
}

pub struct OsSandboxAdapter {
    mode: OsSandboxMode,
    runtime: OsSandboxRuntime,
}

fn find_executable_in_path(prog_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(prog_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{}.exe", prog_name));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
        }
    }
    None
}

impl OsSandboxAdapter {
    pub fn new(mode: OsSandboxMode) -> Self {
        let runtime = Self::detect_runtime();
        Self { mode, runtime }
    }

    pub fn with_runtime(mode: OsSandboxMode, runtime: OsSandboxRuntime) -> Self {
        Self { mode, runtime }
    }

    pub fn mode(&self) -> OsSandboxMode {
        self.mode
    }

    pub fn runtime(&self) -> &OsSandboxRuntime {
        &self.runtime
    }

    pub fn detect_runtime() -> OsSandboxRuntime {
        #[cfg(target_os = "linux")]
        {
            if let Some(path) = find_executable_in_path("bwrap") {
                return OsSandboxRuntime::Bubblewrap(path);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = find_executable_in_path("sandbox-exec") {
                return OsSandboxRuntime::Seatbelt(path);
            }
        }
        OsSandboxRuntime::Confinement
    }

    /// Wraps a command according to the configured sandbox mode and detected OS runtime
    pub fn wrap_command(
        &self,
        command: &str,
        workspace_root: &Path,
        cwd: &Path,
    ) -> Result<(String, Vec<String>)> {
        match self.mode {
            OsSandboxMode::DangerFullAccess => {
                #[cfg(windows)]
                {
                    Ok((
                        "powershell".to_string(),
                        vec!["-Command".to_string(), command.to_string()],
                    ))
                }
                #[cfg(not(windows))]
                {
                    Ok((
                        "sh".to_string(),
                        vec!["-c".to_string(), command.to_string()],
                    ))
                }
            }
            OsSandboxMode::ReadOnly => match &self.runtime {
                OsSandboxRuntime::Bubblewrap(bwrap_bin) => {
                    let root_str = workspace_root.to_string_lossy().to_string();
                    let cwd_str = cwd.to_string_lossy().to_string();
                    let args = vec![
                        "--ro-bind".to_string(),
                        "/".to_string(),
                        "/".to_string(),
                        "--dev".to_string(),
                        "/dev".to_string(),
                        "--proc".to_string(),
                        "/proc".to_string(),
                        "--tmpfs".to_string(),
                        "/tmp".to_string(),
                        "--ro-bind".to_string(),
                        root_str.clone(),
                        root_str,
                        "--chdir".to_string(),
                        cwd_str,
                        "--unshare-all".to_string(),
                        "--share-net".to_string(),
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ];
                    Ok((bwrap_bin.to_string_lossy().to_string(), args))
                }
                OsSandboxRuntime::Seatbelt(seatbelt_bin) => {
                    let profile = "(version 1) (allow default) (deny file-write*)";
                    let args = vec![
                        "-p".to_string(),
                        profile.to_string(),
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ];
                    Ok((seatbelt_bin.to_string_lossy().to_string(), args))
                }
                OsSandboxRuntime::Confinement => {
                    // Fallback to standard shell with path confinement
                    Ok((
                        "sh".to_string(),
                        vec!["-c".to_string(), command.to_string()],
                    ))
                }
            },
            OsSandboxMode::WorkspaceWrite => match &self.runtime {
                OsSandboxRuntime::Bubblewrap(bwrap_bin) => {
                    let root_str = workspace_root.to_string_lossy().to_string();
                    let cwd_str = cwd.to_string_lossy().to_string();
                    let args = vec![
                        "--ro-bind".to_string(),
                        "/".to_string(),
                        "/".to_string(),
                        "--dev".to_string(),
                        "/dev".to_string(),
                        "--proc".to_string(),
                        "/proc".to_string(),
                        "--tmpfs".to_string(),
                        "/tmp".to_string(),
                        "--bind".to_string(),
                        root_str.clone(),
                        root_str,
                        "--chdir".to_string(),
                        cwd_str,
                        "--unshare-all".to_string(),
                        "--share-net".to_string(),
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ];
                    Ok((bwrap_bin.to_string_lossy().to_string(), args))
                }
                OsSandboxRuntime::Seatbelt(seatbelt_bin) => {
                    let root_str = workspace_root.to_string_lossy();
                    let profile = format!(
                        "(version 1) (allow default) (deny file-write*) (allow file-write* (subpath \"{}\"))",
                        root_str
                    );
                    let args = vec![
                        "-p".to_string(),
                        profile,
                        "sh".to_string(),
                        "-c".to_string(),
                        command.to_string(),
                    ];
                    Ok((seatbelt_bin.to_string_lossy().to_string(), args))
                }
                OsSandboxRuntime::Confinement => {
                    // Fallback to standard shell
                    Ok((
                        "sh".to_string(),
                        vec!["-c".to_string(), command.to_string()],
                    ))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_sandbox_modes_and_command_wrapping() {
        let ws = PathBuf::from("/tmp/purrcode_ws");
        let cwd = PathBuf::from("/tmp/purrcode_ws/src");

        // Danger full access
        let adapter = OsSandboxAdapter::new(OsSandboxMode::DangerFullAccess);
        let (prog, args) = adapter.wrap_command("ls -la", &ws, &cwd).unwrap();
        assert!(prog == "sh" || prog == "powershell");
        assert!(args.contains(&"ls -la".to_string()));

        // WorkspaceWrite with mock bwrap
        let bwrap_adapter = OsSandboxAdapter::with_runtime(
            OsSandboxMode::WorkspaceWrite,
            OsSandboxRuntime::Bubblewrap(PathBuf::from("/usr/bin/bwrap")),
        );
        let (prog, args) = bwrap_adapter.wrap_command("cargo test", &ws, &cwd).unwrap();
        assert_eq!(prog, "/usr/bin/bwrap");
        assert!(args.contains(&"--bind".to_string()));
        assert!(args.contains(&"/tmp/purrcode_ws".to_string()));
        assert!(args.contains(&"cargo test".to_string()));

        // ReadOnly with mock bwrap
        let ro_adapter = OsSandboxAdapter::with_runtime(
            OsSandboxMode::ReadOnly,
            OsSandboxRuntime::Bubblewrap(PathBuf::from("/usr/bin/bwrap")),
        );
        let (prog, args) = ro_adapter.wrap_command("find .", &ws, &cwd).unwrap();
        assert_eq!(prog, "/usr/bin/bwrap");
        assert!(!args.contains(&"--bind".to_string()));
        assert!(args.contains(&"--ro-bind".to_string()));
        assert!(args.contains(&"find .".to_string()));
    }
}
