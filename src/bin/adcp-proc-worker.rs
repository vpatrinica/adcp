use adcp::{AppConfig, simulator};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let mut input = None;
    let mut config_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                if i + 1 < args.len() {
                    input = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--config" => {
                if i + 1 < args.len() {
                    config_path = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let input = input.ok_or_else(|| anyhow::anyhow!("missing --input"))?;
    let config_path = config_path.ok_or_else(|| anyhow::anyhow!("missing --config"))?;

    let config = AppConfig::load(&config_path)?;

    println!("Worker processing: {}", input);
    simulator::replay_sample(input, &config).await?;
    println!("Worker finished");

    Ok(())
}
