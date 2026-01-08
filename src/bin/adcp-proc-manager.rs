use adcp::{simulator, AppConfig};
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{RpcClient, RpcEvent, RpcHandlers, RpcResult};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal;
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};

struct ProcHandlers;

#[async_trait]
impl RpcHandlers for ProcHandlers {
    async fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Ok(None)
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, _frame: busrt::Frame) {}
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Load config
    let config_path = AppConfig::default_path();
    let config = AppConfig::load(config_path)?;
    let app_config = Arc::new(config.clone());

    let name = format!("adcp.proc.manager.{}", std::process::id());

    // Connect to BusRT
    let bus_config = Config::new("127.0.0.1:7777", &name);
    let client = Client::connect(&bus_config).await?;

    let _rpc_client = RpcClient::new(client, ProcHandlers);

    println!("Processing Manager started");
    println!("Watching: {}", config.data_process_folder);

    let process_folder = PathBuf::from(&config.data_process_folder);
    let processed_folder = PathBuf::from(&config.processed_folder);
    fs::create_dir_all(&process_folder)?;
    fs::create_dir_all(&processed_folder)?;

    let stability_sec = config.file_stability_seconds;

    // Watch Loop
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if let Err(e) = scan_and_process(&process_folder, &processed_folder, stability_sec, &app_config).await {
                eprintln!("Processing scan error: {}", e);
            }
        }
    });

    signal::ctrl_c().await?;
    println!("Processing Manager stopping...");

    Ok(())
}

async fn scan_and_process(src: &Path, dst: &Path, stability_sec: u64, config: &AppConfig) -> std::io::Result<()> {
    let mut entries = fs::read_dir(src)?;
    let now = std::time::SystemTime::now();

    while let Some(entry) = entries.next() {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            // Check if .writing marker exists
            let file_name = path.file_name().unwrap().to_string_lossy();
            if file_name.ends_with(".writing") {
                continue;
            }

            // Check for corresponding .writing file
            let writing_marker = src.join(format!("{}.writing", file_name));
            if writing_marker.exists() {
                continue;
            }

            // Check stability (mtime)
            let metadata = fs::metadata(&path)?;
            if let Ok(mtime) = metadata.modified() {
                if let Ok(age) = now.duration_since(mtime) {
                    if age.as_secs() >= stability_sec {
                        println!("Processing file: {:?}", path);

                        match simulator::replay_sample(&path, config).await {
                            Ok(_) => {
                                println!("Processing successful.");
                                let dest_path = dst.join(path.file_name().unwrap());
                                fs::rename(&path, &dest_path)?;
                                println!("Moved to: {:?}", dest_path);
                            }
                            Err(e) => {
                                eprintln!("Processing failed for {:?}: {}", path, e);
                                let dest_path = dst.join(format!("{}.failed", path.file_name().unwrap().to_string_lossy()));
                                fs::rename(&path, &dest_path)?;
                                println!("Moved to: {:?}", dest_path);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
