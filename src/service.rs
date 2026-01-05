use anyhow::Result;
use std::sync::Arc;
use tokio::{
    select, signal,
    sync::watch,
    time::{Duration, Instant},
};

use crate::config::AppConfig;

pub struct Service {
    config: AppConfig,
}

impl Service {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(());
        let supervisor_name = Arc::new(self.config.service_name);
        let data_directory = Arc::new(self.config.data_directory);
        let serial_port = Arc::new(self.config.serial_port);

        let worker = {
            let supervisor_name = supervisor_name.clone();
            let data_directory = data_directory.clone();
            let serial_port = serial_port.clone();
            async move {
                tracing::info!(service = %supervisor_name, data_dir = %data_directory, port = %serial_port, "worker loop starting");
                loop {
                    let checkpoint = Instant::now();
                    select! {
                        _ = shutdown_rx.changed() => {
                            tracing::info!(service = %supervisor_name, "shutdown requested");
                            break;
                        }
                        _ = tokio::time::sleep_until(checkpoint + Duration::from_secs(10)) => {
                            tracing::debug!(service = %supervisor_name, "heartbeat");
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

        select! {
            res = worker => res,
            _ = shutdown_signal => Ok(()),
        }
    }
}
