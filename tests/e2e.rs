mod common;

#[test]
fn e2e_table_driven_fixtures() {
    // Each scenario: (fixture path, expected_date_substrings)
    let scenarios = vec![
        ("tests/sample.data", vec!["2026-01-05"]),
        ("tests/sample2.data", vec!["2026-01-05", "2026-02-05"]),
        ("tests/fixtures/small.data", vec!["2026-01-05"]),
        ("tests/fixtures/corrupt.data", vec!["2026-01-05"]),
        ("tests/fixtures/literal.data", vec!["2026-01-05"]),
    ];

    for (fixture, expected_dates) in scenarios {
        let (_tmp, entries) = common::replay_fixture_and_collect(fixture);
        // There should be at least one file created
        assert!(!entries.is_empty(), "no files created for fixture {}", fixture);
        // Check that each expected date substring appears in at least one filename
        for date in expected_dates {
            assert!(entries.iter().any(|name| name.contains(date)),
                "expected date {} not found in {}: {:?}", date, fixture, entries);
        }
    }
}

#[tokio::test]
async fn concurrent_recording_and_processing() {
    use adcp::{backup, config::{AppConfig, ServiceMode}, processing};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::{fs, time::{sleep, Duration}};
    use tokio::sync::watch;

    let tmp = tempdir().expect("temp dir");
    let backup_dir = tmp.path().join("backup");
    let data_process_dir = tmp.path().join("to_process");
    let processed_dir = tmp.path().join("processed");
    let data_output_dir = tmp.path().join("data");

    fs::create_dir_all(&backup_dir).await.expect("create backup");
    fs::create_dir_all(&data_process_dir).await.expect("create to_process");
    fs::create_dir_all(&processed_dir).await.expect("create processed");
    fs::create_dir_all(&data_output_dir).await.expect("create data");

    let config = Arc::new(AppConfig {
        service_name: "test-processor".to_string(),
        log_level: "info".to_string(),
        data_directory: data_output_dir.to_string_lossy().to_string(),
        serial_port: Some("/dev/null".to_string()),
        baud_rate: 115200,
        idle_threshold_seconds: 30,
        alert_webhook: None,
        mode: ServiceMode::Processing,
        backup_folder: backup_dir.to_string_lossy().to_string(),
        data_process_folder: data_process_dir.to_string_lossy().to_string(),
        processed_folder: processed_dir.to_string_lossy().to_string(),
        split_mode: adcp::config::SplitMode::Daily,
        max_backup_files: None,
        max_backup_age_days: None,
        file_stability_seconds: 1, // Short for test
        sample_file: None,
    });

    let (shutdown_tx, shutdown_rx) = watch::channel(());

    // Spawn processing loop
    let processing_config = config.clone();
    let processing_handle = tokio::spawn(async move {
        processing::run_processing_loop(processing_config, shutdown_rx).await
    });

    // Spawn recording simulator
    let recording_handle = tokio::spawn(async move {
        let mut backup = backup::Backup::new_per_append(&data_process_dir).await.expect("create backup");
        let sample_data = vec![
            "$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*41\r\n",
            "$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77\r\n",
            "$PNORC,010526,220800,1,-32.77,-32.77,-32.77,-32.77,46.34,225.0,C,65,64,61,59,40,37,14,22*35\r\n",
        ];
        let ts = chrono::Utc::now();
        for line in sample_data {
            backup.append(line, ts).await.expect("append line");
            sleep(Duration::from_millis(100)).await; // Simulate some time
        }
    });

    // Wait for recording to finish
    recording_handle.await.expect("recording failed");

    // Wait for stability
    sleep(Duration::from_secs(2)).await;

    // Shutdown processing
    shutdown_tx.send(()).ok();

    // Wait for processing to finish
    let _ = processing_handle.await.expect("processing failed");

    // Check that files were processed
    let mut processed_entries = fs::read_dir(&processed_dir).await.expect("read processed");
    let mut processed_files = vec![];
    while let Ok(Some(entry)) = processed_entries.next_entry().await {
        processed_files.push(entry.file_name().to_string_lossy().to_string());
    }
    assert!(!processed_files.is_empty(), "no files processed");

    // Check data output
    let mut data_entries = fs::read_dir(&data_output_dir).await.expect("read data");
    let mut data_files = vec![];
    while let Ok(Some(entry)) = data_entries.next_entry().await {
        data_files.push(entry.file_name().to_string_lossy().to_string());
    }
    assert!(!data_files.is_empty(), "no data files created");
}
