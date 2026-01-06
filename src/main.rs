use adcp::{logging, platform, simulator, AppConfig, Service};
use anyhow::{bail, Context, Result};

#[derive(Debug)]
struct Cli {
    config_path: String,
    sample_path: Option<String>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut config_path: Option<String> = None;
        let mut sample_path: Option<String> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--config requires a path"))?;
                    config_path = Some(value);
                }
                "--sample" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--sample requires a path"))?;
                    sample_path = Some(value);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: adcp [--config <path>] [--sample <capture>]\n\
                         --config <path>   Path to TOML configuration (default: config/adcp.toml)\n\
                         --sample <path>   Replay newline-delimited capture file instead of serial"
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
            sample_path,
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

    if let Some(sample) = cli.sample_path {
        tracing::info!(sample = %sample, data_dir = %config.data_directory, "replaying capture from sample file");
        simulator::replay_sample(sample, &config).await
    } else {
        Service::new(config).run().await
    }
}
