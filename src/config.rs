use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub enum ServiceMode {
    Recording,
    Processing,
}

#[derive(Debug, Clone, Deserialize)]
pub enum SplitMode {
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub service_name: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_data_dir")]
    pub data_directory: String,
    pub serial_port: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_idle_threshold_secs")]
    pub idle_threshold_seconds: u64,
    #[serde(default)]
    pub alert_webhook: Option<String>,
    #[serde(default = "default_mode")]
    pub mode: ServiceMode,
    #[serde(default = "default_backup_folder")]
    pub backup_folder: String,
    #[serde(default = "default_data_process_folder")]
    pub data_process_folder: String,
    #[serde(default = "default_processed_folder")]
    pub processed_folder: String,
    #[serde(default = "default_split_mode")]
    pub split_mode: SplitMode,
    pub max_backup_files: Option<usize>,
    pub max_backup_age_days: Option<u64>,
    #[serde(default = "default_file_stability_secs")]
    pub file_stability_seconds: u64,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_file_stability_secs() -> u64 {
    5
}

fn default_data_dir() -> String {
    "./data".to_string()
}

fn default_baud_rate() -> u32 {
    115200
}

fn default_idle_threshold_secs() -> u64 {
    30
}

fn default_mode() -> ServiceMode {
    ServiceMode::Recording
}

fn default_backup_folder() -> String {
    "./backup".to_string()
}

fn default_data_process_folder() -> String {
    "./to_process".to_string()
}

fn default_processed_folder() -> String {
    "./processed".to_string()
}

fn default_split_mode() -> SplitMode {
    SplitMode::Daily
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
        assert_eq!(config.idle_threshold_seconds, 30);
        assert!(config.alert_webhook.is_none());
        // New defaults
        assert!(matches!(config.mode, ServiceMode::Recording));
        assert_eq!(config.backup_folder, "./backup");
        assert_eq!(config.data_process_folder, "./to_process");
        assert_eq!(config.processed_folder, "./processed");
        assert!(matches!(config.split_mode, SplitMode::Daily));
        assert!(config.max_backup_files.is_none());
        assert!(config.max_backup_age_days.is_none());
        assert_eq!(config.file_stability_seconds, 5);
    }
}
