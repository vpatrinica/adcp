use tokio::process::Command;
use tokio::signal;
use std::process::Stdio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("Starting ADCP Core Services...");

    // Spawn broker
    let mut broker = Command::new("cargo")
        .args(&["run", "--bin", "adcp-core-broker"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    println!("Broker spawned with PID: {:?}", broker.id());

    // Wait a bit for broker to start
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Spawn conf manager
    let mut conf_manager = Command::new("cargo")
        .args(&["run", "--bin", "adcp-conf-manager"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    println!("Conf Manager spawned with PID: {:?}", conf_manager.id());

    println!("All core services started. Press Ctrl-C to stop.");

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    println!("Stopping services...");

    // Kill processes
    let _ = conf_manager.kill().await;
    let _ = broker.kill().await;

    println!("Stopped.");

    Ok(())
}
