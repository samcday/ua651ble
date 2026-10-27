use std::fmt;

use anyhow::{Result, ensure};
use chrono::{NaiveDate, Utc};
use serde::Serialize;

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub struct Reading {
    pub address: String,
    pub received_at: String,
    /// Local cuff time, without an assumed time zone. Null if unknown.
    pub measured_at: Option<String>,
    pub unit: &'static str,
    pub systolic: Option<f64>,
    pub diastolic: Option<f64>,
    pub map: Option<f64>,
    pub pulse: Option<f64>,
    pub user_id: Option<u8>,
    pub status: Option<Status>,
    /// Original 2A35 payload, including special SFLOAT encodings.
    pub raw: String,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub raw: u16,
    pub body_movement: bool,
    pub cuff_too_loose: bool,
    pub irregular_pulse: bool,
    pub pulse_range: &'static str,
    pub improper_position: bool,
}

// IEEE 11073 SFLOAT: signed 12-bit mantissa and signed 4-bit decimal exponent.
// Special values are null in JSON, never fabricated zeroes or JSON NaN.
fn sfloat(word: u16) -> Option<f64> {
    if matches!(word, 0x07fe..=0x0802) {
        return None;
    }
    let mantissa = word & 0x0fff;
    let mantissa = ((mantissa << 4) as i16) >> 4;
    let exponent = (word as i16) >> 12;
    // Parse the decimal value directly to avoid multiplication artifacts in JSON.
    Some(
        format!("{mantissa}e{exponent}")
            .parse()
            .expect("SFLOAT components are bounded integers"),
    )
}

pub fn decode(address: &str, data: &[u8]) -> Result<Reading> {
    ensure!(!data.is_empty(), "Empty blood pressure packet");
    let flags = data[0] & 0x1f;
    let expected = 7
        + if flags & 2 != 0 { 7 } else { 0 }
        + if flags & 4 != 0 { 2 } else { 0 }
        + usize::from(flags & 8 != 0)
        + if flags & 16 != 0 { 2 } else { 0 };
    ensure!(
        data.len() >= expected,
        "Invalid blood pressure packet length: expected at least {expected}, got {}",
        data.len()
    );
    let word = |i| u16::from_le_bytes([data[i], data[i + 1]]);
    let mut i = 7;
    let measured_at = if flags & 2 != 0 {
        let year = word(i);
        let (month, day, hour, minute, second) = (
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
        );
        i += 7;
        ensure!(hour < 24 && minute < 60 && second < 60, "Invalid cuff time");
        if year == 0 || month == 0 || day == 0 {
            None
        } else {
            ensure!((1582..=9999).contains(&year), "Invalid cuff year");
            let date = NaiveDate::from_ymd_opt(year.into(), month.into(), day.into())
                .and_then(|d| d.and_hms_opt(hour.into(), minute.into(), second.into()));
            ensure!(date.is_some(), "Invalid cuff timestamp");
            date.map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
        }
    } else {
        None
    };
    let pulse = if flags & 4 != 0 {
        let value = sfloat(word(i));
        i += 2;
        value
    } else {
        None
    };
    let user_id = if flags & 8 != 0 {
        let value = data[i];
        i += 1;
        Some(value)
    } else {
        None
    };
    let status = (flags & 16 != 0).then(|| {
        let raw = word(i);
        Status {
            raw,
            body_movement: raw & 1 != 0,
            cuff_too_loose: raw & 2 != 0,
            irregular_pulse: raw & 4 != 0,
            pulse_range: ["within_range", "above_limit", "below_limit", "reserved"]
                [usize::from((raw >> 3) & 3)],
            improper_position: raw & 32 != 0,
        }
    });
    Ok(Reading {
        address: address.into(),
        received_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        measured_at,
        unit: if flags & 1 == 0 { "mmHg" } else { "kPa" },
        systolic: sfloat(word(1)),
        diastolic: sfloat(word(3)),
        map: sfloat(word(5)),
        pulse,
        user_id,
        status,
        raw: data
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    })
}

impl fmt::Display for Reading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [systolic, diastolic, map, pulse] =
            [self.systolic, self.diastolic, self.map, self.pulse]
                .map(|n| n.map_or_else(|| "unavailable".into(), |n| n.to_string()));
        let unit = self.unit;
        let measured = self.measured_at.as_deref().unwrap_or("not reported");
        write!(
            f,
            "Blood pressure: {systolic}/{diastolic} {unit}\n\
             Pulse: {pulse} bpm\n\
             Mean arterial pressure: {map} {unit}\n\
             Measured: {measured}\n"
        )?;
        let Some(s) = &self.status else {
            return write!(f, "Status: not reported");
        };
        let mut flags: Vec<_> = [
            (s.body_movement, "body movement"),
            (s.cuff_too_loose, "loose cuff"),
            (s.irregular_pulse, "irregular pulse"),
            (s.improper_position, "improper position"),
            (s.pulse_range != "within_range", s.pulse_range),
            (s.raw & !0x3f != 0, "reserved status bits set"),
        ]
        .into_iter()
        .filter_map(|(set, label)| set.then_some(label))
        .collect();
        if flags.is_empty() {
            flags.push("none");
        }
        write!(f, "Status: {}", flags.join(", "))
    }
}
