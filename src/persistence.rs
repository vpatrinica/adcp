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
            }),
        })
    }

    pub async fn append(&self, frame: &Frame) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let today = Utc::now().date_naive();
        if inner.date != Some(today) || inner.file.is_none() {
            inner.file = Some(self.open_file(today).await?);
            inner.date = Some(today);
        }
        if let Some(file) = inner.file.as_mut() {
            file.write_all(frame.to_persistence_line().as_bytes())
                .await
                .context("failed to write frame")?;
            file.write_all(b"\n")
                .await
                .context("failed to terminate frame")?;
            file.flush().await.context("failed to flush frame")?;
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
