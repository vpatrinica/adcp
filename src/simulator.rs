use crate::{metrics::Metrics, parser::Frame, persistence::Persistence, AppConfig};
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};
use tokio::fs;

/// Replays a newline-delimited capture file through the parser and persistence pipeline.
pub async fn replay_sample(sample_path: impl AsRef<Path>, config: &AppConfig) -> Result<()> {
    let data_dir = &config.data_directory;
    let persistence = Arc::new(
        Persistence::new(data_dir)
            .await
            .context("prepare persistence backend")?,
    );
    let metrics = Metrics::new();

    let raw = fs::read_to_string(sample_path.as_ref())
        .await
        .with_context(|| format!("open sample capture {}", sample_path.as_ref().display()))?;

    for raw_line in normalize_capture(&raw) {
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
    // The bundled sample uses literal "\\r\\n" sequences; treat both literal and actual CRLF
    // as frame delimiters and rebuild clean lines that start with '$'.
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
