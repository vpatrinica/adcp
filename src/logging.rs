use anyhow::{Error, Result};
use tracing_subscriber::{fmt, EnvFilter, registry::Registry};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::AppConfig;
use std::path::Path;
use std::io::stdout;
use tracing_appender::rolling;
use tracing_appender::non_blocking::WorkerGuard;

pub fn init(config: &AppConfig) -> Result<WorkerGuard> {
    let filter =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new(&config.log_level))?;

    // Ensure deployment log directory exists and set up a rolling file appender per service
    let log_dir = Path::new("./deployment/log");
    std::fs::create_dir_all(log_dir).map_err(|e| Error::msg(e))?;
    let safe_name = config.service_name.replace(' ', "_");

    // Use daily rotation; file names will be like <service>.log.YYYY-MM-DD
    let file_appender = rolling::daily(log_dir, &safe_name);
    let (file_non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // File layer: plain, no ANSI, less span noise
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(file_non_blocking)
        .with_span_events(fmt::format::FmtSpan::NONE);

    // Stdout layer: ansi on for terminal readability
    let stdout_layer = fmt::layer()
        .with_ansi(true)
        .with_writer(stdout)
        .with_span_events(fmt::format::FmtSpan::NONE);

    // Build combined subscriber: EnvFilter applied, with both layers
    Registry::default()
        .with(filter)
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(|err| Error::msg(err))?;

    Ok(guard)
}
