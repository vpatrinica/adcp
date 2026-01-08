use crate::{metrics::Metrics, parser::Frame, persistence::Persistence, AppConfig};
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Replays a newline-delimited capture file through the parser and persistence pipeline.
pub async fn replay_sample(sample_path: impl AsRef<Path>, config: &AppConfig) -> Result<()> {
    let data_dir = &config.data_directory;
    let persistence = Arc::new(
        Persistence::new(data_dir)
            .await
            .context("prepare persistence backend")?,
    );
    let metrics = Metrics::new();

    let file = File::open(sample_path.as_ref())
        .await
        .with_context(|| format!("open sample capture {}", sample_path.as_ref().display()))?;

    let mut reader = BufReader::new(file).lines();

    while let Some(line) = reader.next_line().await? {
        // The original logic handled literal "\r\n" in the sample file for tests.
        // For line-by-line reading, `lines()` handles actual CRLF.
        // But if the file contains *literal* escaped chars "\\r\\n" (as in the test fixture),
        // we might need to handle it.
        // However, `lines()` splits on \n or \r\n.
        // The normalize_capture function was doing `replace("\\r\\n", "\n")` which suggests literal string replacement.
        // If real files don't contain "\\r\\n" literals but actual CRLF, streaming is fine.
        // If they do, we need to process the line.
        // Assuming test fixtures might be special.
        // Let's preserve normalization per line.

        let normalized = line.replace("\\r\\n", "\n").replace('\r', "\n");
        // Split by $ if multiple frames per line (though usually 1 per line)
        for chunk in normalized.split('$') {
             let trimmed = chunk.trim();
             if trimmed.is_empty() {
                 continue;
             }
             let raw_line = format!("${}", trimmed);

             match Frame::from_line(&raw_line) {
                Ok(frame) => {
                    persistence.append(&frame).await?;
                    metrics.record_frame();
                }
                Err(err) => {
                    metrics.record_parse_error();
                    tracing::warn!(error = %err, frame = %raw_line, "sample frame rejected");
                }
            }
        }
    }

    let snapshot = metrics.snapshot();
    tracing::info!(
        frames = snapshot.frames,
        parse_errors = snapshot.parse_errors,
        persistence_errors = snapshot.persistence_errors,
        data_dir = %data_dir,
        "sample replay completed"
    );

    Ok(())
}

fn normalize_capture(raw: &str) -> Vec<String> {
    let normalized = raw.replace("\\r\\n", "\n").replace('\r', "\n");
    normalized
        .split('$')
        .filter_map(|chunk| {
            let trimmed = chunk.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("${}", trimmed))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_capture;

    #[test]
    fn normalizes_literal_crlf_sequences() {
        let raw = "$PNORI,4*41\\r\\n$PNORS,010526,220800*77";
        let lines = normalize_capture(raw);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "$PNORI,4*41");
        assert_eq!(lines[1], "$PNORS,010526,220800*77");
    }
}
