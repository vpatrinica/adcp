use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::{fs, sync::watch, time::sleep};

use crate::{simulator, AppConfig};

const SCAN_INTERVAL_SECS: u64 = 2;

/// Scans the data process folder and processes stable files in chronological order.
pub async fn run_processing_loop(
    config: Arc<AppConfig>,
    shutdown: watch::Receiver<()>,
) -> Result<()> {
    let data_dir = PathBuf::from(&config.data_process_folder);
    let processed_dir = PathBuf::from(&config.processed_folder);

    // Ensure processed folder exists
    fs::create_dir_all(&processed_dir)
        .await
        .with_context(|| format!("prepare processed folder {}", processed_dir.display()))?;

    // File stability timeout configurable from AppConfig
    let stable_secs = config.file_stability_seconds;

    loop {
        // Check for shutdown
        if shutdown.has_changed().unwrap_or(false) {
            tracing::info!("shutdown requested for processing loop");
            break;
        }

        let mut entries = match fs::read_dir(&data_dir).await {
            Ok(rd) => rd,
            Err(err) => {
                tracing::error!(error = %err, folder = %data_dir.display(), "failed to read processing folder");
                sleep(Duration::from_secs(SCAN_INTERVAL_SECS)).await;
                continue;
            }
        };

        let mut files: Vec<PathBuf> = Vec::new();
        while let Ok(Some(ent)) = entries.next_entry().await {
            let path = ent.path();
            if path.is_file() {
                files.push(path);
            }
        }

        // Sort by filename (date-based filenames will sort chronologically)
        files.sort_by_key(|p| p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
        );

        let mut any_work = false;
        for file in files {
            if shutdown.has_changed().unwrap_or(false) {
                tracing::info!("shutdown requested for processing loop");
                break;
            }

            // Check stability
            match is_stable(&file, stable_secs).await {
                Ok(true) => {
                    tracing::info!(file = %file.display(), "processing stable file (no recent writer marker detected)");
                    any_work = true;
                    match simulator::replay_sample(&file, &config).await {
                        Ok(_) => {
                            if let Err(err) = move_to_processed(&file, &processed_dir).await {
                                tracing::error!(file = %file.display(), error = %err, "failed to move processed file");
                            }
                        }
                        Err(err) => {
                            tracing::error!(file = %file.display(), error = %err, "processing failed");
                            // Move to processed folder with .failed suffix to mark for manual inspection
                            if let Err(move_err) = move_failed(&file, &processed_dir).await {
                                tracing::error!(file = %file.display(), error = %move_err, "failed to move failed file");
                            }
                        }
                    }
                }
                Ok(false) => {
                    tracing::debug!(file = %file.display(), "file not yet stable");
                }
                Err(err) => {
                    tracing::error!(file = %file.display(), error = %err, "failed to stat file");
                }
            }
        }

        if !any_work {
            sleep(Duration::from_secs(SCAN_INTERVAL_SECS)).await;
        }
    }

    Ok(())
}

async fn is_stable(path: &PathBuf, stable_secs: u64) -> Result<bool> {

    let meta = fs::metadata(path).await?;
    let mtime = meta.modified()?;
    let age = SystemTime::now().duration_since(mtime).unwrap_or_default();

    // If file mtime is old enough, proceed to check writer marker.
    if age < Duration::from_secs(stable_secs) {
        return Ok(false);
    }

    // Check for an active writer marker: filename.raw.writing
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("file has no name"))?;
    let marker_name = format!("{}.writing", file_name);
    let marker_path = path.parent().unwrap_or_else(|| std::path::Path::new(".")).join(marker_name);

    match fs::metadata(&marker_path).await {
        Ok(m) => {
            // marker exists; check its mtime
            let mm = m.modified()?;
            let marker_age = SystemTime::now().duration_since(mm).unwrap_or_default();
            if marker_age < Duration::from_secs(stable_secs) {
                // writer was active recently
                return Ok(false);
            }
            // Otherwise it's old enough: file is stable
            Ok(true)
        }
        Err(_) => {
            // No marker exists; file mtime was already sufficiently old, so treat as stable
            Ok(true)
        }
    }
}

async fn move_to_processed(path: &PathBuf, processed_dir: &PathBuf) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("file has no file name"))?;
    let dest = processed_dir.join(name);
    // Attempt atomic rename; fallback to copy + remove
    match fs::rename(path, &dest).await {
        Ok(_) => Ok(()),
        Err(_) => {
            fs::copy(path, &dest).await?;
            fs::remove_file(path).await?;
            Ok(())
        }
    }
}

async fn move_failed(path: &PathBuf, processed_dir: &PathBuf) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("file has no file name"))?;
    let dest = processed_dir.join(format!("{}.failed", name));
    match fs::rename(path, &dest).await {
        Ok(_) => Ok(()),
        Err(_) => {
            fs::copy(path, &dest).await?;
            fs::remove_file(path).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServiceMode, SplitMode};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn processes_and_moves_file() {
        let tmp = tempdir().expect("temp dir");
        let to_process = tmp.path().join("to_process");
        let processed = tmp.path().join("processed");
        let data_out = tmp.path().join("out");
        fs::create_dir_all(&to_process).await.expect("mk to_process");
        fs::create_dir_all(&processed).await.expect("mk processed");
        fs::create_dir_all(&data_out).await.expect("mk out");

        let sample = to_process.join("2026-01-01.raw");
        let content = "$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*41\n$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77\n";
        fs::write(&sample, content).await.expect("write sample");

        // Use a short stability timeout for test
        let stable = 1u64;

        let config = AppConfig {
            service_name: "test".to_string(),
            log_level: "info".to_string(),
            data_directory: data_out.to_string_lossy().to_string(),
            serial_port: "/dev/null".to_string(),
            baud_rate: 115200,
            idle_threshold_seconds: 30,
            alert_webhook: None,
            mode: ServiceMode::Processing,
            backup_folder: "./backup".to_string(),
            data_process_folder: to_process.to_string_lossy().to_string(),
            processed_folder: processed.to_string_lossy().to_string(),
            split_mode: SplitMode::Daily,
            max_backup_files: None,
            max_backup_age_days: None,
            file_stability_seconds: stable,
        };

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let cfg = Arc::new(config);
        let handle = tokio::spawn(async move { run_processing_loop(cfg, shutdown_rx).await.expect("processing loop") });

        // Simulate active writer by touching marker file and ensure processor waits until marker ages
        let marker = to_process.join("2026-01-01.raw.writing");
        fs::write(&marker, "1").await.expect("write marker");

        // Wait less than stable so file should not yet be moved
        tokio::time::sleep(std::time::Duration::from_secs(stable)).await;
        assert!(fs::metadata(&sample).await.is_ok(), "sample should still be present while writer marker is recent");

        // Age the marker by removing it (simulating writer inactivity)
        fs::remove_file(&marker).await.expect("remove marker");

        // Wait until file becomes stable and is processed
        tokio::time::sleep(std::time::Duration::from_secs(stable + 2)).await;

        assert!(!fs::metadata(&sample).await.is_ok(), "sample should be moved after writer marker is cleared");
        assert!(fs::metadata(processed.join("2026-01-01.raw")).await.is_ok(), "processed file present");

        // Request shutdown and wait
        shutdown_tx.send(()).ok();
        handle.await.expect("join");
    }
}
