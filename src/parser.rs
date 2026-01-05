use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

/// Represents a single telemetry frame emitted by the ADCP hardware.
#[derive(Debug, Clone)]
pub struct Frame {
    pub timestamp: DateTime<Utc>,
    pub depth_m: f32,
    pub velocity_m_s: f32,
}

impl Frame {
    pub fn from_line(line: &str) -> Result<Self> {
        let mut parts = line.split(',').map(str::trim);
        let timestamp = parts
            .next()
            .context("missing timestamp in frame")?
            .parse::<DateTime<Utc>>()
            .with_context(|| format!("unable to parse timestamp from '{line}'"))?;
        let velocity = parts
            .next()
            .context("missing velocity in frame")?
            .parse::<f32>()
            .with_context(|| format!("unable to parse velocity from '{line}'"))?;
        let depth = parts
            .next()
            .context("missing depth in frame")?
            .parse::<f32>()
            .with_context(|| format!("unable to parse depth from '{line}'"))?;
        Ok(Self {
            timestamp,
            depth_m: depth,
            velocity_m_s: velocity,
        })
    }

    pub fn to_persistence_line(&self) -> String {
        format!(
            "{},{:.3},{:.3}",
            self.timestamp.to_rfc3339(),
            self.velocity_m_s,
            self.depth_m
        )
    }
}
