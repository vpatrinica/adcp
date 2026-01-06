use crate::parser::Frame;
use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use std::path::{Path, PathBuf};
use tokio::{
    fs::{create_dir_all, File, OpenOptions},
    io::AsyncWriteExt,
    sync::Mutex,
};

struct PersistenceInner {
    date: Option<NaiveDate>,
    file: Option<File>,
    pending: Vec<String>,
}

/// Handles daily rotating files while serializing frames into structured log lines.
pub struct Persistence {
    base: PathBuf,
    inner: Mutex<PersistenceInner>,
}

impl Persistence {
    pub async fn new(base_dir: impl AsRef<Path>) -> Result<Self> {
        let base = base_dir.as_ref().to_path_buf();
        create_dir_all(&base)
            .await
            .with_context(|| format!("failed to create data directory {}", base.display()))?;
        Ok(Self {
            base,
            inner: Mutex::new(PersistenceInner {
                date: None,
                file: None,
                pending: Vec::new(),
            }),
        })
    }

    pub async fn append(&self, frame: &Frame) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let frame_line = frame.to_persistence_line();
        let frame_date = frame.payload.sent_at().map(|dt| dt.date_naive());

        let target_date = match (frame_date, inner.date) {
            (Some(date), current) if current != Some(date) => {
                inner.file = Some(self.open_file(date).await?);
                inner.date = Some(date);
                // Flush any pending undated lines into the new file.
                if !inner.pending.is_empty() {
                    let pending = std::mem::take(&mut inner.pending);
                    if let Some(file) = inner.file.as_mut() {
                        for line in pending {
                            file.write_all(line.as_bytes())
                                .await
                                .context("failed to write pending frame")?;
                            file.write_all(b"\n")
                                .await
                                .context("failed to terminate pending frame")?;
                        }
                        file.flush().await.context("failed to flush pending frames")?;
                    }
                }
                Some(date)
            }
            (Some(date), current) => {
                if current.is_none() {
                    inner.file = Some(self.open_file(date).await?);
                    inner.date = Some(date);
                    // Flush pending as above, though none expected when first file opens.
                    if !inner.pending.is_empty() {
                        let pending = std::mem::take(&mut inner.pending);
                        if let Some(file) = inner.file.as_mut() {
                            for line in pending {
                                file.write_all(line.as_bytes())
                                    .await
                                    .context("failed to write pending frame")?;
                                file.write_all(b"\n")
                                    .await
                                    .context("failed to terminate pending frame")?;
                            }
                            file.flush().await.context("failed to flush pending frames")?;
                        }
                    }
                }
                Some(date)
            }
            (None, Some(date)) => Some(date),
            (None, None) => {
                inner.pending.push(frame_line);
                return Ok(());
            }
        };

        if let Some(file) = inner.file.as_mut() {
            file.write_all(frame_line.as_bytes())
                .await
                .context("failed to write frame")?;
            file.write_all(b"\n")
                .await
                .context("failed to terminate frame")?;
            file.flush().await.context("failed to flush frame")?;
        } else {
            // This should be unreachable, but keep a guard.
            anyhow::bail!("persistence file not initialized for date {:?}", target_date);
        }
        Ok(())
    }

    async fn open_file(&self, date: NaiveDate) -> Result<File> {
        let filename = format!("adcp-{}.log", date.format("%Y-%m-%d"));
        let path = self.base.join(filename);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open {}", path.display()))
    }

    pub async fn current_path(&self) -> PathBuf {
        let inner = self.inner.lock().await;
        let date = inner.date.unwrap_or_else(|| Utc::now().date_naive());
        self.base
            .join(format!("adcp-{}.log", date.format("%Y-%m-%d")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Frame;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn rotates_using_frame_timestamp_date() {
        let tmp = tempdir().expect("temp dir");
        let persistence = Persistence::new(tmp.path())
            .await
            .expect("persistence backend");

        // Frame with timestamped payload sets recorded_at to 2026-01-05
        let sensor = Frame::from_line(
            "$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77",
        )
        .expect("parse sensor");
        persistence.append(&sensor).await.expect("persist sensor");

        let current_path = persistence.current_path().await;
        assert!(
            current_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .contains("2026-01-05"),
            "expected file to reflect payload date"
        );

        let content = fs::read_to_string(current_path).expect("read log file");
        assert!(content.contains("PNORS"));
    }

    #[tokio::test]
    async fn buffers_undated_until_df100_timestamp_present() {
        let tmp = tempdir().expect("temp dir");
        let persistence = Persistence::new(tmp.path())
            .await
            .expect("persistence backend");

        // PNORI has no timestamp; it should be buffered until a dated frame arrives.
        let config = Frame::from_line(
            "$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*41",
        )
        .expect("parse config");
        persistence.append(&config).await.expect("buffer config");

        // First dated frame establishes the rotation date and flushes pending config.
        let sensor = Frame::from_line(
            "$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77",
        )
        .expect("parse sensor");
        persistence.append(&sensor).await.expect("persist sensor");

        let dated_log = tmp.path().join("adcp-2026-01-05.log");
        let content = fs::read_to_string(dated_log).expect("read dated log");
        assert!(content.contains("PNORI"));
        assert!(content.contains("PNORS"));
    }
}
