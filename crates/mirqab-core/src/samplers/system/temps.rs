//! Temperatures from hwmon (and thermal zones where hwmon has no CPU
//! sensor), the thermal word, and throttling (plan 002).

use crate::proto::stream::{Temp, Thermal};
use crate::proto::strings;
use crate::root::Root;

/// Thresholds of the thermal word, as shares of a sensor's critical
/// temperature. Below 60 % a chip is cool (idle desktops sit at 35–50 % of a
/// 95 °C limit); 80 % is sustained load; past 95 % it is about to throttle.
pub const WARM: f64 = 0.60;
pub const HOT: f64 = 0.80;
pub const CRITICAL: f64 = 0.95;
/// A critical temperature for sensors that report none.
pub const DEFAULT_CRITICAL: f64 = 95.0;

/// The kind of sensor a hwmon driver name means.
pub fn kind(name: &str) -> &'static str {
    match name {
        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" => "cpu",
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => "gpu",
        "nvme" | "drivetemp" => "disk",
        "spd5118" | "jc42" => "memory",
        _ => "other",
    }
}

fn read_milli(root: &Root, rel: &str) -> Option<f64> {
    let v: i64 = root.read(rel).ok()?.trim().parse().ok()?;
    Some(v as f64 / 1000.0)
}

/// Every hwmon temperature, in hwmon order. A driver name seen twice
/// (two NVMe drives) gets `.1`, `.2` in its id so ids stay unique.
pub fn read(root: &Root) -> Vec<Temp> {
    let mut out = Vec::new();
    let Ok(mut dirs) = root.read_dir("sys/class/hwmon") else {
        return thermal_zones(root);
    };
    dirs.sort_by_key(|d| {
        d.trim_start_matches("hwmon")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    let mut seen: std::collections::BTreeMap<String, u32> = Default::default();
    for dir in dirs {
        let base = format!("sys/class/hwmon/{dir}");
        let Ok(name) = root.read(&format!("{base}/name")) else {
            continue;
        };
        let name = name.trim().to_string();
        let Ok(files) = root.read_dir(&base) else {
            continue;
        };
        let mut inputs: Vec<u32> = files
            .iter()
            .filter_map(|f| f.strip_prefix("temp")?.strip_suffix("_input")?.parse().ok())
            .collect();
        if inputs.is_empty() {
            continue;
        }
        inputs.sort_unstable();
        let n = seen.entry(name.clone()).or_insert(0);
        let tag = if *n == 0 {
            name.clone()
        } else {
            format!("{name}.{n}")
        };
        *n += 1;
        let k = kind(&name);
        for (i, t) in inputs.iter().enumerate() {
            let Some(celsius) = read_milli(root, &format!("{base}/temp{t}_input")) else {
                continue;
            };
            let label = root
                .read(&format!("{base}/temp{t}_label"))
                .ok()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| strings::temp_label(k, i + 1));
            out.push(Temp {
                id: format!("{tag}/temp{t}"),
                label,
                celsius,
                critical: read_milli(root, &format!("{base}/temp{t}_crit")),
                kind: k.to_string(),
            });
        }
    }
    if !out.iter().any(|t| t.kind == "cpu") {
        out.extend(thermal_zones(root));
    }
    out
}

/// `sys/class/thermal/thermal_zoneN`: a CPU zone (`x86_pkg_temp`,
/// `cpu-thermal`, `cpu_thermal`, `soc_thermal`) where hwmon has none.
fn thermal_zones(root: &Root) -> Vec<Temp> {
    let mut out = Vec::new();
    let Ok(zones) = root.read_dir("sys/class/thermal") else {
        return out;
    };
    for z in zones.iter().filter(|z| z.starts_with("thermal_zone")) {
        let base = format!("sys/class/thermal/{z}");
        let Ok(ty) = root.read(&format!("{base}/type")) else {
            continue;
        };
        let ty = ty.trim();
        if !matches!(
            ty,
            "x86_pkg_temp" | "cpu-thermal" | "cpu_thermal" | "soc_thermal"
        ) {
            continue;
        }
        if let Some(celsius) = read_milli(root, &format!("{base}/temp")) {
            out.push(Temp {
                id: format!("{z}/{ty}"),
                label: strings::temp_label("cpu", out.len() + 1),
                celsius,
                critical: None,
                kind: "cpu".to_string(),
            });
        }
    }
    out
}

/// The thermal word from the hottest CPU or GPU sensor, measured against
/// its own critical temperature (plan 002).
pub fn thermal(temps: &[Temp], throttling: bool) -> Option<Thermal> {
    let ratio = |t: &Temp| t.celsius / t.critical.filter(|c| *c > 0.0).unwrap_or(DEFAULT_CRITICAL);
    let hottest = temps
        .iter()
        .filter(|t| t.kind == "cpu" || t.kind == "gpu")
        .max_by(|a, b| ratio(a).total_cmp(&ratio(b)))?;
    let r = ratio(hottest);
    let state = if r < WARM {
        "cool"
    } else if r < HOT {
        "warm"
    } else if r < CRITICAL {
        "hot"
    } else {
        "critical"
    };
    Some(Thermal {
        state: state.to_string(),
        hottest: Some(hottest.id.clone()),
        throttling,
    })
}

/// Intel's per-core throttle counters, summed; None where absent (AMD).
pub fn throttle_count(root: &Root, ids: &[u32]) -> Option<u64> {
    let mut any = false;
    let mut sum = 0;
    for n in ids {
        if let Some(v) = root
            .read(&format!(
                "sys/devices/system/cpu/cpu{n}/thermal_throttle/core_throttle_count"
            ))
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
        {
            any = true;
            sum += v;
        }
    }
    any.then_some(sum)
}

/// Where the kernel exposes no counter (AMD): the clock held below 60 % of
/// its maximum while the hottest CPU sensor is within 5 °C of critical.
pub fn throttling_by_clock(temps: &[Temp], mhz_avg: Option<u32>, mhz_max: Option<u32>) -> bool {
    let (Some(avg), Some(max)) = (mhz_avg, mhz_max) else {
        return false;
    };
    let hot = temps
        .iter()
        .filter(|t| t.kind == "cpu")
        .any(|t| t.celsius >= t.critical.filter(|c| *c > 0.0).unwrap_or(DEFAULT_CRITICAL) - 5.0);
    hot && f64::from(avg) < f64::from(max) * 0.6
}
