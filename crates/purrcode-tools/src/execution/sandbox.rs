// SPDX-License-Identifier: GPL-3.0-or-later

//! OS sandbox for the `bash` tool (v0.3 F21, ADR-014 item 8).
//!
//! A Landlock ruleset is built in the PARENT, before the child is
//! spawned, and enforced in the `bash` child from `Command::pre_exec`
//! (after `fork`, before `exec`). Landlock restrictions are inherited
//! and irrevocable: once the child restricts itself there is no window
//! in which it runs less confined than intended, and no way back.
//!
//! # Rules (all modes)
//!
//! * `/` — read + execute. The child must be able to run `/usr/bin/sh`,
//!   its dynamic loader, and the tools it invokes.
//! * `/dev/null` — read + write. It is a sink, not a file: ordinary
//!   commands redirect there, and denying it breaks them without
//!   protecting anything.
//!
//! Writes then follow the preset's `sandbox_mode`:
//!
//! * `read-only` — nothing else is writable. Not even the workspace.
//! * `workspace-write` — the workspace root is writable (create /
//!   modify / delete / rename / truncate under it).
//! * `danger-full-access` — `/` is writable too. The Landlock
//!   mechanism still runs (ruleset + `no_new_privs`), but the ruleset
//!   grants full filesystem access.
//!
//! # Known limitation: reads are not confined
//!
//! Only writes (mutation) are confined. The child can still READ
//! anything the user can read, including `~/.ssh` and
//! `~/.purrcode/credentials` — Landlock rules are allow-lists, and
//! the child needs broad read access to run at all. The confinement
//! boundary is mutation plus the exfiltration surface: a child that
//! can read a secret but can neither write it anywhere nor reach the
//! network (network restriction is the seccomp phase 2, after v0.3)
//! can only show it to the model. Deny-read hardening for specific
//! secret paths is a follow-up, not F21.
//!
//! # Fail-closed
//!
//! Every failure refuses the run: Landlock unavailable on the kernel,
//! a ruleset that is not `FullyEnforced`, `no_new_privs` not enforced,
//! a missing rule path, or a non-Linux platform. The command is never
//! spawned unsandboxed when the preset demanded the OS sandbox.

use purrcode_api::error::{PurrError, Result};
use purrcode_api::tool::SandboxStrictness;
use std::path::Path;

/// A Landlock ruleset built in the parent process, to be enforced in
/// the `bash` child from `Command::pre_exec` (before `exec`).
pub struct OsSandbox {
    strictness: SandboxStrictness,
    #[cfg(target_os = "linux")]
    ruleset: landlock::RulesetCreated,
}

impl std::fmt::Debug for OsSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsSandbox")
            .field("strictness", &self.strictness)
            .finish_non_exhaustive()
    }
}

