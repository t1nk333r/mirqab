//! Power (plan 002): package watts from RAPL's energy counter, readable only
//! by root unless the helper is enabled (plan 015), and battery watts from
//! `power_supply`.

use crate::root::Root;

/// RAPL's package energy counter and its wrap point, in µJ.
pub fn energy(root: &Root) -> Option<(u64, u64)> {
    let base = "sys/class/powercap/intel-rapl:0";
    let e: u64 = root
        .read(&format!("{base}/energy_uj"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let max: u64 = root
        .read(&format!("{base}/max_energy_range_uj"))
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(u64::MAX);
    Some((e, max))
}

/// Watts between two counter readings `secs` apart; the counter wraps at
/// `max`.
pub fn watts(prev: u64, now: u64, max: u64, secs: f64) -> Option<f64> {
    if secs <= 0.0 {
        return None;
    }
    let delta = if now >= prev {
        now - prev
    } else {
        max.checked_sub(prev)?.checked_add(now)?
    };
    Some(delta as f64 / 1e6 / secs)
}

/// The draw of every battery that is discharging, in watts: `power_now`
/// (µW), or `current_now` (µA) × `voltage_now` (µV). None with no battery
/// or none discharging.
pub fn battery(root: &Root) -> Option<f64> {
    let mut total: Option<f64> = None;
    for dev in root.read_dir("sys/class/power_supply").ok()? {
        let base = format!("sys/class/power_supply/{dev}");
        let read = |f: &str| {
            root.read(&format!("{base}/{f}"))
                .ok()
                .map(|s| s.trim().to_string())
        };
        if read("type").as_deref() != Some("Battery")
            || read("status").as_deref() != Some("Discharging")
        {
            continue;
        }
        let num = |f: &str| read(f).and_then(|v| v.parse::<f64>().ok());
        let w = num("power_now")
            .map(|uw| uw / 1e6)
            .or_else(|| Some(num("current_now")? * num("voltage_now")? / 1e12));
        if let Some(w) = w {
            *total.get_or_insert(0.0) += w;
        }
    }
    total
}
