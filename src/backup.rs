use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tokio::{
    fs::{create_dir_all, File, OpenOptions},
    io::AsyncWriteExt,
};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn per_append_writes_and_appends() {
        let tmp = tempdir().expect("tmp");
        let dir = tmp.path().to_path_buf();
        let mut b = Backup::new_per_append(&dir).await.expect("new per append");
        let ts = Utc::now();
        b.append("line1", ts).await.expect("write1");
        b.append("line2", ts).await.expect("write2");
        let p = dir.join(format!("{}.raw", ts.date_naive().format("%Y-%m-%d")));
        let content = fs::read_to_string(p).await.expect("read");
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }
}


/// Handles rolling backup files for raw serial data.
pub struct Backup {
    base: PathBuf,
    current_file: Option<File>,
    current_date: Option<chrono::NaiveDate>,
    /// When true, the backup opens, appends, and closes the file on each append call.
    /// This is useful for the processing folder where we must not hold a long-lived
    /// file handle that prevents file rotation and moving by the processing worker.
    per_append: bool,
}

impl Backup {
    pub async fn new(base_dir: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_option(base_dir, false).await
    }

    pub async fn new_per_append(base_dir: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_option(base_dir, true).await
    }

    async fn new_with_option(base_dir: impl AsRef<Path>, per_append: bool) -> Result<Self> {
        let base = base_dir.as_ref().to_path_buf();
        create_dir_all(&base)
            .await
            .with_context(|| format!("failed to create backup directory {}", base.display()))?;

        if !per_append {
            // Task: Protection for the backup folder between reruns.
            // Move any existing .raw files to an archive subfolder to avoid mixing runs.
            let mut entries = tokio::fs::read_dir(&base).await?;
            let mut files_to_archive = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "raw" {
                            files_to_archive.push(path);
                        }
                    }
                }
            }

            if !files_to_archive.is_empty() {
                let archive_name = format!("archive_{}", Utc::now().format("%Y%m%d_%H%M%S"));
                let archive_dir = base.join(archive_name);
                tokio::fs::create_dir_all(&archive_dir).await?;
                for file_path in files_to_archive {
                    if let Some(filename) = file_path.file_name() {
                        let dest = archive_dir.join(filename);
                        tokio::fs::rename(&file_path, &dest).await?;
                    }
                }
                tracing::info!(dir = %archive_dir.display(), "archived existing backup files to prevent overwrite between runs");
            }
        }

        Ok(Self {
            base,
            current_file: None,
            current_date: None,
            per_append,
        })
    }

    /// Appends a line to the current backup file, rolling to a new file if needed.
    /// If `per_append` is set, this method opens, writes and closes the file every call.
    pub async fn append(&mut self, line: &str, timestamp: DateTime<Utc>) -> Result<()> {
        let date = timestamp.date_naive();

        if self.per_append {
            let filename = format!("{}.raw", date.format("%Y-%m-%d"));
            let path = self.base.join(&filename);
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .with_context(|| format!("failed to open backup file {}", path.display()))?;
            file.write_all(line.as_bytes())
                .await
                .context("failed to write to backup file")?;
            file.write_all(b"\n")
                .await
                .context("failed to write newline to backup file")?;
            file.flush().await.context("failed to flush backup file")?;
            // Update marker file to signal recent write activity for processors
            let marker_name = format!("{}.writing", &filename);
            let marker_path = self.base.join(&marker_name);
            // Write current unix timestamp into marker (not strictly necessary; touching mtime is sufficient)
            let mut marker = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&marker_path)
                .await
                .with_context(|| format!("failed to update marker {}", marker_path.display()))?;
            let ts = format!("{}", chrono::Utc::now().timestamp());
            marker.write_all(ts.as_bytes()).await.context("failed to write marker")?;
            marker.flush().await.context("failed to flush marker")?;
            // marker closed when dropped
            return Ok(());
        }

        // Check if we need to roll to a new file
        if self.current_date != Some(date) {
            self.roll_to_date(date).await?;
        }

        if let Some(file) = &mut self.current_file {
            file.write_all(line.as_bytes())
                .await
                .context("failed to write to backup file")?;
            file.write_all(b"\n")
                .await
                .context("failed to write newline to backup file")?;
            file.flush().await.context("failed to flush backup file")?;
        }

        Ok(())
    }

    async fn roll_to_date(&mut self, date: chrono::NaiveDate) -> Result<()> {
        if let Some(file) = self.current_file.take() {
            // Close previous file if any
            drop(file);
        }

        let filename = format!("{}.raw", date.format("%Y-%m-%d"));
        let path = self.base.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open backup file {}", path.display()))?;

        self.current_file = Some(file);
        self.current_date = Some(date);

        Ok(())
    }
}