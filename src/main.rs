use adcp::{logging, platform, AppConfig, Service, simulator};
use anyhow::{bail, Context, Result};

#[derive(Debug)]
struct Cli {
    config_path: String,
    replay: Option<String>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut config_path: Option<String> = None;
        let mut replay: Option<String> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--config requires a path"))?;
                    config_path = Some(value);
                }
                "--replay" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--replay requires a path"))?;
                    replay = Some(value);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: adcp [--config <path>] [--replay <sample>]\n\
                         --config <path>   Path to TOML configuration (default: config/adcp.toml)\n\
                         --replay <path>   Replay a capture file through the pipeline and exit"
                    );
                    std::process::exit(0);
                }
                other => {
                    if config_path.is_none() {
                        config_path = Some(other.to_string());
                    } else {
                        bail!("unknown argument '{other}'");
                    }
                }
            }
        }

        Ok(Self {
            config_path: config_path.unwrap_or_else(|| AppConfig::default_path().into()),
            replay,
        })
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse()?;

    let config = AppConfig::load(&cli.config_path)
        .with_context(|| format!("unable to load configuration from {}", cli.config_path))?;

    logging::init(&config)?;
    platform::log_platform_guidance();

    if let Some(sample) = cli.replay {
        simulator::replay_sample(sample, &config).await?;
        return Ok(());
    }

    Service::new(config).run().await
}
