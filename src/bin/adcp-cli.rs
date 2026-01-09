use adcp::telemetry::RecorderStats;
use adcp::AppConfig;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient, RpcEvent, RpcHandlers, RpcResult};
use busrt::QoS;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Row, Table, Cell},
    Terminal,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::io;
use async_trait::async_trait;
use serde_json::Value;

struct AppState {
    config: Option<Value>,
    recorder_stats: HashMap<String, RecorderStats>,
}

struct CliHandlers {
    state: Arc<Mutex<AppState>>,
}

#[async_trait]
impl RpcHandlers for CliHandlers {
    async fn handle_call(&self, _event: RpcEvent) -> RpcResult {
        Ok(None)
    }
    async fn handle_notification(&self, _event: RpcEvent) {}
    async fn handle_frame(&self, frame: busrt::Frame) {
        if let Some(topic) = frame.topic() {
            if topic == "conf.update" {
                if let Ok(json) = serde_json::from_slice::<Value>(frame.payload()) {
                    let mut state = self.state.lock().unwrap();
                    state.config = Some(json);
                }
            } else if topic.starts_with("stat/recorder/") {
                if let Ok(stats) = serde_json::from_slice::<RecorderStats>(frame.payload()) {
                    let mut state = self.state.lock().unwrap();
                    state.recorder_stats.insert(stats.port_name.clone(), stats);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Cannot use tracing to stdout as TUI takes over.
    // tracing_subscriber::fmt::init();

    let name = format!("adcp.cli.{}", std::process::id());

    // Connect to BusRT
    let bus_config = Config::new("127.0.0.1:7777", &name);
    let mut client = Client::connect(&bus_config).await?;

    // Subscribe
    client.subscribe("conf.update", QoS::Processed).await?;
    client.subscribe("stat/recorder/#", QoS::Processed).await?;

    let state = Arc::new(Mutex::new(AppState {
        config: None,
        recorder_stats: HashMap::new(),
    }));

    let handlers = CliHandlers { state: state.clone() };
    let rpc_client = RpcClient::new(client, handlers);

    // Initial fetch
    let response = rpc_client.call(
        "adcp.conf.manager",
        "cmd.conf.get",
        Vec::new().into(),
        QoS::Processed
    ).await;

    if let Ok(response) = response {
        if !response.payload().is_empty() {
             if let Ok(json) = serde_json::from_slice::<Value>(response.payload()) {
                let mut s = state.lock().unwrap();
                s.config = Some(json);
            }
        }
    }

    // TUI setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let s = state.lock().unwrap();

            // Config Table
            let config_str = if let Some(ref c) = s.config {
                format!("{:#}", c)
            } else {
                "Fetching...".to_string()
            };

            // Simplified view for config (just dumping JSON string nicely formatted is hard in table without parsing keys)
            // Let's just show raw lines for now
            let config_rows: Vec<Row> = config_str.lines().map(|l| Row::new(vec![Cell::from(l)])).collect();
            let config_table = Table::new(config_rows, [Constraint::Percentage(100)])
                .block(Block::default().title("Configuration").borders(Borders::ALL));
            f.render_widget(config_table, chunks[0]);

            // Stats Table
            let header = Row::new(vec![
                Cell::from("Port"),
                Cell::from("Uptime (s)"),
                Cell::from("Total Bytes"),
                Cell::from("BPS"),
                Cell::from("Errors"),
            ]).style(Style::default().fg(Color::Yellow));

            let mut stat_rows = Vec::new();
            for stats in s.recorder_stats.values() {
                stat_rows.push(Row::new(vec![
                    Cell::from(stats.port_name.as_str()),
                    Cell::from(stats.uptime_seconds.to_string()),
                    Cell::from(stats.bytes_read_total.to_string()),
                    Cell::from(stats.bytes_per_second.to_string()),
                    Cell::from(stats.write_errors.to_string()),
                ]));
            }

            let stats_table = Table::new(stat_rows, [
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Length(15),
                Constraint::Length(10),
                Constraint::Length(10),
            ])
            .header(header)
            .block(Block::default().title("Recorder Telemetry").borders(Borders::ALL));
            f.render_widget(stats_table, chunks[1]);

        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    break;
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    // TUI cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
