use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::fs::File;
use tokio_serial::{SerialPortBuilderExt, SerialStream};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

enum ReaderSource {
    Serial(BufReader<SerialStream>),
    File(BufReader<File>),
}

/// A minimal async wrapper around a serial stream or file that returns newline-delimited
/// strings. The buffer is reused to avoid repeated allocations.
pub struct SerialPort {
    reader: ReaderSource,
    buffer: String,
}

impl SerialPort {
    pub async fn connect(port: &str, baud_rate: u32) -> Result<Self> {
        let _metadata = std::fs::metadata(port)?;
        
        let is_fifo_or_file = {
            #[cfg(unix)]
            { _metadata.file_type().is_fifo() || _metadata.is_file() }
            #[cfg(not(unix))]
            { _metadata.is_file() }
        };

        let reader = if is_fifo_or_file {
            // Treat as FIFO/file
            let file = File::open(port)
                .await
                .with_context(|| format!("failed to open FIFO/file {}", port))?;
            ReaderSource::File(BufReader::new(file))
        } else {
            // Treat as serial port
            let builder = tokio_serial::new(port, baud_rate);
            let stream = builder
                .open_native_async()
                .with_context(|| format!("failed to open serial port {}", port))?;
            ReaderSource::Serial(BufReader::new(stream))
        };
        Ok(Self {
            reader,
            buffer: String::with_capacity(256),
        })
    }

    pub async fn next_line(&mut self) -> Result<Option<String>> {
        self.buffer.clear();
        let bytes = match &mut self.reader {
            ReaderSource::Serial(r) => r.read_line(&mut self.buffer).await?,
            ReaderSource::File(r) => r.read_line(&mut self.buffer).await?,
        };
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
