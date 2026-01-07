use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::{
    fs,
    io::AsyncWriteExt,
    process,
    signal,
    sync::watch,
    time::{sleep, Duration},
};

use crate::config::{AppConfig, ServiceMode};
use crate::{backup, metrics, parser, persistence, serial, processing};
use chrono::Utc;
use std::time::Duration as StdDuration;
use tokio::time::interval;
// StdArc not needed; use `Arc` imported above where required

pub struct Service {
    config: AppConfig,
}

impl Service {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        match self.config.mode {
            ServiceMode::Recording => self.run_recording().await,
            ServiceMode::Processing => self.run_processing().await,
            ServiceMode::Orchestrator => self.run_orchestrator().await,
            ServiceMode::Simulator => self.run_simulator().await,
        }
    }

    async fn run_recording(&self) -> Result<()> {
        let AppConfig {
            service_name,
            data_directory,
            serial_port: serial_port_opt,
            baud_rate,
            idle_threshold_seconds,
            alert_webhook,
            backup_folder,
            data_process_folder,
            file_stability_seconds,
            ..
        } = &self.config;

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let supervisor_name = Arc::new(service_name.clone());
        let data_directory = Arc::new(data_directory.clone());
        let serial_port = Arc::new(serial_port_opt.clone().ok_or_else(|| anyhow::anyhow!("serial_port required for Recording mode"))?);
        let backup_folder = Arc::new(backup_folder.clone());
        let data_process_folder = Arc::new(data_process_folder.clone());
        let metrics = Arc::new(metrics::Metrics::new());
        let persistence = Arc::new(
            persistence::Persistence::new(data_directory.as_ref())
                .await
                .context("prepare persistence backend")?,
        );
        let backup = Arc::new(tokio::sync::Mutex::new(
            backup::Backup::new(backup_folder.as_ref())
                .await
                .context("prepare backup backend")?,
        ));
        let data_process = Arc::new(tokio::sync::Mutex::new(
            backup::Backup::new_per_append(data_process_folder.as_ref())
                .await
                .context("prepare data process backend")?,
        ));

        let health_handle = tokio::spawn(metrics::monitor_health(
            supervisor_name.clone(),
            metrics.clone(),
            shutdown_rx.clone(),
            Duration::from_secs(*idle_threshold_seconds),
            alert_webhook.clone(),
        ));

        // Prepare tmp folder under deployment for IPC and heartbeats
        let tmp_dir = "./deployment/tmp".to_string();
        fs::create_dir_all(&tmp_dir).await.ok();
        // Heartbeat file for supervisor to monitor liveness
        let hb_path = format!("{}/adcp_{}_hb", tmp_dir, service_name.replace(' ', "_"));
        let mut hb_shutdown = shutdown_rx.clone();
        let hb_name = hb_path.clone();
        let hb_interval = StdDuration::from_secs(std::cmp::min(5, *file_stability_seconds).max(1));
        let hb_handle = tokio::spawn(async move {
            let mut ticker = interval(hb_interval);
            loop {
                tokio::select! {
                    _ = hb_shutdown.changed() => break,
                    _ = ticker.tick() => {
                        let _ = tokio::fs::write(&hb_name, format!("{}", chrono::Utc::now().timestamp())).await;
                    }
                }
            }
        });

        let worker_future = {
            let supervisor_name = supervisor_name.clone();
            let data_directory = data_directory.clone();
            let serial_port = serial_port.clone();
            let persistence = persistence.clone();
            let metrics = metrics.clone();
            let backup = backup.clone();
            let data_process = data_process.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            async move {
                tracing::info!(
                    service = %supervisor_name,
                    data_dir = %data_directory,
                    port = %serial_port,
                    "serial capture starting"
                );
                let mut reader = serial::SerialPort::connect(&serial_port, *baud_rate).await?;
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            tracing::info!(service = %supervisor_name, "shutdown requested");
                            break;
                        }
                        line = reader.next_line() => {
                            match line {
                                Ok(Some(raw)) => {
                                                        // Always write raw capture to backup and processing folders. Do not allow
                                    // backup failures to stop capture; log and continue. The data_process
                                    // append updates a writer marker file to signal active writing so the
                                    // processor will avoid files that are still being appended to.
                                    let ts = Utc::now();
                                    if let Err(err) = backup.lock().await.append(&raw, ts).await {
                                        tracing::error!(service = %supervisor_name, error = %err, "backup write failed");
                                    }
                                    if let Err(err) = data_process.lock().await.append(&raw, ts).await {
                                        tracing::error!(service = %supervisor_name, error = %err, "data process write failed");
                                    }

                                    match parser::Frame::from_line(&raw) {
                                        Ok(frame) => {
                                            metrics.record_frame();
                                            if let Err(err) = persistence.append(&frame).await {
                                                metrics.record_persistence_error();
                                                tracing::error!(
                                                    service = %supervisor_name,
                                                    error = %err,
                                                    "persister failed"
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            metrics.record_parse_error();
                                            tracing::warn!(
                                                service = %supervisor_name,
                                                error = %err,
                                                frame = %raw,
                                                "frame rejected"
                                            );
                                        }
                                    }
                                }
                                Ok(None) => {
                                    tracing::warn!(service = %supervisor_name, "serial port closed");
                                    sleep(Duration::from_secs(1)).await;
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        service = %supervisor_name,
                                        error = %err,
                                        "serial read failed"
                                    );
                                    sleep(Duration::from_secs(1)).await;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
        };

        let shutdown_signal = {
            let supervisor_name = supervisor_name.clone();
            let shutdown_tx = shutdown_tx.clone();
            async move {
                signal::ctrl_c().await.ok();
                tracing::info!(service = %supervisor_name, "ctrl-c received, requesting shutdown");
                shutdown_tx.send(()).ok();
            }
        };

        let worker_result = tokio::select! {
            res = worker_future => res,
            _ = shutdown_signal => Ok(()),
        };

        shutdown_tx.send(()).ok();
        health_handle.await??;
        hb_handle.await.ok();

        // Cleanup any leftover writer marker files in the data process folder
        // This ensures `.writing` markers do not persist after the recorder shuts down.
        if let Err(e) = async {
            let dp = &*data_process_folder;
            let mut rd = tokio::fs::read_dir(dp).await?;
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".writing") {
                        let _ = tokio::fs::remove_file(&path).await;
                        tracing::info!(marker = %name, folder = %dp, "removed leftover writing marker");
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            tracing::warn!(error = %e, "failed to cleanup leftover writing markers");
        }

        worker_result
    }

    async fn run_processing(&self) -> Result<()> {
        let AppConfig { service_name, .. } = &self.config;

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let supervisor_name = Arc::new(service_name.clone());

        let health_handle = tokio::spawn(metrics::monitor_health(
            supervisor_name.clone(),
            Arc::new(metrics::Metrics::new()),
            shutdown_rx.clone(),
            Duration::from_secs(60),
            None,
        ));

        // Heartbeat file for supervisor to monitor liveness
        let tmp_dir = "./deployment/tmp".to_string();
        fs::create_dir_all(&tmp_dir).await.ok();
        let hb_path = format!("{}/adcp_{}_hb", tmp_dir, service_name.replace(' ', "_"));
        let mut hb_shutdown = shutdown_rx.clone();
        let hb_name = hb_path.clone();
        let hb_interval = StdDuration::from_secs(std::cmp::min(5, self.config.file_stability_seconds).max(1));
        let hb_handle = tokio::spawn(async move {
            let mut ticker = interval(hb_interval);
            loop {
                tokio::select! {
                    _ = hb_shutdown.changed() => break,
                    _ = ticker.tick() => {
                        let _ = tokio::fs::write(&hb_name, format!("{}", chrono::Utc::now().timestamp())).await;
                    }
                }
            }
        });

        let cfg = Arc::new(self.config.clone());
        let processing_handle = tokio::spawn({
            let cfg = cfg.clone();
            async move { processing::run_processing_loop(cfg, shutdown_rx).await }
        });

        // Wait for ctrl-c
        signal::ctrl_c().await.ok();
        tracing::info!(service = %supervisor_name, "ctrl-c received, requesting shutdown");
        shutdown_tx.send(()).ok();

        // Wait for tasks
        let res = processing_handle.await?;
        health_handle.await??;
        hb_handle.await.ok();
        res
    }

    async fn run_orchestrator(&self) -> Result<()> {
        let tmp_dir = "./deployment/tmp".to_string();
        fs::create_dir_all(&tmp_dir).await.ok();
        let fifo_path = format!("{}/adcp_fifo", tmp_dir);
        // Create FIFO (Unix only)
        #[cfg(unix)]
        {
            std::process::Command::new("mkfifo")
                .arg(&fifo_path)
                .status()
                .context("failed to create FIFO")?;
        }
        #[cfg(windows)]
        {
            // On Windows, create a regular file to stand in for the FIFO.
            // This allows the recorder to "connect" to it before the simulator starts writing.
            let _ = tokio::fs::remove_file(&fifo_path).await;
            let _ = tokio::fs::File::create(&fifo_path).await;
        }
        
        // Spawn simulator
        let simulator_config = format!("service_name = \"adcp-simulator\"\nmode = \"Simulator\"\nserial_port = \"{}\"\nsample_file = \"tests/sample.data\"\n", fifo_path);
        let simulator_cfg_path = format!("{}/simulator.toml", tmp_dir);
        fs::write(&simulator_cfg_path, simulator_config).await?;
        let simulator_proc = process::Command::new("./target/release/adcp")
            .arg(&simulator_cfg_path)
            .spawn()
            .context("failed to spawn simulator")?;
        
        // Spawn recorder (use configured folders so deployment layout is respected)
        let recorder_config = format!(
            "service_name = \"adcp-recorder\"\nmode = \"Recording\"\nserial_port = \"{}\"\ndata_process_folder = \"{}\"\nbackup_folder = \"{}\"\n",
            fifo_path,
            &self.config.data_process_folder,
            &self.config.backup_folder,
        );
        let recorder_cfg_path = format!("{}/recorder.toml", tmp_dir);
        fs::write(&recorder_cfg_path, recorder_config).await?;
        // Ensure the recorder/processor folders exist before spawning child processes
        fs::create_dir_all(&self.config.data_process_folder).await.ok();
        fs::create_dir_all(&self.config.backup_folder).await.ok();
        fs::create_dir_all(&self.config.processed_folder).await.ok();
        fs::create_dir_all(&self.config.data_directory).await.ok();

        let recorder_proc = process::Command::new("./target/release/adcp")
            .arg(&recorder_cfg_path)
            .spawn()
            .context("failed to spawn recorder")?;
        
        // Spawn processor (use configured folders)
        let processor_config = format!(
            "service_name = \"adcp-processor\"\nmode = \"Processing\"\ndata_process_folder = \"{}\"\nprocessed_folder = \"{}\"\ndata_directory = \"{}\"\nfile_stability_seconds = {}\n",
            &self.config.data_process_folder,
            &self.config.processed_folder,
            &self.config.data_directory,
            &self.config.file_stability_seconds,
        );
        let processor_cfg_path = format!("{}/processor.toml", tmp_dir);
        fs::write(&processor_cfg_path, processor_config).await?;
        let processor_proc = process::Command::new("./target/release/adcp")
            .arg(&processor_cfg_path)
            .spawn()
            .context("failed to spawn processor")?;
        
        use tokio::sync::Mutex as TokioMutex;
        use std::sync::Arc as StdArc;

        // Wrap children in Arc<Mutex<>> so the watchdog can restart them
        let sim_cmd = ("./target/release/adcp".to_string(), simulator_cfg_path.to_string());
        let rec_cmd = ("./target/release/adcp".to_string(), recorder_cfg_path.to_string());
        let proc_cmd = ("./target/release/adcp".to_string(), processor_cfg_path.to_string());

        let sim_child = StdArc::new(TokioMutex::new(Some(simulator_proc)));
        let rec_child = StdArc::new(TokioMutex::new(Some(recorder_proc)));
        let proc_child = StdArc::new(TokioMutex::new(Some(processor_proc)));

        // Heartbeat file paths (child services write these)
        let sim_hb = format!("{}/adcp_{}_hb", tmp_dir, "adcp-simulator".replace(' ', "_"));
        let rec_hb = format!("{}/adcp_{}_hb", tmp_dir, "adcp-recorder".replace(' ', "_"));
        let proc_hb = format!("{}/adcp_{}_hb", tmp_dir, "adcp-processor".replace(' ', "_"));

        let sim_child_mon = sim_child.clone();
        let rec_child_mon = rec_child.clone();
        let proc_child_mon = proc_child.clone();

        // Compute a safer threshold for considering a child heartbeat stale.
        // Use 3x the configured `file_stability_seconds`, but at least 10s.
        let threshold_secs = std::cmp::max(10u64, self.config.file_stability_seconds.saturating_mul(3));
        let watchdog = tokio::spawn(async move {
            let mut ticker = interval(StdDuration::from_secs(2));
            loop {
                ticker.tick().await;
                let threshold = StdDuration::from_secs(threshold_secs);
                let now = std::time::SystemTime::now();

                let check_and_restart = |hb: &str, cmd: &(String,String), child_arc: StdArc<TokioMutex<Option<process::Child>>>| {
                    let hb = hb.to_string();
                    let cmd = cmd.clone();
                    let child_arc = child_arc.clone();
                    async move {
                        let stale = match tokio::fs::metadata(&hb).await {
                            Ok(meta) => match meta.modified() {
                                Ok(m) => now.duration_since(m).unwrap_or_default() > threshold,
                                Err(_) => true,
                            },
                            Err(_) => true,
                        };
                        if stale {
                            tracing::warn!(heartbeat = %hb, "heartbeat stale — restarting job");
                            // kill existing
                            if let Some(mut c) = child_arc.lock().await.take() {
                                let _ = c.kill().await;
                                let _ = c.wait().await;
                            }
                            // respawn
                            match process::Command::new(&cmd.0).arg(&cmd.1).spawn() {
                                Ok(newc) => {
                                    *child_arc.lock().await = Some(newc);
                                    tracing::info!(cmd = %cmd.1, "restarted job");
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "failed to restart job")
                                }
                            }
                        }
                    }
                };

                // Run checks concurrently
                let _ = tokio::join!(
                    check_and_restart(&sim_hb, &sim_cmd, sim_child_mon.clone()),
                    check_and_restart(&rec_hb, &rec_cmd, rec_child_mon.clone()),
                    check_and_restart(&proc_hb, &proc_cmd, proc_child_mon.clone()),
                );
            }
        });

        // Wait for ctrl-c
        signal::ctrl_c().await.ok();
        tracing::info!("orchestrator shutting down");

        // Stop the watchdog first so it does not restart children while we shut them down
        watchdog.abort();
        let _ = watchdog.await;

        // kill children and wait for them to exit
        if let Some(mut c) = sim_child.lock().await.take() {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
        if let Some(mut c) = rec_child.lock().await.take() {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
        if let Some(mut c) = proc_child.lock().await.take() {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }

        // Cleanup any leftover child pid files created by children (best-effort)
        if let Ok(rd) = std::fs::read_dir("./deployment/tmp") {
            for entry in rd.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("adcp-") && name.ends_with(".pid") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        // Cleanup any leftover writer marker files in the data process folder
        let dp_ref = self.config.data_process_folder.clone();
        if let Err(e) = async move {
            let mut rd = tokio::fs::read_dir(&dp_ref).await?;
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".writing") {
                        let _ = tokio::fs::remove_file(&path).await;
                        tracing::info!(marker = %name, folder = %dp_ref, "removed leftover writing marker (orchestrator shutdown)");
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            tracing::warn!(error = %e, "failed to cleanup leftover writing markers (orchestrator shutdown)");
        }

        // Also move any remaining .raw files into the processed folder so they are not lost.
        let dp = self.config.data_process_folder.clone();
        let proc = self.config.processed_folder.clone();
        if let Err(e) = async move {
            let mut rd = tokio::fs::read_dir(&dp).await?;
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".raw") {
                        let dest = std::path::Path::new(&proc).join(name);
                        match tokio::fs::rename(&path, &dest).await {
                            Ok(_) => tracing::info!(file = %name, "moved raw file to processed (orchestrator shutdown)"),
                            Err(_) => {
                                // fallback to copy + remove
                                tokio::fs::copy(&path, &dest).await?;
                                tokio::fs::remove_file(&path).await?;
                                tracing::info!(file = %name, "copied raw file to processed (orchestrator shutdown)");
                            }
                        }
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            tracing::warn!(error = %e, "failed to move remaining .raw files to processed (orchestrator shutdown)");
        }
        
        Ok(())
    }

    async fn run_simulator(&self) -> Result<()> {
        let sample_file = self.config.sample_file.as_ref().ok_or_else(|| anyhow::anyhow!("sample_file required for simulator mode"))?;
        let fifo_path = self.config.serial_port.as_ref().ok_or_else(|| anyhow::anyhow!("serial_port required for simulator mode"))?; // Use serial_port as the output FIFO
        // Ensure tmp dir exists and start heartbeat for simulator
        let tmp_dir = "./deployment/tmp".to_string();
        fs::create_dir_all(&tmp_dir).await.ok();
        let hb_name = format!("{}/adcp_{}_hb", tmp_dir, self.config.service_name.replace(' ', "_"));
        use tokio::time::interval as tokio_interval;
        let hb_interval = StdDuration::from_secs(std::cmp::min(5, self.config.file_stability_seconds).max(1));
        let hb_handle = tokio::spawn({
            let hb_name = hb_name.clone();
            async move {
                let mut ticker = tokio_interval(hb_interval);
                loop {
                    ticker.tick().await;
                    let _ = tokio::fs::write(&hb_name, format!("{}", chrono::Utc::now().timestamp())).await;
                }
            }
        });

        let sample_data = fs::read_to_string(sample_file).await?;
        let lines: Vec<&str> = sample_data.lines().collect();

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true) // Ensure file is created if not already there (especially for Windows "FIFO" simulation)
            .open(fifo_path)
            .await
            .with_context(|| format!("failed to open FIFO {}", fifo_path))?;

        for line in &lines {
            if line.trim().is_empty() { continue; }
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
            file.flush().await?;
            sleep(Duration::from_millis(100)).await; // Simulate real-time data
        }
        // Stop heartbeat and return
        hb_handle.abort();
        hb_handle.await.ok();
        Ok(())
    }
}
