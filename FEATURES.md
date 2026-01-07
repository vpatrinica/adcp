# Implemented Features

This document outlines the features currently implemented in the ADCP acquisition service.

## Core Functionality

### Configuration Management
- Loads operational settings from a TOML configuration file (`config/adcp.toml` by default)
- Supports command-line override of config path via `--config` option
- Provides sensible defaults for optional settings to ensure service resilience
- Configuration includes service name, log level, data directory, serial port, baud rate, idle threshold, and optional alert webhook

### Logging and Telemetry
- Uses structured logging with the `tracing` crate
- Outputs to stderr with configurable verbosity levels (error, warn, info, debug, trace)
- Includes service name and relevant context in log messages
- Supports span events for detailed tracing

### Serial Communication
- Asynchronous serial port reading using `tokio-serial`
- Configurable baud rate (default 115200)
- Handles newline-delimited data streams
- Automatic reconnection attempts on read failures

### Data Parsing
- Parses NMEA-formatted sentences from ADCP devices
- Supports three sentence types:
  - `$PNORI`: Configuration data (instrument type, head ID, beams, cells, blanking distance, cell size, coordinate system)
  - `$PNORS`: Sensor data (timestamp, error/status codes, battery voltage, sound speed, heading, pitch, roll, pressure, temperature, analog inputs)
  - `$PNORC`: Current velocity data (timestamp, cell number, velocities for 4 beams, speed, direction, amplitude, correlation)
- Validates checksums for data integrity
- Handles invalid or missing values (marked as -9 or empty)

### Data Persistence
- Stores parsed frames as JSON lines in daily rotated files
- Files are named by date (e.g., `2024-01-01.jsonl`)
- Located in the configured data directory
- Uses timestamps from frame payloads for proper file rotation during replay

### Health Monitoring
- Tracks metrics: total frames received, parse errors, persistence errors, last frame timestamp
- Periodic health heartbeats logged every 60 seconds
- Alerts when no frames received beyond configurable idle threshold (default 30 seconds)
- Optional webhook logging for alerts

### Service Lifecycle
- Asynchronous service supervisor using Tokio
- Graceful shutdown on Ctrl+C signal
- Clean termination of all background tasks
- Watch channels for coordinated shutdown

### Sample Data Replay
- Sample replay utilities exist (see `simulator::replay_sample`) and are exercised by tests, and a `--replay <path>` CLI flag was added to replay a capture file through the pipeline and exit (useful for deterministic E2E checks).
- Sample replay processes files through the same parsing and persistence pipeline and ensures timestamp-based rotation for replays.
- End-to-end fixtures live under `tests/fixtures/` and are exercised by `tests/e2e.rs` (table-driven scenarios that assert produced dated logs and basic content checks).
- Run E2E: `cargo test --test e2e` or run locally with `cargo run -- --config <path> --replay tests/fixtures/<fixture>.data`.

### Serial Data Backup and Separate Processing
- Recording process writes raw serial captures to a **backup folder** (rolling files) and appends to a **processing folder** simultaneously ✅. For the `data_process_folder`, the recorder uses a per-append mode (open/write/close) to avoid holding long-lived file descriptors that would prevent safe movement by the processor.

- The recorder updates a lightweight `<filename>.writing` marker each time it appends; the processor skips files with recent markers to avoid reading files that are actively being written.
- Processing scans the `data_process_folder`, waits for files to become stable (mtime older than `file_stability_seconds` and no recent writer marker), replays them through the existing parser/persistence pipeline, then moves completed files into a **processed** folder on success or renames them with a `.failed` suffix on permanent failure ✅.
- Separation enables restartable processing that does not interrupt ongoing capture and allows historical processing of backlog ✅.
- Configurable via `AppConfig` (fields: `mode`, `backup_folder`, `data_process_folder`, `processed_folder`, `split_mode`, `max_backup_files`, `max_backup_age_days`, `file_stability_seconds`) 🔧
- File stability timeout configurable via `file_stability_seconds` (default 5s) ⚙️

### Cross-Platform Deployment
- Supports Linux (systemd) and Windows (Windows Service)
- Platform-specific service templates logged at startup
- Conditional compilation for Windows-specific dependencies
- Binary runs identically on both platforms

### Command-Line Interface
- Simple CLI with help option (`--help` or `-h`)
- Supports `--config` for configuration file path
- Positional argument fallback for config path
- Note: the `--sample` CLI replay option was removed; sample replay remains available via test utilities and the `simulator` helper

## Dependencies
- `tokio`: Asynchronous runtime and utilities
- `tracing` and `tracing-subscriber`: Structured logging
- `serde` and `toml`: Configuration serialization
- `chrono`: Date and time handling
- `tokio-serial`: Serial port communication
- `anyhow`: Error handling
- `windows-service`: Windows service integration (Windows only)

## Testing
- Unit tests for configuration parsing
- Integration tests for Linux configuration and Windows service template
- Sample data files and the `simulator::replay_sample` helper are used in tests to validate replay and processing behavior
- Code formatting with `cargo fmt` and linting with `cargo clippy`