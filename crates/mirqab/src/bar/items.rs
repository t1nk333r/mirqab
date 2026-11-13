//! The bar items (plan 004): one function per item from a `system` snapshot
//! to its text, tooltip, class and percentage. Text never carries colour or
//! markup; the class carries state and the bar's CSS colours it.

use mirqab_core::proto::stream::SystemData;
use mirqab_core::proto::strings;
use mirqab_view::format::{self, TempUnit};

/// One reading of an item.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub text: String,
    pub tooltip: String,
    pub class: &'static str,
    pub percentage: Option<u32>,
}

/// The items this plan serves, all from the `system` stream.
pub fn known(item: &str) -> bool {
    strings::BAR_ITEMS.iter().any(|(name, _)| *name == item)
}

/// What `combo` shows when the config names nothing usable.
pub const COMBO_DEFAULT: &[&str] = &["cpu", "mem", "temp"];

/// `normal`, then `high` at 80 % and `critical` at 95 %.
fn level(p: f64) -> &'static str {
    if p >= 95.0 {
        "critical"
    } else if p >= 80.0 {
        "high"
    } else {
        "normal"
    }
}

/// How bad a class is, for `combo`'s worst-of.
fn severity(class: &str) -> u8 {
    match class {
        "critical" => 2,
        "high" | "hot" => 1,
        _ => 0,
    }
}

fn none(tooltip: String) -> Reading {
    Reading {
        text: strings::NO_VALUE.to_string(),
        tooltip,
        class: "unknown",
        percentage: None,
    }
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| strings::NO_VALUE.to_string(), format::percent)
}

fn percent_reading(p: Option<f64>, tooltip: String) -> Reading {
    match p.filter(|p| p.is_finite()) {
        Some(p) => Reading {
            text: strings::bar_percent(p),
            tooltip,
            class: level(p),
            percentage: Some(p.round().clamp(0.0, 100.0) as u32),
        },
        None => none(tooltip),
    }
}

fn cpu(d: &SystemData) -> Reading {
    let Some(c) = &d.cpu else {
        return none(String::new());
    };
    let tip = strings::bar_tip_cpu(
        &pct(c.percent),
        &pct(c.user),
        &pct(c.system),
        &pct(c.iowait),
    );
    percent_reading(c.percent, tip)
}

fn mem(d: &SystemData) -> Reading {
    let Some(m) = d.mem.as_ref().filter(|m| m.total_bytes > 0) else {
        return none(String::new());
    };
    let tip = strings::bar_tip_mem(
        &format::bytes(m.used_bytes),
        &format::bytes(m.total_bytes),
        &format::bytes(m.available_bytes),
    );
    percent_reading(
        Some(m.used_bytes as f64 * 100.0 / m.total_bytes as f64),
        tip,
    )
}

fn temp(d: &SystemData) -> Reading {
    let Some(t) = d
        .temps
        .iter()
        .filter(|t| t.kind == "cpu")
        .max_by(|a, b| a.celsius.total_cmp(&b.celsius))
    else {
        return none(String::new());
    };
    let (state, throttling) = d
        .thermal
        .as_ref()
        .map_or(("unknown", false), |th| (th.state.as_str(), th.throttling));
    let class = match state {
        "cool" => "cool",
        "warm" => "warm",
        "hot" => "hot",
        "critical" => "critical",
        _ => "unknown",
    };
    Reading {
        text: strings::bar_degrees(t.celsius),
        tooltip: strings::bar_tip_temp(
            &t.label,
            &format::temperature(t.celsius, TempUnit::Celsius),
            &strings::thermal_word(state, throttling),
        ),
        class,
        percentage: t
            .critical
            .filter(|c| *c > 0.0)
            .map(|c| (t.celsius * 100.0 / c).round().clamp(0.0, 100.0) as u32),
    }
}

fn gpu(d: &SystemData) -> Reading {
    if d.gpus.is_empty() {
        return none(String::new());
    }
    let tip: Vec<String> = d
        .gpus
        .iter()
        .map(|g| {
            let busy = if g.suspended {
                strings::SYSTEM_SUSPENDED.to_string()
            } else {
                pct(g.busy_percent)
            };
            strings::bar_tip_gpu(&g.name, &busy)
        })
        .collect();
    let busiest = d
        .gpus
        .iter()
        .filter(|g| !g.suspended)
        .filter_map(|g| g.busy_percent)
        .max_by(f64::total_cmp);
    percent_reading(busiest, tip.join("\n"))
}

fn load(d: &SystemData) -> Reading {
    match &d.load {
        Some(l) => Reading {
            text: strings::bar_load(l.one),
            tooltip: strings::bar_tip_load(l.one, l.five, l.fifteen),
            class: "normal",
            percentage: None,
        },
        None => none(String::new()),
    }
}

