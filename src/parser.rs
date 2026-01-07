use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Serialize;

/// A validated NMEA frame captured from the ADCP stream.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Frame {
    /// When the service received the line (uses payload timestamp when present).
    pub recorded_at: DateTime<Utc>,
    pub raw: String,
    pub checksum: Checksum,
    pub payload: Payload,
    /// Parts of the raw line that were discarded during parsing.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub discarded: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Checksum {
    pub provided: u8,
    pub computed: u8,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Config(ConfigSentence),
    Sensor(SensorSentence),
    Current(CurrentSentence),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConfigSentence {
    pub instrument_type: InstrumentType,
    pub head_id: String,
    pub beams: u8,
    pub cells: u16,
    pub blanking_m: f32,
    pub cell_size_m: f32,
    pub coordinate_system: CoordinateSystem,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentType {
    Signature,
    Other(u8),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSystem {
    Enu,
    Xyz,
    Beam,
    Unknown(u8),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SensorSentence {
    pub sent_at: DateTime<Utc>,
    pub error_code_hex: u32,
    pub status_code_hex: u32,
    pub battery_voltage_v: Option<f32>,
    pub sound_speed_m_s: Option<f32>,
    pub heading_deg: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub roll_deg: Option<f32>,
    pub pressure_dbar: Option<f32>,
    pub temperature_c: Option<f32>,
    pub analog_input_1: Option<f32>,
    pub analog_input_2: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CurrentSentence {
    pub sent_at: DateTime<Utc>,
    pub cell_number: u16,
    pub velocity_1_m_s: Option<f32>,
    pub velocity_2_m_s: Option<f32>,
    pub velocity_3_m_s: Option<f32>,
    pub velocity_4_m_s: Option<f32>,
    pub speed_m_s: Option<f32>,
    pub direction_deg: Option<f32>,
    pub amplitude_unit: AmplitudeUnit,
    pub amplitude_beam_1: Option<u8>,
    pub amplitude_beam_2: Option<u8>,
    pub amplitude_beam_3: Option<u8>,
    pub amplitude_beam_4: Option<u8>,
    pub correlation_beam_1_pct: Option<u8>,
    pub correlation_beam_2_pct: Option<u8>,
    pub correlation_beam_3_pct: Option<u8>,
    pub correlation_beam_4_pct: Option<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AmplitudeUnit {
    Counts,
    Unknown(String),
}

impl Frame {
    pub fn from_line(line: &str) -> Result<Self> {
        let raw = line.trim_end_matches(|c| c == '\r' || c == '\n').trim();
        let (provided, computed, body, discarded) = validate_checksum(raw)?;
        let fields: Vec<&str> = body.split(',').collect();
        let ident = fields
            .get(0)
            .copied()
            .ok_or_else(|| anyhow!("missing sentence identifier"))?;
        let payload = match ident {
            "PNORI" => Payload::Config(parse_config(&fields[1..])?),
            "PNORS" => Payload::Sensor(parse_sensor(&fields[1..])?),
            "PNORC" => Payload::Current(parse_current(&fields[1..])?),
            other => bail!("unsupported sentence '{other}'"),
        };
        let recorded_at = payload.sent_at().unwrap_or_else(Utc::now);
        Ok(Self {
            recorded_at,
            raw: raw.to_string(),
            checksum: Checksum {
                provided,
                computed,
                valid: provided == computed,
            },
            payload,
            discarded,
        })
    }

    pub fn to_persistence_line(&self) -> String {
        serde_json::to_string(self).expect("frame serialization cannot fail")
    }
}

impl Payload {
    pub fn sent_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Payload::Config(_) => None,
            Payload::Sensor(s) => Some(s.sent_at),
            Payload::Current(c) => Some(c.sent_at),
        }
    }
}

fn validate_checksum(raw: &str) -> Result<(u8, u8, &str, Vec<String>)> {
    let mut discarded = Vec::new();
    let (body_raw, checksum_hex) = raw
        .rsplit_once('*')
        .ok_or_else(|| anyhow!("NMEA sentence missing '*' checksum delimiter"))?;
    let original_checksum = checksum_hex;
    
    let mut hex_chars = String::with_capacity(2);
    let mut last_hex_pos = 0;
    for (i, c) in checksum_hex.chars().enumerate() {
        if c.is_ascii_hexdigit() {
            hex_chars.push(c);
            if hex_chars.len() == 2 {
                last_hex_pos = i + 1;
                break;
            }
        } else if !c.is_whitespace() {
            if !hex_chars.is_empty() {
                break;
            }
        }
    }
    if hex_chars.len() != 2 {
        bail!("checksum '{}' is not two hex digits, original '{}'", hex_chars, original_checksum);
    }
    if last_hex_pos < original_checksum.len() {
        let junk = &original_checksum[last_hex_pos..];
        if !junk.trim().is_empty() {
            discarded.push(junk.to_string());
        }
    }

    let provided = u8::from_str_radix(&hex_chars, 16)
        .with_context(|| format!("checksum '{}' is not hex, original '{}'", hex_chars, original_checksum))?;

    // If the body contains junk before a known sentence ($PNORC/$PNORS/$PNORI), trim it.
    let mut body = body_raw;
    let mut found_pos = None;
    for marker in ["$PNORC", "$PNORS", "$PNORI"] {
        if let Some(pos) = body.find(marker) {
            if found_pos.map_or(true, |p| pos < p) {
                found_pos = Some(pos);
            }
        }
    }
    
    if let Some(pos) = found_pos {
        if pos > 0 {
            let junk = &body[..pos];
            if !junk.trim().is_empty() {
                discarded.push(junk.to_string());
            }
            body = &body[pos..];
        }
    }

    let body_valid = body.strip_prefix('$').unwrap_or(body);
    let computed = body_valid.bytes().fold(0u8, |acc, b| acc ^ b);
    if provided != computed {
        bail!(
            "checksum mismatch: provided {provided:02X} != computed {computed:02X}"
        );
    }
    Ok((provided, computed, body_valid, discarded))
}

fn parse_config(fields: &[&str]) -> Result<ConfigSentence> {
    if fields.len() < 7 {
        bail!("PNORI expects 7 fields, got {}", fields.len());
    }
    let instrument_type_raw: u8 = fields[0]
        .parse()
        .with_context(|| format!("invalid instrument type '{}'", fields[0]))?;
    let instrument_type = match instrument_type_raw {
        4 => InstrumentType::Signature,
        other => InstrumentType::Other(other),
    };
    let head_id = fields[1].to_string();
    let beams: u8 = fields[2]
        .parse()
        .with_context(|| format!("invalid beam count '{}'", fields[2]))?;
    let cells: u16 = fields[3]
        .parse()
        .with_context(|| format!("invalid cell count '{}'", fields[3]))?;
    let blanking_m: f32 = fields[4]
        .parse()
        .with_context(|| format!("invalid blanking distance '{}'", fields[4]))?;
    let cell_size_m: f32 = fields[5]
        .parse()
        .with_context(|| format!("invalid cell size '{}'", fields[5]))?;
    let coordinate_system = parse_coordinate_system(fields[6])?;
    Ok(ConfigSentence {
        instrument_type,
        head_id,
        beams,
        cells,
        blanking_m,
        cell_size_m,
        coordinate_system,
    })
}

fn parse_sensor(fields: &[&str]) -> Result<SensorSentence> {
    if fields.len() < 13 {
        bail!("PNORS expects 13 fields, got {}", fields.len());
    }
    let sent_at = parse_datetime(fields[0], fields[1])?;
    let error_code_hex = parse_hex_u32(fields[2], "error code")?;
    let status_code_hex = parse_hex_u32(fields[3], "status code")?;
    Ok(SensorSentence {
        sent_at,
        error_code_hex,
        status_code_hex,
        battery_voltage_v: parse_opt_f32(fields[4]),
        sound_speed_m_s: parse_opt_f32(fields[5]),
        heading_deg: parse_opt_f32(fields[6]),
        pitch_deg: parse_opt_f32(fields[7]),
        roll_deg: parse_opt_f32(fields[8]),
        pressure_dbar: parse_opt_f32(fields[9]),
        temperature_c: parse_opt_f32(fields[10]),
        analog_input_1: parse_opt_f32(fields[11]),
        analog_input_2: parse_opt_f32(fields[12]),
    })
}

fn parse_current(fields: &[&str]) -> Result<CurrentSentence> {
    if fields.len() < 18 {
        bail!("PNORC expects 18 fields, got {}", fields.len());
    }
    let sent_at = parse_datetime(fields[0], fields[1])?;
    let cell_number: u16 = fields[2]
        .parse()
        .with_context(|| format!("invalid cell number '{}'", fields[2]))?;
    let amplitude_unit = parse_amplitude_unit(fields[9]);
    Ok(CurrentSentence {
        sent_at,
        cell_number,
        velocity_1_m_s: parse_opt_f32(fields[3]),
        velocity_2_m_s: parse_opt_f32(fields[4]),
        velocity_3_m_s: parse_opt_f32(fields[5]),
        velocity_4_m_s: parse_opt_f32(fields[6]),
        speed_m_s: parse_opt_f32(fields[7]),
        direction_deg: parse_opt_f32(fields[8]),
        amplitude_unit,
        amplitude_beam_1: parse_opt_u8(fields[10]),
        amplitude_beam_2: parse_opt_u8(fields[11]),
        amplitude_beam_3: parse_opt_u8(fields[12]),
        amplitude_beam_4: parse_opt_u8(fields[13]),
        correlation_beam_1_pct: parse_opt_u8(fields[14]),
        correlation_beam_2_pct: parse_opt_u8(fields[15]),
        correlation_beam_3_pct: parse_opt_u8(fields[16]),
        correlation_beam_4_pct: parse_opt_u8(fields[17]),
    })
}

fn parse_datetime(date: &str, time: &str) -> Result<DateTime<Utc>> {
    let date = parse_date(date)?;
    let time = parse_time(time)?;
    let naive = NaiveDateTime::new(date, time);
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

fn parse_date(date: &str) -> Result<NaiveDate> {
    if date.len() != 6 {
        bail!("date '{date}' must be MMDDYY");
    }
    let month: u32 = date[0..2]
        .parse()
        .with_context(|| format!("invalid month in '{date}'"))?;
    let day: u32 = date[2..4]
        .parse()
        .with_context(|| format!("invalid day in '{date}'"))?;
    let year: i32 = 2000
        + date[4..6]
            .parse::<i32>()
            .with_context(|| format!("invalid year in '{date}'"))?;
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| anyhow!("invalid calendar date '{date}'"))
}

fn parse_time(time: &str) -> Result<NaiveTime> {
    if time.len() != 6 {
        bail!("time '{time}' must be hhmmss");
    }
    let hour: u32 = time[0..2]
        .parse()
        .with_context(|| format!("invalid hour in '{time}'"))?;
    let minute: u32 = time[2..4]
        .parse()
        .with_context(|| format!("invalid minute in '{time}'"))?;
    let second: u32 = time[4..6]
        .parse()
        .with_context(|| format!("invalid second in '{time}'"))?;
    NaiveTime::from_hms_opt(hour, minute, second)
        .ok_or_else(|| anyhow!("invalid clock time '{time}'"))
}

fn parse_coordinate_system(raw: &str) -> Result<CoordinateSystem> {
    let code: u8 = raw
        .parse()
        .with_context(|| format!("invalid coordinate system '{}'", raw))?;
    let system = match code {
        0 => CoordinateSystem::Enu,
        1 => CoordinateSystem::Xyz,
        2 => CoordinateSystem::Beam,
        other => CoordinateSystem::Unknown(other),
    };
    Ok(system)
}

fn parse_hex_u32(raw: &str, label: &str) -> Result<u32> {
    u32::from_str_radix(raw, 16)
        .with_context(|| format!("invalid {label} hex '{raw}'"))
}

fn parse_amplitude_unit(raw: &str) -> AmplitudeUnit {
    match raw {
        "C" | "c" => AmplitudeUnit::Counts,
        other => AmplitudeUnit::Unknown(other.to_string()),
    }
}

fn parse_opt_f32(raw: &str) -> Option<f32> {
    if is_invalid_field(raw) {
        None
    } else {
        raw.parse().ok()
    }
}

fn parse_opt_u8(raw: &str) -> Option<u8> {
    if is_invalid_field(raw) {
        None
    } else {
        raw.parse().ok()
    }
}

fn is_invalid_field(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.is_empty() || trimmed.starts_with("-9")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn parses_pnori_config() {
        let raw = "$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*41";
        let frame = Frame::from_line(raw).expect("parse config");
        match frame.payload {
            Payload::Config(cfg) => {
                assert!(frame.checksum.valid);
                assert!(matches!(cfg.instrument_type, InstrumentType::Signature));
                assert_eq!(cfg.head_id, "Signature1000_100297");
                assert_eq!(cfg.beams, 4);
                assert_eq!(cfg.cells, 21);
                assert_eq!(cfg.coordinate_system, CoordinateSystem::Enu);
            }
            _ => panic!("expected config"),
        }
    }

    #[test]
    fn parses_pnors_sensor() {
        let raw = "$PNORS,010526,220800,00000000,3ED40002,23.7,1532.0,275.4,-49.1,83.0,0.000,24.02,0,0*77";
        let frame = Frame::from_line(raw).expect("parse sensor");
        match frame.payload {
            Payload::Sensor(sensor) => {
                let expected_ts = Utc.with_ymd_and_hms(2026, 1, 5, 22, 8, 0).unwrap();
                assert_eq!(sensor.sent_at, expected_ts);
                assert_eq!(sensor.error_code_hex, 0x00000000);
                assert_eq!(sensor.status_code_hex, 0x3ED40002);
                assert_eq!(sensor.battery_voltage_v, Some(23.7));
                assert_eq!(sensor.temperature_c, Some(24.02));
            }
            _ => panic!("expected sensor"),
        }
    }

    #[test]
    fn parses_pnorc_current_with_invalid_flags() {
        let raw = "$PNORC,010526,220800,4,0.56,-0.80,-1.99,-1.33,0.98,305.2,C,80,88,67,78,13,17,10,18*26";
        let frame = Frame::from_line(raw).expect("parse current");
        match frame.payload {
            Payload::Current(cur) => {
                assert_eq!(cur.cell_number, 4);
                assert_eq!(cur.direction_deg, Some(305.2));
                assert_eq!(cur.amplitude_unit, AmplitudeUnit::Counts);
                assert_eq!(cur.amplitude_beam_1, Some(80));
                assert_eq!(cur.correlation_beam_4_pct, Some(18));
            }
            _ => panic!("expected current"),
        }
    }

    #[test]
    fn rejects_bad_checksum() {
        let raw = "$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*40"; // wrong checksum
        let err = Frame::from_line(raw).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn treats_minus_nine_variants_as_missing() {
        let raw = "$PNORC,010526,220800,1,-9.00,-9,-9.0,-9.99,-9,305.2,C,-9,-9,-9,-9,-9,-9,-9,-9*1A";
        let frame = Frame::from_line(raw).expect("parse with sentinels");
        match frame.payload {
            Payload::Current(cur) => {
                assert_eq!(cur.velocity_1_m_s, None);
                assert_eq!(cur.correlation_beam_4_pct, None);
            }
            _ => panic!("expected current"),
        }
    }

    #[test]
    fn parses_with_junk_and_records_it() {
        let raw = "prefix_junk$PNORI,4,Signature1000_100297,4,21,0.20,1.00,0*41suffix_junk";
        let frame = Frame::from_line(raw).expect("parse config with junk");
        assert_eq!(frame.discarded.len(), 2);
        assert!(frame.discarded.contains(&"prefix_junk".to_string()));
        assert!(frame.discarded.contains(&"suffix_junk".to_string()));
    }
}
