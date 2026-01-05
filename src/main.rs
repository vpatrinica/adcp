mod config;
mod logging;
mod platform;
mod service;

use anyhow::Context;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| config::AppConfig::default_path().into());
    let config = config::AppConfig::load(&config_path)
        .with_context(|| format!("unable to load configuration from {}", config_path))?;

    logging::init(&config)?;
    platform::log_platform_guidance();

    service::Service::new(config).run().await
}
