// SPDX-License-Identifier: GPL-3.0-or-later

pub mod cron;
pub mod curator;
pub mod heartbeat;

use purrcode_approval::{AuditEntry, AuditLogger};
use std::path::PathBuf;

pub struct ServiceInstaller;

impl ServiceInstaller {
    pub fn generate_systemd_unit() -> String {
        r#"[Unit]
Description=PurrCode AI Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/purrcode-ai gateway
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
"#
        .to_string()
    }

    pub fn generate_openrc_init() -> String {
        r#"#!/sbin/openrc-run
description="PurrCode AI Daemon"

command="/usr/local/bin/purrcode-ai"
command_args="gateway"
pidfile="/run/purrcode-ai.pid"
command_background="yes"

depend() {
    need net
}
"#
        .to_string()
    }

    pub fn generate_windows_task_script(target_exe: Option<&str>) -> String {
        let exe_path = target_exe.unwrap_or(r"C:\Program Files\PurrCode\purrcode-ai.exe");
        format!(
            r#"# PurrCode AI - Windows Scheduled Task Setup Script
# Run this script in PowerShell to install the PurrCode AI gateway daemon:

$TaskName = "PurrCodeAI_Gateway"
$ExePath = "{exe_path}"

if (-not (Test-Path $ExePath)) {{
    $Resolved = (Get-Command purrcode-ai.exe -ErrorAction SilentlyContinue).Source
    if ($Resolved) {{
        $ExePath = $Resolved
    }}
}}

$Action = New-ScheduledTaskAction -Execute $ExePath -Argument "gateway"
$Trigger = New-ScheduledTaskTrigger -AtLogOn
$Principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings -Description "PurrCode AI Gateway Background Daemon" -Force
Write-Host "PurrCode AI Gateway task registered successfully. To run now: Start-ScheduledTask -TaskName '$TaskName'"
"#
        )
    }

    pub fn status_report() -> String {
        #[cfg(windows)]
        {
            "Windows host: run `purrcode-ai service install` to generate PowerShell Scheduled Task installer script".to_string()
        }
        #[cfg(not(windows))]
        {
            if std::path::Path::new("/etc/systemd/system/purrcode-ai.service").exists() {
                "systemd unit installed at /etc/systemd/system/purrcode-ai.service".to_string()
            } else if std::path::Path::new("/etc/init.d/purrcode-ai").exists() {
                "OpenRC init script installed at /etc/init.d/purrcode-ai".to_string()
            } else {
                "not installed (run `purrcode-ai service install` to inspect unit)".to_string()
            }
        }
    }
}

/// The user's PurrCode home directory (`~/.purrcode`, falling back to
/// a relative `.purrcode` when no home directory can be determined).
/// Moved here from the CLI (`onboard::get_purrcode_dir`) in the R2
/// service extraction (merge basis dev-muse) so the background
/// runners can find it without depending on the CLI crate.
pub fn purrcode_dir() -> PathBuf {
    let home = {
        #[cfg(windows)]
        {
            std::env::var_os("USERPROFILE").map(PathBuf::from)
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME").map(PathBuf::from)
        }
    };
    match home {
        Some(h) => h.join(".purrcode"),
        None => PathBuf::from(".purrcode"),
    }
}

/// Audit-log a warning for every plugin whose declared capabilities
/// are not enforced yet (v0.1: process plugins run with full user
/// rights; enforcement comes with the v0.3 OS sandbox). Moved here
/// from the CLI in the R2 service extraction; the runners and the
/// CLI call it alike.
pub fn log_unenforced_plugin_capabilities(
    logger: &AuditLogger,
    session_id: &str,
    agent: &str,
    unenforced: &[(String, Vec<&'static str>)],
) {
    for (name, kinds) in unenforced {
        let _ = logger.log_entry(AuditEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            agent: agent.to_string(),
            event: "plugin_capability_unenforced".to_string(),
            tool: None,
            decision: "warn".to_string(),
            detail: format!(
                "plugin '{name}' declares {} capabilities, which are NOT enforced in v0.1: process plugins run with full user rights; enforcement comes with the v0.3 OS sandbox",
                kinds.join(" + ")
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_unit_generators() {
        let systemd = ServiceInstaller::generate_systemd_unit();
        assert!(systemd.contains("ExecStart=/usr/local/bin/purrcode-ai gateway"));
        let openrc = ServiceInstaller::generate_openrc_init();
        assert!(openrc.contains("command=\"/usr/local/bin/purrcode-ai\""));
        assert!(openrc.contains("command_args=\"gateway\""));
        assert!(!ServiceInstaller::status_report().is_empty());
    }

    #[test]
    fn test_systemd_unit_dependencies_and_restart_policy() {
        let unit = ServiceInstaller::generate_systemd_unit();
        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("After=network.target"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=5s"));
        assert!(unit.contains("[Install]"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn test_openrc_init_shebang_and_network_dependency() {
        let init = ServiceInstaller::generate_openrc_init();
        assert!(init.starts_with("#!/sbin/openrc-run"));
        assert!(init.contains("pidfile=\"/run/purrcode-ai.pid\""));
        assert!(init.contains("need net"));
    }

    #[test]
    fn test_windows_task_script_generator() {
        let script =
            ServiceInstaller::generate_windows_task_script(Some(r"C:\bin\purrcode-ai.exe"));
        assert!(script.contains("PurrCodeAI_Gateway"));
        assert!(script.contains(r"C:\bin\purrcode-ai.exe"));
        assert!(script.contains("Register-ScheduledTask"));
        assert!(script.contains("New-ScheduledTaskAction"));
        assert!(script.contains("gateway"));
    }
}
