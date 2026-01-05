# adcp
Cross-platform ADCP acquisition service focused on configuration discipline, structured logging, and graceful lifecycle control on Linux and Windows hosts.

## Design principles
- **Robust configuration**: `config/adcp.toml` captures operational choices so deployments can be audited and scripted.
- **Resilient runtime**: `tokio` + `watch` channels keep a supervisor in charge of clean shutdowns and periodic heartbeats.
- **Observability**: `tracing` + `tracing-subscriber` drive consistent telemetry piped to stderr (pluggable for file or system collectors).
- **Cross-platform readiness**: platform-aware helpers document how to wrap the binary as a systemd unit or Windows service.

## Quick start
1. Install Rust 1.78+ via rustup (Linux or Windows).  
2. Build the service: `cargo build --release`.  
3. Run with the example configuration: `./target/release/adcp --config config/adcp.toml`.  
4. Override `--config` to point to a production-grade TOML file.

## Configuration
| Key | Meaning | Default |
| --- | ------- | ------- |
| `service_name` | Friendly identifier seen in logs and watchdogs | `adcp-supervisor` |
| `log_level` | Tracing verbosity (`error`, `warn`, `info`, `debug`, `trace`) | `info` |
| `data_directory` | Destination directory for rotating files | `./data` |
| `serial_port` | Physical or virtual serial port to bind (e.g., `/dev/ttyUSB0` or `COM3`) | n/a |
| `baud_rate` | Serial baud rate used during handshake | `115200` |

Any missing option falls back to a sane default so the service can self-heal after partial deployments.

## Deployment guidance
The binary runs identically on both OS families, but operational tooling differs.

### Linux (systemd)
Place the binary in `/usr/local/bin/adcp` (or similar) and drop the following unit file under `/etc/systemd/system/adcp.service`:
```
[Unit]
Description=ADCP acquisition service
After=network.target

[Service]
ExecStart=/usr/local/bin/adcp --config /etc/adcp/adcp.toml
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```
Enable and start with `sudo systemctl enable --now adcp`

### Windows service (outline)
1. Copy `target\release\adcp.exe` and the TOML config to `C:\Program Files\adcp`.  
2. Create the service via PowerShell or `sc.exe`:
```
sc create ADCPService binPath= "C:\Program Files\adcp\adcp.exe --config C:\Program Files\adcp\adcp.toml" start= auto
```
3. Start it: `sc start ADCPService` and monitor the Windows Event Log for stdout/stderr forwarders.

The Windows service crate is configured as a target-specific dependency so builds on Linux remain lean.

## Testing
- `cargo test` (executes config parsing validations and future unit coverage).
- Build with `cargo fmt` and `cargo clippy` before tagging releases.

## Implementation plan
1. Core runtime: config loader → logging → service scaffolding.  
2. Delivery: sample configuration + platform descriptor + README guidance.  
3. Next steps: add serial/reader modules, structured persistence, and integration tests.