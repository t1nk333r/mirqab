//! Power (plan 002): package watts from RAPL's energy counter, readable only
//! by root unless the helper is enabled (plan 015), and battery watts from
//! `power_supply`.

use crate::root::Root;

const RAPL: &str = "sys/class/powercap/intel-rapl:0";
/// No package draws more; a larger value is a counter reset (review of
/// PR #7: a reset read as 62 kW and went into history).
const MAX_WATTS: f64 = 2000.0;

/// Whether RAPL is there but unreadable: it needs root, which the helper
/// gives (plan 015). False where there is no RAPL at all.
pub fn needs_helper(root: &Root) -> bool {
    root.exists(&format!("{RAPL}/energy_uj")) && energy(root).is_none()
}

/// RAPL's package energy counter and its wrap point, in µJ; the wrap point
/// is `u64::MAX` when unknown.
pub fn energy(root: &Root) -> Option<(u64, u64)> {
    let base = RAPL;
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
/// `max`. None for a decrease with no known wrap point, and for a value no
/// package draws.
pub fn watts(prev: u64, now: u64, max: u64, secs: f64) -> Option<f64> {
    if secs <= 0.0 {
        return None;
    }
    let delta = if now >= prev {
        now - prev
    } else if max == u64::MAX {
        return None;
    } else {
        max.checked_sub(prev)?.checked_add(now)?
    };
    Some(delta as f64 / 1e6 / secs).filter(|w| *w <= MAX_WATTS)
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
        // Some drivers sign the discharge current.
        if let Some(w) = w.map(f64::abs) {
            *total.get_or_insert(0.0) += w;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::watts;

    #[test]
    fn rapl_wraps_resets_and_impossible_values() {
        assert_eq!(watts(1_000_000, 61_000_000, u64::MAX, 1.0), Some(60.0));
        // Wrapped at a known point: 1 J before and 1 J after.
        assert_eq!(watts(9_000_000, 1_000_000, 10_000_000, 2.0), Some(1.0));
        assert_eq!(watts(1000, 500, u64::MAX, 1.0), None, "no wrap point");
        assert_eq!(
            watts(200_000_000_000, 5, 262_143_328_850, 1.0),
            None,
            "a reset, not 62 kW"
        );
        assert_eq!(watts(0, 1, 10, 0.0), None);
    }
}
