use busrt::broker::{Broker, ServerConfig};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let mut broker = Broker::new();
    let config = ServerConfig::default()
        .buf_ttl(std::time::Duration::from_millis(100));

    // Create a TCP server on port 7777
    broker.spawn_tcp_server("127.0.0.1:7777", config).await?;

    println!("Broker started on 127.0.0.1:7777");

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    println!("Broker shutting down...");

    Ok(())
}
