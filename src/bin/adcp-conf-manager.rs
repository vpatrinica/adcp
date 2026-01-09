use adcp::AppConfig;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient, RpcEvent, RpcError, RpcHandlers, RpcResult, RPC_ERROR_CODE_INTERNAL};
use std::sync::Arc;
use tokio::signal;
use async_trait::async_trait;

struct ConfRpcHandlers {
    config: Arc<AppConfig>,
}

#[async_trait]
impl RpcHandlers for ConfRpcHandlers {
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        match event.parse_method() {
            Ok("cmd.conf.get") => {
                let json = serde_json::to_vec(&*self.config).map_err(|e| {
                    RpcError::new(RPC_ERROR_CODE_INTERNAL, Some(e.to_string().as_bytes().to_vec()))
                })?;
                Ok(Some(json))
            }
            Ok(_) => Err(RpcError::method(None)),
            Err(_) => Err(RpcError::new(busrt::rpc::RPC_ERROR_CODE_PARSE, None)),
        }
    }

    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, _frame: busrt::Frame) {}
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Load config
    let config_path = AppConfig::default_path();
    let config = Arc::new(AppConfig::load(config_path)?);

    let name = "adcp.conf.manager";

    // Connect to BusRT
    let bus_config = Config::new("127.0.0.1:7777", name);
    let client = Client::connect(&bus_config).await?;

    let handlers = ConfRpcHandlers {
        config: config.clone(),
    };

    let _rpc_client = RpcClient::new(client, handlers);

    println!("Conf manager started: {}", name);

    // Keep alive
    signal::ctrl_c().await?;

    Ok(())
}
