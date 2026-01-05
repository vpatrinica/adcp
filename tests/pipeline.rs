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
            "2024-01-01T00:00:00Z,1.2,3.4",
            "2024-01-01T00:00:01Z,1.3,3.5",
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
            .map(|raw| {
                let frame = Frame::from_line(raw).unwrap();
                frame.to_persistence_line()
            })
            .collect();
        expected.sort();
        assert_eq!(lines_written, expected);

        // Ensure the timestamp is preserved as UTC
        let parsed = Frame::from_line(lines[0]).unwrap();
        assert_eq!(
            parsed.timestamp,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        );
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
            "2024-01-01T00:00:00Z,0.1,0.2",
            "2024-01-01T00:00:02Z,0.3,0.4",
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
