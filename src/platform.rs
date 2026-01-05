pub fn log_platform_guidance() {
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

    tracing::info!(
        template = TEMPLATE,
        "platform-specific service descriptor available"
    );
}
