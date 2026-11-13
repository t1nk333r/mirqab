//! Numbers as people read them: bytes, rates, percentages, durations and
//! temperatures (plan 001 phase D). Every unit word comes from
//! `mirqab_proto::strings`, so a translation touches no code here.

use mirqab_proto::strings as s;

/// The unit a temperature is shown in (`units.temperature` in config.json).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    /// `"c"` or `"f"`, as config.json stores it; anything else is Celsius.
    pub fn from_config(v: &str) -> TempUnit {
        match v {
            "f" => TempUnit::Fahrenheit,
            _ => TempUnit::Celsius,
        }
    }
}

/// A byte count in SI units, three significant figures at most:
/// `512 B`, `1.5 kB`, `14 MB`, `1.2 GB`.
pub fn bytes(n: u64) -> String {
    if n < 1000 {
        return format!("{n} {}", s::BYTE_UNITS[0]);
    }
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < s::BYTE_UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    // 999.96 kB would print as "1000 kB": promote instead.
    if round_for_display(v) >= 1000.0 && unit < s::BYTE_UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    format!("{} {}", short_number(v), s::BYTE_UNITS[unit])
}

/// Bytes per second: `0 B/s`, `1.2 MB/s`. Negative or not a number is no
/// value.
pub fn rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec < 0.0 {
        return s::NO_VALUE.to_string();
    }
    format!("{}{}", bytes(bytes_per_sec.round() as u64), s::PER_SECOND)
}

/// A percentage: one decimal under 10 (`0.2%`), none above (`37%`).
/// Negative values show as 0; not a number is no value.
pub fn percent(p: f64) -> String {
    if !p.is_finite() {
        return s::NO_VALUE.to_string();
    }
    format!("{}{}", short_number(p.max(0.0)), s::PERCENT)
}

/// Whole units, the two largest: `45s`, `12m`, `5h 12m`, `3d 4h`.
pub fn duration(secs: u64) -> String {
    s::duration_parts(
        secs / 86_400,
        secs % 86_400 / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

/// A temperature given in °C, rounded to a whole degree in `unit`.
pub fn temperature(celsius: f64, unit: TempUnit) -> String {
    if !celsius.is_finite() {
        return s::NO_VALUE.to_string();
    }
    match unit {
        TempUnit::Celsius => format!("{} {}", celsius.round() as i64, s::DEGREES_C),
        TempUnit::Fahrenheit => format!(
            "{} {}",
            (celsius * 9.0 / 5.0 + 32.0).round() as i64,
            s::DEGREES_F
        ),
    }
}

/// One decimal under 10, none from 10 up. Rounding that crosses 10 drops
/// the decimal (`9.96` is `10`, not `10.0`).
fn short_number(v: f64) -> String {
    let r = round_for_display(v);
    if r < 10.0 {
        format!("{r:.1}")
    } else {
        format!("{r:.0}")
    }
}

fn round_for_display(v: f64) -> f64 {
    let one = (v * 10.0).round() / 10.0;
    if one < 10.0 { one } else { v.round() }
}
