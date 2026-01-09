use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecorderStats {
    pub port_name: String,
    pub bytes_read_total: u64,
    pub bytes_per_second: u64,
    pub write_errors: u64,
    pub rotation_count: u64,
    pub last_packet_time: Option<u64>, // Unix timestamp in seconds or milliseconds
    pub uptime_seconds: u64,
}