impl OsSandbox {
    /// Build the ruleset in the PARENT, before the child is spawned.
    /// Any failure here is fail-closed: the caller must not spawn
    /// the command at all.
    pub fn build(workspace_root: &Path, strictness: SandboxStrictness) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::build(workspace_root, strictness)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = workspace_root;
            Err(PurrError::Tool {
                tool: "bash".to_string(),
                message: format!(
                    "OS sandbox requested (sandbox_mode '{}') but Landlock is Linux-only; \
                     this platform cannot enforce it. Use a confinement preset.",
                    strictness.as_kebab_str()
                ),
            })
        }
    }

    /// Enforce the ruleset in the CHILD. Call this — and only this —
    /// from `Command::pre_exec`, before the shell is exec'd. The
    /// success path does no allocation (the child is post-fork and
    /// pre-exec, where the allocator may be in an inconsistent state);
    /// the failure paths allocate only to describe the error, at which
    /// point the child is about to die and `spawn` reports it. Any
    /// error aborts the spawn: the child never runs unsandboxed.
    pub fn restrict_self(self) -> std::io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            linux::restrict_self(self.ruleset)
        }
        #[cfg(not(target_os = "linux"))]
        {
            // Unreachable in practice: `build` always errors here.
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use landlock::{
        path_beneath_rules, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
    };

    /// The Landlock ABI this build pins. V8 (kernel 7.0) is the newest
    /// ABI the project's own test machine can verify enforcement for:
    /// it covers every right up to `IoctlDev`, including `Refer`
    /// (V2, cross-directory rename/link) and `Truncate` (V3 — without
    /// it a child could wipe a user-owned file such as the credentials
    /// file without ever opening it for write). V9 (`ResolveUnix`,
    /// kernel 7.1) is out of scope until a machine running 7.1+ can
    /// verify it. On a kernel older than 7.0 the crate drops the
    /// unsupported rights (best-effort) and `restrict_self` reports
    /// `PartiallyEnforced` — which we treat as a hard failure
    /// (fail-closed), never as a weaker sandbox.
    const SANDBOX_ABI: landlock::ABI = landlock::ABI::V8;

    fn tool_err(message: String) -> PurrError {
        PurrError::Tool {
            tool: "bash".to_string(),
            message,
        }
    }

    /// Fail-closed pre-check for a ruleset path (see `build`): the
    /// path must be reachable, or the ruleset cannot cover it.
    fn require_rule_path(path: &Path, what: &str) -> Result<()> {
        std::fs::metadata(path).map_err(|e| {
            tool_err(format!(
                "OS sandbox: {what} path {} is not accessible: {e}. The command was not run.",
                path.display()
            ))
        })?;
        Ok(())
    }

    pub(super) fn build(workspace_root: &Path, strictness: SandboxStrictness) -> Result<OsSandbox> {
        let abi = SANDBOX_ABI;
        let read = AccessFs::from_read(abi);
        let write = AccessFs::from_write(abi);
        let mode = strictness.as_kebab_str();

        // `path_beneath_rules` SILENTLY SKIPS paths it cannot open
        // (filter_map), so a missing rule path would produce a ruleset
        // that quietly lacks that rule. Pre-validate every rule path
        // instead: a sandbox that cannot cover its paths fails closed
        // here, before any child exists.
        require_rule_path(Path::new("/"), "system root")?;
        require_rule_path(Path::new("/dev/null"), "device")?;
        require_rule_path(workspace_root, "workspace root")?;
        if !workspace_root.is_dir() {
            return Err(tool_err(format!(
                "OS sandbox: workspace root {} is not a directory; refusing to build a ruleset over it",
                workspace_root.display()
            )));
        }

        let mut ruleset = Ruleset::default()
            .handle_access(read | write)
            .map_err(|e| {
                tool_err(format!(
                    "OS sandbox: failed to configure Landlock ruleset: {e}"
                ))
            })?
            .create()
            .map_err(|e| {
                tool_err(format!(
                    "OS sandbox requested (sandbox_mode '{mode}') but Landlock is unavailable \
                     on this kernel: {e}. The command was not run. Use a confinement preset, \
                     or a kernel with Landlock (5.13+)."
                ))
            })?;

        // Read + execute across `/` for every mode: the child must be
        // able to run the system it lives in (see the module docs for
        // the read-everywhere limitation).
        ruleset = ruleset
            .add_rules(path_beneath_rules(["/"], read))
            .map_err(|e| {
                tool_err(format!(
                    "OS sandbox: failed to add the system read rule: {e}"
                ))
            })?;

        match strictness {
            SandboxStrictness::ReadOnly | SandboxStrictness::WorkspaceWrite => {
                // `/dev/null` is a sink, not a file (see module docs).
                ruleset = ruleset
                    .add_rules(path_beneath_rules(
                        ["/dev/null"],
                        landlock::make_bitflags!(AccessFs::{WriteFile | ReadFile}),
                    ))
                    .map_err(|e| {
                        tool_err(format!("OS sandbox: failed to add the /dev/null rule: {e}"))
                    })?;
                if strictness == SandboxStrictness::WorkspaceWrite {
                    ruleset = ruleset
                        .add_rules(path_beneath_rules([workspace_root], write))
                        .map_err(|e| {
                            tool_err(format!(
                                "OS sandbox: failed to add the workspace write rule ({}): {e}",
                                workspace_root.display()
                            ))
                        })?;
                }
            }
            SandboxStrictness::DangerFullAccess => {
                // The mechanism still runs, but the ruleset grants full
                // filesystem access everywhere (read was added above).
                ruleset = ruleset
                    .add_rules(path_beneath_rules(["/"], write))
                    .map_err(|e| {
                        tool_err(format!(
                            "OS sandbox: failed to add the full-access rule: {e}"
                        ))
                    })?;
            }
        }

        Ok(OsSandbox {
            strictness,
            ruleset: ruleset.no_new_privs(true),
        })
    }

    pub(super) fn restrict_self(ruleset: landlock::RulesetCreated) -> std::io::Result<()> {
        let status = ruleset.restrict_self().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Landlock restrict_self failed: {e}"),
            )
        })?;
        if status.ruleset != RulesetStatus::FullyEnforced {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Landlock sandbox only {:?}; refusing to run the command under a partial sandbox",
                    status.ruleset
                ),
            ));
        }
        if !status.no_new_privs {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Landlock sandbox did not enforce no_new_privs; refusing to run the command",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    use std::os::unix::process::CommandExt;

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "purr-f21-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp workspace");
        dir
    }

    fn unique_tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "purr-f21-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }

    /// Run `sh -c <command>` under the sandbox (Linux) and return its
    /// exit status. The non-Linux build has no sandbox to test.
    #[cfg(target_os = "linux")]
    fn run_sandboxed(
        workspace: &Path,
        strictness: SandboxStrictness,
        command: &str,
    ) -> std::process::ExitStatus {
        let sandbox = OsSandbox::build(workspace, strictness).expect("build sandbox");
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", command]);
        cmd.current_dir(workspace);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut slot = Some(sandbox);
        unsafe {
            cmd.pre_exec(move || match slot.take() {
                Some(s) => s.restrict_self(),
                None => Ok(()),
            });
        }
        cmd.spawn()
            .expect("spawn sandboxed shell")
            .wait()
            .expect("wait for sandboxed shell")
    }

    #[test]
    fn test_sandbox_readonly_denies_write_outside_workspace() {
        #[cfg(target_os = "linux")]
        {
            let workspace = unique_dir("ro-deny");
            let target = unique_tmp("ro-deny-target");
            let status = run_sandboxed(
                &workspace,
                SandboxStrictness::ReadOnly,
                &format!("echo hi > {}", target.display()),
            );
            assert!(
                !status.success(),
                "read-only sandbox must deny a write outside the workspace"
            );
            assert!(
                !target.exists(),
                "denied write must not have created the file"
            );
            let _ = std::fs::remove_dir_all(&workspace);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (unique_dir, unique_tmp);
        }
    }

    #[test]
    fn test_sandbox_readonly_allows_read_inside_workspace() {
        #[cfg(target_os = "linux")]
        {
            let workspace = unique_dir("ro-read");
            let file = workspace.join("hello.txt");
            std::fs::write(&file, "hello sandbox\n").expect("write fixture");
            let sandbox =
                OsSandbox::build(&workspace, SandboxStrictness::ReadOnly).expect("build sandbox");
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", "cat hello.txt"]);
            cmd.current_dir(&workspace);
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            let mut slot = Some(sandbox);
            unsafe {
                cmd.pre_exec(move || match slot.take() {
                    Some(s) => s.restrict_self(),
                    None => Ok(()),
                });
            }
            let output = cmd
                .spawn()
                .expect("spawn")
                .wait_with_output()
                .expect("wait");
            assert!(
                output.status.success(),
                "read inside the workspace must succeed"
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), "hello sandbox\n");
            let _ = std::fs::remove_dir_all(&workspace);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (unique_dir, unique_tmp);
        }
    }

    #[test]
    fn test_sandbox_workspace_write_allows_inside_denies_outside() {
        #[cfg(target_os = "linux")]
        {
            let workspace = unique_dir("ww");
            let inside = workspace.join("out.txt");
            let status = run_sandboxed(
                &workspace,
                SandboxStrictness::WorkspaceWrite,
                "echo inside > out.txt",
            );
            assert!(
                status.success(),
                "workspace-write must allow a write inside the workspace"
            );
            assert_eq!(
                std::fs::read_to_string(&inside).expect("read written file"),
                "inside\n"
            );

            // …and still deny the same write outside it.
            let target = unique_tmp("ww-deny-target");
            let status = run_sandboxed(
                &workspace,
                SandboxStrictness::WorkspaceWrite,
                &format!("echo hi > {}", target.display()),
            );
            assert!(
                !status.success(),
                "workspace-write must deny a write outside the workspace"
            );
            assert!(
                !target.exists(),
                "denied write must not have created the file"
            );
            let _ = std::fs::remove_dir_all(&workspace);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (unique_dir, unique_tmp);
        }
    }

    #[test]
    fn test_sandbox_danger_full_access_allows_write_outside() {
        #[cfg(target_os = "linux")]
        {
            let workspace = unique_dir("dfa");
            let target = unique_tmp("dfa-target");
            let status = run_sandboxed(
                &workspace,
                SandboxStrictness::DangerFullAccess,
                &format!("echo hi > {}", target.display()),
            );
            assert!(
                status.success(),
                "danger-full-access must allow a write outside the workspace"
            );
            assert_eq!(
                std::fs::read_to_string(&target).expect("read written file"),
                "hi\n"
            );
            let _ = std::fs::remove_file(&target);
            let _ = std::fs::remove_dir_all(&workspace);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (unique_dir, unique_tmp);
        }
    }

    #[test]
    fn test_sandbox_build_fails_for_missing_workspace() {
        // Fail-closed at build time: a ruleset over a path that does
        // not exist must error, never produce a sandbox that allows
        // everything by accident.
        let missing = std::env::temp_dir().join(format!(
            "purr-f21-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        assert!(!missing.exists(), "fixture path must not exist");
        let err = OsSandbox::build(&missing, SandboxStrictness::ReadOnly)
            .expect_err("build over a missing workspace must fail");
        match err {
            PurrError::Tool { tool, message } => {
                assert_eq!(tool, "bash");
                assert!(
                    message.contains("OS sandbox"),
                    "unexpected error: {message}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_sandbox_non_linux_build_errors() {
        // On a platform without Landlock, demanding the OS sandbox is
        // a configuration error, never a silent no-op.
        #[cfg(not(target_os = "linux"))]
        {
            let workspace = unique_dir("nonlinux");
            let err = OsSandbox::build(&workspace, SandboxStrictness::ReadOnly)
                .expect_err("non-Linux build must fail closed");
            match err {
                PurrError::Tool { tool, message } => {
                    assert_eq!(tool, "bash");
                    assert!(
                        message.contains("Linux-only"),
                        "unexpected error: {message}"
                    );
                }
                other => panic!("unexpected error variant: {other:?}"),
            }
            let _ = std::fs::remove_dir_all(&workspace);
        }
        #[cfg(target_os = "linux")]
        {
            // The Linux build is covered by the enforcement tests above.
        }
    }
}
