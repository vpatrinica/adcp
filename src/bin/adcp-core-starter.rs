use tokio::process::Command;
use tokio::signal;
use std::process::Stdio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("Starting ADCP Core Services...");

    // Determine the path to binaries (assuming debug build for dev/test)
    // In production, this would be relative to the executable or in PATH.
    // For this environment, we assume target/debug/

    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("./target/debug"));

    fn spawn_bin(dir: &std::path::Path, name: &str) -> std::io::Result<tokio::process::Child> {
        let mut path = dir.join(name);
        if cfg!(windows) {
            path.set_extension("exe");
        }
        // Fallback to cargo run if binary not found (e.g. running from source root without build)
        if !path.exists() {
             Command::new("cargo")
                .args(&["run", "--bin", name])
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
        } else {
             Command::new(path)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
        }
    }

    // Spawn broker
    let mut broker = spawn_bin(&bin_dir, "adcp-core-broker")?;
    println!("Broker spawned with PID: {:?}", broker.id());

    // Wait a bit for broker to start
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Spawn conf manager
    let mut conf_manager = spawn_bin(&bin_dir, "adcp-conf-manager")?;
    println!("Conf Manager spawned with PID: {:?}", conf_manager.id());

    // Spawn QA
    let mut qa = spawn_bin(&bin_dir, "adcp-core-qa")?;
    println!("QA Watchdog spawned with PID: {:?}", qa.id());

    // Spawn Proc Manager
    let mut proc_manager = spawn_bin(&bin_dir, "adcp-proc-manager")?;
    println!("Proc Manager spawned with PID: {:?}", proc_manager.id());

    println!("All core services started. Press Ctrl-C to stop.");

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    println!("Stopping services...");

    // Kill processes
    let _ = proc_manager.kill().await;
    let _ = qa.kill().await;
    let _ = conf_manager.kill().await;
    let _ = broker.kill().await;

    println!("Stopped.");

    Ok(())
}
