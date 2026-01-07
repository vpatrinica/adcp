use adcp::telemetry::RecorderStats;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient, RpcEvent, RpcHandlers, RpcResult};
use busrt::QoS;
use tokio::signal;
use async_trait::async_trait;
use serde_json::Value;

struct CliHandlers;

#[async_trait]
impl RpcHandlers for CliHandlers {
    async fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Ok(None)
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, frame: busrt::Frame) {
        if let Some(topic) = frame.topic() {
            if topic == "conf.update" {
                println!("Received config update: {:?}", frame.payload());
            } else if topic.starts_with("stat/recorder/") {
                if let Ok(stats) = serde_json::from_slice::<RecorderStats>(frame.payload()) {
                    println!("Recorder Stats [{}]: uptime={}s, bytes={}, bps={}",
                        stats.port_name, stats.uptime_seconds, stats.bytes_read_total, stats.bytes_per_second);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let name = format!("adcp.cli.{}", std::process::id());

    // Connect to BusRT
    let bus_config = Config::new("127.0.0.1:7777", &name);
    let mut client = Client::connect(&bus_config).await?;

    // Subscribe to config updates
    client.subscribe("conf.update", QoS::Processed).await?;
    // Subscribe to recorder stats
    client.subscribe("stat/recorder/#", QoS::Processed).await?;

    let handlers = CliHandlers;
    let rpc_client = RpcClient::new(client, handlers);

    // Call cmd.conf.get
    println!("Fetching configuration...");
    let response = rpc_client.call(
        "adcp.conf.manager",
        "cmd.conf.get",
        Vec::new().into(), // Empty payload
        QoS::Processed
    ).await?;

    let payload = response.payload();
    if payload.is_empty() {
        println!("Received empty config");
    } else {
        match serde_json::from_slice::<Value>(payload) {
            Ok(json) => println!("Current Config:\n{:#}", json),
            Err(e) => println!("Failed to parse config: {}", e),
        }
    }

    println!("Listening for updates. Press Ctrl-C to exit.");
    signal::ctrl_c().await?;

    Ok(())
}
