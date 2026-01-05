use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// A minimal async wrapper around a serial stream that returns newline-delimited
/// strings. The buffer is reused to avoid repeated allocations.
pub struct SerialPort {
    reader: BufReader<SerialStream>,
    buffer: String,
}

impl SerialPort {
    pub async fn connect(port: &str, baud_rate: u32) -> Result<Self> {
        let builder = tokio_serial::new(port, baud_rate);
        let stream = builder
            .open_native_async()
            .with_context(|| format!("failed to open serial port {}", port))?;
        Ok(Self {
            reader: BufReader::new(stream),
            buffer: String::with_capacity(256),
        })
    }

    pub async fn next_line(&mut self) -> Result<Option<String>> {
        self.buffer.clear();
        let bytes = self.reader.read_line(&mut self.buffer).await?;
        if bytes == 0 {
            return Ok(None);
        }
        let line = self
            .buffer
            .trim_end_matches(|c| c == '\r' || c == '\n')
            .to_string();
        Ok(Some(line))
    }
}
