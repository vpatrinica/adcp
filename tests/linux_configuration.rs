#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use adcp::{platform, AppConfig};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn loads_linux_configuration_and_defaults() {
        let mut file = NamedTempFile::new().expect("create temp config");
        writeln!(
            file,
            r#"service_name = "linux-supervisor"
serial_port = "/tmp/ttyADCP"
data_directory = "./linux-data"
"#
        )
        .expect("write config");

        let config = AppConfig::load(file.path()).expect("load config");
        assert_eq!(config.service_name, "linux-supervisor");
        assert_eq!(config.serial_port.as_deref(), Some("/tmp/ttyADCP"));
        assert_eq!(config.data_directory, "./linux-data");
        assert_eq!(config.log_level, "info");
        assert_eq!(config.baud_rate, 115200);
    }

    #[test]
    fn linux_template_includes_execstart() {
        let template = platform::platform_template();
        assert!(template.contains("ExecStart=/usr/local/bin/adcp"));
    }
}
