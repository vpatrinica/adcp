use adcp::telemetry::RecorderStats;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{RpcClient, RpcEvent, RpcHandlers, RpcResult};
use busrt::QoS;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::signal;
use async_trait::async_trait;

struct QaHandlers {
    recorders: Arc<Mutex<HashMap<String, RecorderState>>>,
}

struct RecorderState {
    last_activity: Instant,
    last_bps: u64,
}

#[async_trait]
impl RpcHandlers for QaHandlers {
    async fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Ok(None)
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, frame: busrt::Frame) {
        if let Some(topic) = frame.topic() {
            if topic.starts_with("stat/recorder/") {
                if let Ok(stats) = serde_json::from_slice::<RecorderStats>(frame.payload()) {
                    let mut recorders = self.recorders.lock().unwrap();
                    let state = recorders.entry(stats.port_name.clone()).or_insert_with(|| RecorderState {
                        last_activity: Instant::now(),
                        last_bps: 0,
                    });

                    state.last_bps = stats.bytes_per_second;
                    // If flow is positive, update activity
                    if stats.bytes_per_second > 0 {
                        state.last_activity = Instant::now();
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let name = format!("adcp.qa.{}", std::process::id());

    // Connect to BusRT
    let bus_config = Config::new("127.0.0.1:7777", &name);
    let mut client = Client::connect(&bus_config).await?;

    // Subscribe to recorder stats
    client.subscribe("stat/recorder/#", QoS::Processed).await?;

    let recorders = Arc::new(Mutex::new(HashMap::new()));
    let handlers = QaHandlers {
        recorders: recorders.clone(),
    };

    let _rpc_client = RpcClient::new(client, handlers);

    println!("QA Watchdog started");

    // Monitoring Loop
    let recorders_clone = recorders.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut recs = recorders_clone.lock().unwrap();
            let now = Instant::now();

            for (name, state) in recs.iter_mut() {
                if state.last_bps == 0 {
                    let idle = now.duration_since(state.last_activity).as_secs();
                    if idle > 10 {
                        eprintln!("ALERT: Recorder on port {} has 0 flow for {} seconds!", name, idle);
                        // In a real system, we might trigger a restart via process manager here
                    }
                }
            }
        }
    });

    signal::ctrl_c().await?;
    println!("QA Watchdog stopping...");

    Ok(())
}
