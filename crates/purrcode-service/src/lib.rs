// SPDX-License-Identifier: GPL-3.0-or-later

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

    pub fn status_report() -> String {
        if std::path::Path::new("/etc/systemd/system/purrcode-ai.service").exists() {
            "systemd unit installed at /etc/systemd/system/purrcode-ai.service".to_string()
        } else if std::path::Path::new("/etc/init.d/purrcode-ai").exists() {
            "OpenRC init script installed at /etc/init.d/purrcode-ai".to_string()
        } else {
            "not installed (run `purrcode-ai service install` to inspect unit)".to_string()
        }
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
}