/// The reading of `item`; `combo` lists `combo`'s parts (unknown names and
/// `combo` itself are skipped). None for an unknown item.
pub fn read(item: &str, d: &SystemData, combo: &[String]) -> Option<Reading> {
    Some(match item {
        "cpu" => cpu(d),
        "mem" => mem(d),
        "temp" => temp(d),
        "gpu" => gpu(d),
        "load" => load(d),
        "combo" => {
            let mut parts: Vec<Reading> = combo
                .iter()
                .filter(|p| p.as_str() != "combo")
                .filter_map(|p| read(p, d, &[]))
                .collect();
            if parts.is_empty() {
                parts = COMBO_DEFAULT
                    .iter()
                    .filter_map(|p| read(p, d, &[]))
                    .collect();
            }
            let worst = parts.iter().map(|r| severity(r.class)).max().unwrap_or(0);
            Reading {
                text: parts
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                tooltip: parts
                    .iter()
                    .map(|r| r.tooltip.as_str())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                class: ["normal", "high", "critical"][usize::from(worst)],
                percentage: None,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mirqab_core::proto::stream::{Cpu, Gpu, Load, Mem, Temp, Thermal};

    fn data(cpu: f64, used: u64) -> SystemData {
        SystemData {
            cpu: Some(Cpu {
                percent: Some(cpu),
                ..Cpu::default()
            }),
            mem: Some(Mem {
                total_bytes: 100,
                used_bytes: used,
                ..Mem::default()
            }),
            ..SystemData::default()
        }
    }

    #[test]
    fn percents_round_and_class_at_80_and_95() {
        let r = read("cpu", &data(12.4, 0), &[]).unwrap();
        assert_eq!(
            (r.text.as_str(), r.class, r.percentage),
            ("12%", "normal", Some(12))
        );
        assert_eq!(read("cpu", &data(80.0, 0), &[]).unwrap().class, "high");
        assert_eq!(read("cpu", &data(95.0, 0), &[]).unwrap().class, "critical");
        let m = read("mem", &data(0.0, 41), &[]).unwrap();
        assert_eq!((m.text.as_str(), m.percentage), ("41%", Some(41)));
    }

    #[test]
    fn a_missing_value_is_a_dash_classed_unknown() {
        let mut d = data(0.0, 0);
        d.cpu.as_mut().unwrap().percent = None;
        let r = read("cpu", &d, &[]).unwrap();
        assert_eq!((r.text.as_str(), r.class), (strings::NO_VALUE, "unknown"));
        assert_eq!(read("temp", &d, &[]).unwrap().class, "unknown");
        assert!(read("nope", &d, &[]).is_none());
    }

    #[test]
    fn temp_gpu_and_load() {
        let d = SystemData {
            temps: vec![
                Temp {
                    id: "k10temp/temp1".into(),
                    label: "Tctl".into(),
                    celsius: 52.4,
                    critical: Some(100.0),
                    kind: "cpu".into(),
                },
                Temp {
                    id: "nvme/temp1".into(),
                    label: "Composite".into(),
                    celsius: 70.0,
                    critical: None,
                    kind: "disk".into(),
                },
            ],
            thermal: Some(Thermal {
                state: "hot".into(),
                ..Thermal::default()
            }),
            gpus: vec![
                Gpu {
                    name: "AMD GPU (card0)".into(),
                    busy_percent: Some(3.0),
                    ..Gpu::default()
                },
                Gpu {
                    name: "AMD GPU (card1)".into(),
                    suspended: true,
                    ..Gpu::default()
                },
            ],
            load: Some(Load {
                one: 1.234,
                ..Load::default()
            }),
            ..SystemData::default()
        };
        let t = read("temp", &d, &[]).unwrap();
        assert_eq!(
            (t.text.as_str(), t.class, t.percentage),
            ("52\u{b0}", "hot", Some(52))
        );
        assert!(t.tooltip.starts_with("Tctl"), "{}", t.tooltip);
        let g = read("gpu", &d, &[]).unwrap();
        assert_eq!(g.text, "3%");
        assert_eq!(
            g.tooltip.lines().count(),
            2,
            "every GPU, the suspended one too"
        );
        assert_eq!(read("load", &d, &[]).unwrap().text, "1.2");
    }

    #[test]
    fn combo_joins_its_parts_and_takes_the_worst_class() {
        let d = data(96.0, 41);
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let r = read("combo", &d, &v(&["cpu", "mem"])).unwrap();
        assert_eq!((r.text.as_str(), r.class), ("96% 41%", "critical"));
        // Unknown parts and combo itself are skipped; nothing usable: the
        // default parts.
        let r = read("combo", &d, &v(&["combo", "nope"])).unwrap();
        assert_eq!(r.text, format!("96% 41% {}", strings::NO_VALUE));
    }
}
