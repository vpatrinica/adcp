# New Features: Serial Data Backup and Separate Processing

## Overview

This document describes the planned features for separating data capture (recording) from data processing in the ADCP acquisition service. The goal is to enable deployments where raw serial data is captured to a backup location with rolling files, while processing operates on a separate copy of the data, allowing for restartable processing that doesn't interfere with ongoing capture.

## Key Concepts

### Deployment Architecture
- **Recording Process**: Reads from serial port, writes raw data to backup folder (with rolling), and copies to processing folder
- **Processing Process**: Reads from processing folder, parses and persists processed data, moves completed files to processed folder
- **Separation**: Two independent processes communicating via filesystem, allowing different configurations and restartability

### Data Flow
1. Serial data → Backup folder (rolling files) + Processing folder (append/copy)
2. Processing folder → Parsed data in output folder, files moved to processed folder after completion

## New Configuration Parameters

Add the following to `AppConfig`:

```rust
#[derive(Debug, Deserialize)]
pub enum ServiceMode {
    Recording,
    Processing,
}

#[derive(Debug, Deserialize)]
pub enum SplitMode {
    Daily,
    Weekly,
}

pub struct AppConfig {
    // ... existing fields ...
    pub mode: ServiceMode,
    pub backup_folder: String,
    pub data_process_folder: String,
    pub processed_folder: String,
    pub split_mode: SplitMode,
    pub max_backup_files: Option<usize>, // for size-based rolling
    pub max_backup_age_days: Option<u64>, // for age-based cleanup
}
```

With defaults:
- mode: Recording
- backup_folder: "./backup"
- data_process_folder: "./to_process"
- processed_folder: "./processed"
- split_mode: Daily
- max_backup_files: None
- max_backup_age_days: None

## Recording Mode Features

### Backup Folder
- Raw serial data written to rolling files
- Daily rolling by default, weekly optional
- Append-only: never overwrites existing files
- Optional size-based limits with file count or age-based cleanup
- Files named by date (e.g., `2024-01-01.raw`, `2024-01-01-02.raw` for multiple per day if needed)

### Processing Folder
- Copy of raw data for processing
- Append to current file until split condition met
- Allows processing to work on complete data segments
- Processing can start on historical data while recording continues

### Serial Reading
- Unchanged core logic, but now writes to two locations
- Maintains existing error handling and reconnection

## Processing Mode Features

### Input Handling
- Reads from `data_process_folder`
- Processes files in chronological order
- For each file, checks if it's still being written to (by recording process)
- Waits for file to be "stable" (no writes for configurable period, e.g., 5 seconds)

### Processing Logic
- Uses existing sample replay logic but adapted for directories
- Processes each file completely before moving to next
- Maintains existing parsing, validation, and persistence

### Output Management
- Processed data goes to existing `data_directory`
- Successfully processed files moved to `processed_folder`
- Failed files can be quarantined or retried

### Restartability
- Tracks progress by file modification times or explicit markers
- Can resume from last successfully processed file
- Handles partial processing gracefully

## Implementation Plan

### Phase 1: Configuration and Infrastructure
1. Update `config.rs` to add new fields with defaults
2. Update `AppConfig` deserialization and validation
3. Add new modules: `backup.rs` for rolling file management, `processing.rs` for processing logic
4. Update `Cargo.toml` if needed (likely no new dependencies)

### Phase 2: Recording Mode Implementation
1. Modify `service.rs` to check mode and branch logic
2. Implement `backup.rs`:
   - Rolling file creation (daily/weekly)
   - Append-only writing
   - Optional cleanup based on count/age
3. Modify serial reading to write to backup and processing folders
4. Update persistence logic for processing folder (simple append or rolling)

### Phase 3: Processing Mode Implementation
1. Implement `processing.rs`:
   - Directory scanning for files to process
   - File stability checking (modification time monitoring)
   - Progress tracking and resume logic
2. Extend sample replay to handle directories instead of single files
3. Add file movement logic for completed/failed processing

### Phase 4: Integration and Testing
1. Update `main.rs` to handle mode-specific initialization
2. Update CLI help and validation
3. Add integration tests for both modes
4. Update example configurations

### Phase 5: Documentation and Deployment
1. Update README.md with deployment examples for separate processes
2. Update FEATURES.md with new capabilities
3. Create example configurations for recording and processing setups
4. Document filesystem layout and operational procedures

## Deployment Examples

### Recording Configuration
```toml
service_name = "adcp-recorder"
mode = "recording"
serial_port = "/dev/ttyUSB0"
backup_folder = "/data/adcp/backup"
data_process_folder = "/data/adcp/to_process"
split_mode = "daily"
max_backup_age_days = 30
```

### Processing Configuration
```toml
service_name = "adcp-processor"
mode = "processing"
data_process_folder = "/data/adcp/to_process"
processed_folder = "/data/adcp/processed"
data_directory = "/data/adcp/output"
```

### Systemd Services
- `adcp-recorder.service`: Runs recording process
- `adcp-processor.service`: Runs processing process with dependency on recorder

## Operational Considerations

### File Synchronization
- Processing waits for files to stabilize before processing
- Use filesystem events or polling for file completion detection
- Handle network filesystems carefully (potential race conditions)

### Error Handling
- Recording failures don't stop processing (and vice versa)
- Failed processing files can be retried or quarantined
- Backup integrity is critical - prefer to fail recording rather than lose data

### Performance
- Recording should be lightweight to avoid serial buffer overflows
- Processing can be CPU-intensive but should not block recording
- Consider memory usage for large files

### Monitoring
- Extend metrics to track recording vs processing stats
- Alert on backup disk space, processing backlog, etc.
- Separate health checks for each process

## Testing Strategy

### Unit Tests
- Backup rolling logic
- File stability detection
- Processing resume logic

### Integration Tests
- Full recording + processing pipeline
- File movement and cleanup
- Concurrent operation simulation

### Manual Testing
- Hardware testing with real serial data
- Long-running tests for rolling behavior
- Failure recovery scenarios