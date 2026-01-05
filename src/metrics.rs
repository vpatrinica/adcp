use anyhow::Result;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::time::interval;

/// Aggregates telemetry counters that the health monitor can report on.
pub struct Metrics {
    frames: AtomicU64,
    parse_errors: AtomicU64,
    persistence_errors: AtomicU64,
    last_frame: Mutex<Option<Instant>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            frames: AtomicU64::new(0),
            parse_errors: AtomicU64::new(0),
            persistence_errors: AtomicU64::new(0),
            last_frame: Mutex::new(None),
        }
    }

    pub fn record_frame(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_frame.lock() {
            *guard = Some(Instant::now());
        }
    }

    pub fn record_parse_error(&self) {
        self.parse_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_persistence_error(&self) {
        self.persistence_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let last_frame_age = self.last_frame.lock().ok().and_then(|guard| {
            guard.map(|instant| Instant::now().saturating_duration_since(instant))
        });
        HealthSnapshot {
            frames: self.frames.load(Ordering::Relaxed),
            parse_errors: self.parse_errors.load(Ordering::Relaxed),
            persistence_errors: self.persistence_errors.load(Ordering::Relaxed),
            last_frame_age,
        }
    }
}

pub struct HealthSnapshot {
    pub frames: u64,
    pub parse_errors: u64,
    pub persistence_errors: u64,
    pub last_frame_age: Option<Duration>,
}

pub async fn monitor_health(
    supervisor_name: Arc<String>,
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<()>,
    idle_threshold: Duration,
    alert_webhook: Option<String>,
) -> Result<()> {
    let mut ticker = interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {
                let snapshot = metrics.snapshot();
                tracing::info!(
                    service = %supervisor_name,
                    frames = snapshot.frames,
                    parse_errors = snapshot.parse_errors,
                    persistence_errors = snapshot.persistence_errors,
                    "health heartbeat"
                );
                if let Some(age) = snapshot.last_frame_age {
                    if age > idle_threshold {
                        tracing::warn!(
                            service = %supervisor_name,
                            idle_seconds = ?age.as_secs_f64(),
                            "no frames in the last {} seconds",
                            idle_threshold.as_secs()
                        );
                        if let Some(url) = &alert_webhook {
                            tracing::error!(
                                service = %supervisor_name,
                                webhook = %url,
                                "health alert triggered: idle beyond threshold"
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
