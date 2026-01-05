use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::{
    signal,
    sync::watch,
    time::{sleep, Duration},
};

use crate::config::AppConfig;
use crate::{metrics, parser, persistence, serial};

pub struct Service {
    config: AppConfig,
}

impl Service {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let AppConfig {
            service_name,
            data_directory,
            serial_port,
            baud_rate,
            idle_threshold_seconds,
            alert_webhook,
            ..
        } = self.config;

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let supervisor_name = Arc::new(service_name);
        let data_directory = Arc::new(data_directory);
        let serial_port = Arc::new(serial_port);
        let metrics = Arc::new(metrics::Metrics::new());
        let persistence = Arc::new(
            persistence::Persistence::new(data_directory.as_ref())
                .await
                .context("prepare persistence backend")?,
        );

        let health_handle = tokio::spawn(metrics::monitor_health(
            supervisor_name.clone(),
            metrics.clone(),
            shutdown_rx.clone(),
            Duration::from_secs(idle_threshold_seconds),
            alert_webhook.clone(),
        ));

        let worker_future = {
            let supervisor_name = supervisor_name.clone();
            let data_directory = data_directory.clone();
            let serial_port = serial_port.clone();
            let persistence = persistence.clone();
            let metrics = metrics.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            async move {
                tracing::info!(
                    service = %supervisor_name,
                    data_dir = %data_directory,
                    port = %serial_port,
                    "serial capture starting"
                );
                let mut reader = serial::SerialPort::connect(&serial_port, baud_rate).await?;
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            tracing::info!(service = %supervisor_name, "shutdown requested");
                            break;
                        }
                        line = reader.next_line() => {
                            match line {
                                Ok(Some(raw)) => {
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

        worker_result
    }
}
