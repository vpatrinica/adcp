pub fn log_platform_guidance() {
    tracing::info!(
        template = platform_template(),
        "platform-specific service descriptor available"
    );
}

pub fn platform_template() -> &'static str {
    #[cfg(windows)]
    const TEMPLATE: &str = r#"[Service]
ExecStart=adcp.exe --config C:\etc\adcp\adcp.toml
Restart=always
"#;

    #[cfg(not(windows))]
    const TEMPLATE: &str = r#"[Unit]
Description=ADCP acquisition service
After=network.target

[Service]
ExecStart=/usr/local/bin/adcp --config /etc/adcp/adcp.toml
Restart=on-failure

[Install]
WantedBy=multi-user.target
"#;

    TEMPLATE
}
