#[cfg(target_os = "linux")]
mod pipeline_linux {
    use adcp::{metrics::Metrics, parser::Frame, persistence::Persistence};
    use chrono::{TimeZone, Utc};
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn pipeline_parses_persists_and_counts_frames() {
        let tmp = tempdir().expect("temp dir");
        let persistence = Persistence::new(tmp.path()).await.expect("persistence");
        let metrics = Metrics::new();

        let lines = vec![
            "$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77",
            "$PNORC,010526,220800,1,-32.77,-32.77,-32.77,-32.77,46.34,225.0,C,65,64,61,59,40,37,14,22*35",
        ];

        for line in lines.iter() {
            let frame = Frame::from_line(line).expect("parse frame");
            persistence.append(&frame).await.expect("persist frame");
            metrics.record_frame();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frames, 2);
        assert_eq!(snapshot.parse_errors, 0);
        assert_eq!(snapshot.persistence_errors, 0);

        let current = persistence.current_path().await;
        let content = fs::read_to_string(current).expect("read log");
        let mut lines_written: Vec<&str> = content.lines().collect();
        lines_written.sort();
        let mut expected: Vec<String> = lines
            .iter()
            .map(|raw| Frame::from_line(raw).unwrap().to_persistence_line())
            .collect();
        expected.sort();
        assert_eq!(lines_written, expected);

        // Ensure the payload timestamp is preserved as UTC
        let parsed = Frame::from_line(lines[0]).unwrap();
        match parsed.payload {
            adcp::parser::Payload::Sensor(s) => {
                assert_eq!(
                    s.sent_at,
                    Utc.with_ymd_and_hms(2026, 1, 5, 22, 8, 0).unwrap()
                );
            }
            _ => panic!("expected sensor payload"),
        }
    }
}

#[cfg(target_os = "windows")]
mod pipeline_windows {
    use adcp::{metrics::Metrics, parser::Frame, persistence::Persistence};
    use tempfile::tempdir;

    #[tokio::test]
    async fn pipeline_handles_virtual_com_frames() {
        let tmp = tempdir().expect("temp dir");
        let persistence = Persistence::new(tmp.path()).await.expect("persistence");
        let metrics = Metrics::new();

        let lines = vec![
            "$PNORS,010526,220900,00000000,3ED40002,23.7,1532.0,275.9,-49.1,83.0,0.000,24.01,0,0*78",
            "$PNORC,010526,220900,1,-32.77,-32.77,-32.77,-32.77,46.34,225.0,C,65,65,60,60,42,41,13,24*3C",
        ];

        for line in lines.iter() {
            let frame = Frame::from_line(line).expect("parse frame");
            persistence.append(&frame).await.expect("persist frame");
            metrics.record_frame();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.frames, 2);
        assert_eq!(snapshot.parse_errors, 0);
        assert_eq!(snapshot.persistence_errors, 0);

        // Mock/virtual COM scenario: we only assert the pipeline accepts data without a real port.
    }
}

    mod sample_replay {
        use adcp::{config::{ServiceMode, SplitMode}, simulator, AppConfig};
        use std::fs;
        use tempfile::tempdir;

        #[tokio::test]
        async fn replays_sample_file_into_dated_log() {
            let tmp = tempdir().expect("temp dir");
            let cfg = AppConfig {
                service_name: "sample-supervisor".into(),
                log_level: "info".into(),
                data_directory: tmp.path().to_string_lossy().to_string(),
                serial_port: Some("/dev/null".into()),
                baud_rate: 115200,
                idle_threshold_seconds: 30,
                alert_webhook: None,
                mode: ServiceMode::Recording,
                backup_folder: "./backup".into(),
                data_process_folder: "./to_process".into(),
                processed_folder: "./processed".into(),
                split_mode: SplitMode::Daily,
                max_backup_files: None,
                max_backup_age_days: None,
                file_stability_seconds: 5,
                sample_file: None,
            };

            simulator::replay_sample("tests/sample.data", &cfg)
                .await
                .expect("replay sample");

            let mut entries: Vec<String> = fs::read_dir(tmp.path())
                .expect("read data dir")
                .filter_map(|res| res.ok().and_then(|e| e.file_name().into_string().ok()))
                .collect();
            entries.sort();
            assert!(entries.iter().any(|name| name.contains("2026-01-05")));

            let dated = tmp.path().join("adcp-2026-01-05.log");
            let content = fs::read_to_string(dated).expect("read dated log");
            assert!(content.lines().count() >= 2);
        }

        #[tokio::test]
        async fn replays_sample2_and_rotates_across_days() {
            let tmp = tempdir().expect("temp dir");
            let cfg = AppConfig {
                service_name: "sample2-supervisor".into(),
                log_level: "info".into(),
                data_directory: tmp.path().to_string_lossy().to_string(),
                serial_port: Some("/dev/null".into()),
                baud_rate: 115200,
                idle_threshold_seconds: 30,
                alert_webhook: None,
                mode: ServiceMode::Recording,
                backup_folder: "./backup".into(),
                data_process_folder: "./to_process".into(),
                processed_folder: "./processed".into(),
                split_mode: SplitMode::Daily,
                max_backup_files: None,
                max_backup_age_days: None,
                file_stability_seconds: 5,
                sample_file: None,
            };

            simulator::replay_sample("tests/sample2.data", &cfg)
                .await
                .expect("replay sample2");

            let mut entries: Vec<String> = fs::read_dir(tmp.path())
                .expect("read data dir")
                .filter_map(|res| res.ok().and_then(|e| e.file_name().into_string().ok()))
                .collect();
            entries.sort();
            assert!(entries.iter().any(|name| name.contains("2026-01-05")));
            assert!(entries.iter().any(|name| name.contains("2026-02-05")));

            let day1 = tmp.path().join("adcp-2026-01-05.log");
            let day2 = tmp.path().join("adcp-2026-02-05.log");
            let day1_lines = fs::read_to_string(day1).expect("read day1 log");
            let day2_lines = fs::read_to_string(day2).expect("read day2 log");
            assert!(day1_lines.lines().count() >= 3);
            assert!(day2_lines.lines().count() >= 2);
        }
    }
