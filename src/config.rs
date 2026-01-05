use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub service_name: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_data_dir")]
    pub data_directory: String,
    pub serial_port: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_data_dir() -> String {
    "./data".to_string()
}

fn default_baud_rate() -> u32 {
    115200
}

impl AppConfig {
    pub fn default_path() -> &'static str {
        "config/adcp.toml"
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let raw = fs::read_to_string(path_ref)
            .with_context(|| format!("failed to read configuration from {}", path_ref.display()))?;
        let mut config: Self = toml::from_str(&raw).with_context(|| {
            format!("failed to parse configuration from {}", path_ref.display())
        })?;
        if config.service_name.trim().is_empty() {
            config.service_name = "adcp-supervisor".to_string();
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn load_parses_config() {
        let mut file = NamedTempFile::new().expect("create temp config");
        writeln!(
            file,
            "service_name = \"test-dummy\"
serial_port = \"/dev/null\""
        )
        .unwrap();
        let config = AppConfig::load(file.path()).expect("load config");
        assert_eq!(config.service_name, "test-dummy");
        assert_eq!(config.serial_port, "/dev/null");
        assert_eq!(config.log_level, "info");
        assert_eq!(config.data_directory, "./data");
        assert_eq!(config.baud_rate, 115200);
    }
}
