#[cfg(target_os = "windows")]
mod windows_tests {
    use adcp::platform;
    use adcp::AppConfig;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn windows_template_mentions_windows_service() {
        let template = platform::platform_template();
        assert!(template.contains("ExecStart=adcp.exe"));
        assert!(template.contains("Restart=always"));
    }

    #[test]
    fn windows_config_accepts_com_port() {
        let mut file = NamedTempFile::new().expect("create temp config");
        writeln!(
            file,
            r#"service_name = "windows-supervisor"
serial_port = "COM9"
data_directory = "C:\\ProgramData\\adcp"
"#
        )
        .expect("write config");

        let cfg = AppConfig::load(file.path()).expect("load config");
        assert_eq!(cfg.serial_port.as_deref(), Some("COM9"));
        assert_eq!(cfg.data_directory, "C:\\ProgramData\\adcp");
    }
}
