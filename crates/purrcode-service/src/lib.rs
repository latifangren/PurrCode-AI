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
}
