use adcp::{simulator, AppConfig};

#[tokio::test]
async fn replay_literal_fixture_direct() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let cfg = AppConfig {
        service_name: "dbg-replay".into(),
        log_level: "info".into(),
        data_directory: tmp.path().to_string_lossy().to_string(),
        serial_port: Some("/dev/null".into()),
        baud_rate: 115200,
        idle_threshold_seconds: 30,
        alert_webhook: None,
        mode: adcp::config::ServiceMode::Recording,
        backup_folder: "./backup".into(),
        data_process_folder: "./to_process".into(),
        processed_folder: "./processed".into(),
        split_mode: adcp::config::SplitMode::Daily,
        max_backup_files: None,
        max_backup_age_days: None,
        file_stability_seconds: 5,
        sample_file: None,
    };

    let res = simulator::replay_sample("tests/fixtures/literal.data", &cfg).await;
    assert!(res.is_ok(), "replay failed: {:?}", res.err());
    let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().filter_map(|r| r.ok().map(|e| e.file_name())).collect();
    assert!(!entries.is_empty(), "no files created after replay");
}
