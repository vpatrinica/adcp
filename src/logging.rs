use anyhow::{Error, Result};
use tracing_subscriber::{fmt, EnvFilter};

use crate::config::AppConfig;

pub fn init(config: &AppConfig) -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new(&config.log_level))?;
    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_span_events(fmt::format::FmtSpan::FULL)
        .try_init()
        .map_err(|err| Error::msg(err))?;
    Ok(())
}
