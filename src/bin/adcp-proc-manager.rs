use adcp::AppConfig;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{RpcClient, RpcEvent, RpcHandlers, RpcResult};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal;
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Semaphore;
use tokio::process::Command;

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
    let config_path_str = AppConfig::default_path();
    let config = AppConfig::load(config_path_str)?;
    // Use configured path if possible, but for spawning workers we need to pass a path they can read.
    // Assuming AppConfig::default_path() works or CLI arg passed.

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

    // Concurrency limit
    let semaphore = Arc::new(Semaphore::new(4)); // Max 4 concurrent workers

    // Watch Loop
    let config_path_owned = config_path_str.to_string();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if let Err(e) = scan_and_process(&process_folder, &processed_folder, stability_sec, &config_path_owned, &semaphore).await {
                eprintln!("Processing scan error: {}", e);
            }
        }
    });

    signal::ctrl_c().await?;
    println!("Processing Manager stopping...");

    Ok(())
}

async fn scan_and_process(src: &Path, dst: &Path, stability_sec: u64, config_path: &str, semaphore: &Arc<Semaphore>) -> std::io::Result<()> {
    let mut entries = fs::read_dir(src)?;
    let now = std::time::SystemTime::now();

    while let Some(entry) = entries.next() {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            if file_name.ends_with(".writing") || file_name.ends_with(".failed") {
                continue;
            }

            let writing_marker = src.join(format!("{}.writing", file_name));
            if writing_marker.exists() {
                continue;
            }

            let metadata = fs::metadata(&path)?;
            if let Ok(mtime) = metadata.modified() {
                if let Ok(age) = now.duration_since(mtime) {
                    if age.as_secs() >= stability_sec {
                        // Acquire permit
                        let permit = match semaphore.clone().acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => break, // closed
                        };

                        let path_clone = path.clone();
                        let dst_clone = dst.to_path_buf();
                        let config_path_clone = config_path.to_string();
                        let file_name_clone = file_name.clone();

                        // Rename to .processing to avoid double processing
                        let processing_path = src.join(format!("{}.processing", file_name));
                        if let Err(e) = fs::rename(&path, &processing_path) {
                            eprintln!("Failed to mark file as processing: {}", e);
                            continue; // Skip spawning
                        }

                        let input_path = processing_path.clone();

                        println!("Spawning worker for: {:?}", processing_path);

                        // Spawn worker
                        tokio::spawn(async move {
                             let _permit = permit; // hold permit until end of task

                             // Spawn process
                             // Determine binary path
                             let bin_dir = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                                .unwrap_or_else(|| std::path::PathBuf::from("./target/debug"));

                             let worker_bin = if cfg!(windows) {
                                 bin_dir.join("adcp-proc-worker.exe")
                             } else {
                                 bin_dir.join("adcp-proc-worker")
                             };

                             let status = Command::new(worker_bin)
                                .arg("--input")
                                .arg(&input_path)
                                .arg("--config")
                                .arg(&config_path_clone)
                                .status()
                                .await;

                             match status {
                                 Ok(s) if s.success() => {
                                     println!("Worker success for {:?}", input_path);
                                     let dest_path = dst_clone.join(&file_name_clone);
                                     let _ = fs::rename(&input_path, &dest_path);
                                 }
                                 Ok(s) => {
                                     eprintln!("Worker failed for {:?} with status {}", input_path, s);
                                     let dest_path = dst_clone.join(format!("{}.failed", file_name_clone));
                                     let _ = fs::rename(&input_path, &dest_path);
                                 }
                                 Err(e) => {
                                      eprintln!("Failed to spawn worker for {:?}: {}", input_path, e);
                                      // Rename to failed to avoid loop
                                      let dest_path = dst_clone.join(format!("{}.failed", file_name_clone));
                                      let _ = fs::rename(&input_path, &dest_path);
                                 }
                             }
                        });

                        // We continue loop to spawn more if permit available, but since we are iterating
                        // over fs read_dir, we might pick up the same file again if not moved yet?
                        // scan_and_process runs every 2s. If worker takes longer, file is still there.
                        // We need to mark it as "processing" or move it to a temp folder?
                        // Or rely on the fact that if we just spawned it, we shouldn't touch it.
                        // But scan_and_process is stateless between ticks.

                        // FIX: Move to "processing" staging area?
                        // Or rename to .processing?
                        // Let's rename to .processing before spawning.

                        // Actually, I can't rename inside the loop easily if I want to keep it simple.
                        // But if I don't, next tick will spawn another worker for the same file.
                        // So I MUST mark it.

                        // Let's rename source file to .processing
                    }
                }
            }
        }
    }
    Ok(())
}
