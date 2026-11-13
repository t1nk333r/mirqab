//! The System tab's view model (plan 002): a `system` event in, the text,
//! fractions and groupings of each card out. The widgets in
//! `crates/mirqab-ui/src/tabs/system/` only lay these out.

use crate::format::{self, TempUnit};
use crate::graph::Scale;
use mirqab_proto::stream::{Gpu, SystemData};
use mirqab_proto::strings as s;
use serde_json::Value;

/// A status colour; the widget maps it to a style class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    pub thermal: Option<(String, Tone)>,
    pub uptime: Option<String>,
    pub load: Option<String>,
    pub advice: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreCell {
    pub label: String,
    /// Busy share from 0 to 1, None before the second sample.
    pub fraction: Option<f64>,
    pub text: String,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuCard {
    pub percent: String,
    /// `user 8.1%`, `system 3.9%`, `iowait 0.5%`.
    pub legend: Vec<String>,
    pub clock: Option<String>,
    pub governor: Option<String>,
    /// Groups of cores: one group, or performance then efficiency.
    pub core_groups: Vec<Vec<CoreCell>>,
}

/// Used, cache and free as shares of the total; they sum to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemSplit {
    pub used: f64,
    pub cache: f64,
    pub free: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryCard {
    pub used_of_total: String,
    pub split: MemSplit,
    pub legend: Vec<String>,
    pub swap: Option<String>,
    pub zram: Option<String>,
    pub paging: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PressureRow {
    pub name: &'static str,
    pub series: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpuCard {
    pub id: String,
    pub name: String,
    pub suspended: bool,
    pub busy: Option<String>,
    /// `1.1 GB of 17 GB` and its share.
    pub vram: Option<(String, f64)>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TempRow {
    pub label: String,
    pub text: String,
    /// Against the sensor's critical temperature (95 °C when it has none).
    pub fraction: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PowerCard {
    pub package: Option<String>,
    pub battery: Option<String>,
    /// RAPL is there but needs root (plan 015's helper), as the daemon
    /// says; not merely a missing first reading (review of PR #8).
    pub needs_helper: bool,
    /// The machine has a package counter, so the graph has something to
    /// draw once readable.
    pub rapl: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemView {
    pub overview: Overview,
    pub cpu: Option<CpuCard>,
    pub memory: Option<MemoryCard>,
    pub pressure: Vec<PressureRow>,
    pub gpus: Vec<GpuCard>,
    /// Groups by kind, in the order CPU, GPU, disk, memory, other.
    pub temps: Vec<(String, Vec<TempRow>)>,
    pub power: PowerCard,
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| s::NO_VALUE.to_string(), format::percent)
}

pub fn thermal_tone(state: &str, throttling: bool) -> Tone {
    match (state, throttling) {
        (_, true) | ("critical", _) => Tone::Error,
        ("hot", _) => Tone::Warning,
        ("warm", _) => Tone::Neutral,
        ("cool", _) => Tone::Success,
        // A state this version does not know is not good news.
        _ => Tone::Neutral,
    }
}

pub fn mem_split(total: u64, used: u64, cache: u64) -> MemSplit {
    if total == 0 {
        return MemSplit {
            used: 0.0,
            cache: 0.0,
            free: 1.0,
        };
    }
    let t = total as f64;
    let used = (used as f64 / t).clamp(0.0, 1.0);
    let cache = (cache as f64 / t).clamp(0.0, 1.0 - used);
    MemSplit {
        used,
        cache,
        free: 1.0 - used - cache,
    }
}

fn gpu_card(g: &Gpu, unit: TempUnit) -> GpuCard {
    let mut details = Vec::new();
    details.extend(g.celsius.map(|c| format::temperature(c, unit)));
    details.extend(g.watts.map(|w| format!("{w:.1} {}", s::WATTS)));
    details.extend(g.mhz.map(s::mhz));
    GpuCard {
        id: g.id.clone(),
        name: g.name.clone(),
        suspended: g.suspended,
        busy: g.busy_percent.map(format::percent),
        vram: match (g.vram_used_bytes, g.vram_total_bytes) {
            (Some(u), Some(t)) if t > 0 => Some((
                s::system_used_of(&format::bytes(u), &format::bytes(t)),
                (u as f64 / t as f64).clamp(0.0, 1.0),
            )),
            _ => None,
        },
        details,
    }
}

pub fn view(d: &SystemData, unit: TempUnit) -> SystemView {
    let overview = Overview {
        thermal: d.thermal.as_ref().map(|t| {
            (
                s::thermal_word(&t.state, t.throttling),
                thermal_tone(&t.state, t.throttling),
            )
        }),
        uptime: d.uptime_sec.map(|u| s::system_uptime(&format::duration(u))),
        load: d
            .load
            .as_ref()
            .map(|l| s::system_load(l.one, l.five, l.fifteen)),
        advice: d
            .advice
            .first()
            .map(|a| s::system_advice(&a.reason).to_string()),
    };
    let cpu = d.cpu.as_ref().map(|c| {
        let cell = |core: &mirqab_proto::stream::Core| CoreCell {
            label: s::core_label(core.id),
            fraction: core.percent.map(|p| (p / 100.0).clamp(0.0, 1.0)),
            text: pct(core.percent),
            kind: core.kind.clone(),
        };
        let p: Vec<CoreCell> = c
            .cores
            .iter()
            .filter(|k| k.kind.as_deref() == Some("p"))
            .map(cell)
            .collect();
        let e: Vec<CoreCell> = c
            .cores
            .iter()
            .filter(|k| k.kind.as_deref() == Some("e"))
            .map(cell)
            .collect();
        let core_groups = if !p.is_empty() && !e.is_empty() {
            vec![p, e]
        } else if c.cores.is_empty() {
            Vec::new()
        } else {
            vec![c.cores.iter().map(cell).collect()]
        };
        CpuCard {
            percent: pct(c.percent),
            legend: vec![
                format!("{} {}", s::SYSTEM_USER, pct(c.user)),
                format!("{} {}", s::SYSTEM_SYS, pct(c.system)),
                format!("{} {}", s::SYSTEM_IOWAIT, pct(c.iowait)),
            ],
            clock: c.mhz_avg.map(|a| {
                s::system_clock(
                    f64::from(a) / 1000.0,
                    c.mhz_max.map(|m| f64::from(m) / 1000.0),
                )
            }),
            governor: c.governor.clone(),
            core_groups,
        }
    });
    let memory = d.mem.as_ref().map(|m| {
        let split = mem_split(m.total_bytes, m.used_bytes, m.cache_bytes);
        let free = m.total_bytes.saturating_sub(m.used_bytes + m.cache_bytes);
        MemoryCard {
            used_of_total: s::system_used_of(
                &format::bytes(m.used_bytes),
                &format::bytes(m.total_bytes),
            ),
            split,
            legend: vec![
                format!("{} {}", s::SYSTEM_USED, format::bytes(m.used_bytes)),
                format!("{} {}", s::SYSTEM_CACHE, format::bytes(m.cache_bytes)),
                format!("{} {}", s::SYSTEM_FREE, format::bytes(free)),
            ],
            swap: (m.swap_total_bytes > 0).then(|| {
                s::system_swap(
                    &format::bytes(m.swap_used_bytes),
                    &format::bytes(m.swap_total_bytes),
                )
            }),
            zram: m.zram.as_ref().filter(|z| z.orig_bytes > 0).map(|z| {
                s::system_zram(&format::bytes(z.orig_bytes), &format::bytes(z.compr_bytes))
            }),
            paging: match (m.page_in_per_sec, m.page_out_per_sec) {
                // vmstat counts KiB.
                (Some(i), Some(o)) => Some(s::system_paging(
                    &format::rate(i * 1024.0),
                    &format::rate(o * 1024.0),
                )),
                _ => None,
            },
        }
    });
    let mut pressure = Vec::new();
    if let Some(p) = &d.pressure {
        for (name, series, stall) in [
            (s::SYSTEM_CPU, "psi.cpu", &p.cpu),
            (s::SYSTEM_MEMORY, "psi.memory", &p.memory),
            (s::SYSTEM_IO, "psi.io", &p.io),
        ] {
            if let Some(st) = stall {
                pressure.push(PressureRow {
                    name,
                    series,
                    text: format::percent(st.some60),
                });
            }
        }
    }
    let mut temps: Vec<(String, Vec<TempRow>)> = Vec::new();
    for kind in ["cpu", "gpu", "disk", "memory", "other"] {
        let rows: Vec<TempRow> = d
            .temps
            .iter()
            .filter(|t| t.kind == kind)
            .map(|t| TempRow {
                label: t.label.clone(),
                text: format::temperature(t.celsius, unit),
                fraction: (t.celsius / t.critical.filter(|c| *c > 0.0).unwrap_or(95.0))
                    .clamp(0.0, 1.0),
            })
            .collect();
        if !rows.is_empty() {
            temps.push((s::temp_kind(kind).to_string(), rows));
        }
    }
    let power = PowerCard {
        package: d
            .power
            .as_ref()
            .and_then(|p| p.package_watts)
            .map(|w| format!("{w:.1} {}", s::WATTS)),
        battery: d
            .power
            .as_ref()
            .and_then(|p| p.battery_watts)
            .map(|w| format!("{w:.1} {}", s::WATTS)),
        needs_helper: d.power.as_ref().is_some_and(|p| p.needs_helper),
        rapl: d
            .power
            .as_ref()
            .is_some_and(|p| p.source == "rapl" || p.source == "both"),
    };
    SystemView {
        overview,
        cpu,
        memory,
        pressure,
        // History keeps two GPUs (`gpu_graph`); a third card would never
        // be built, and the page would rebuild its cards on every event
        // (review of PR #8).
        gpus: d.gpus.iter().take(2).map(|g| gpu_card(g, unit)).collect(),
        temps,
        power,
    }
}

/// A graph on the page: the history series it shows, how to read its live
/// value from an event, and its scale.
#[derive(Debug, Clone, Copy)]
pub struct GraphSpec {
    pub series: &'static str,
    pub live: fn(&Value) -> Option<f64>,
    pub scale: Scale,
}

const PERCENT: Scale = Scale::Fixed {
    min: 0.0,
    max: 100.0,
};

pub const CPU_GRAPH: GraphSpec = GraphSpec {
    series: "cpu.total",
    live: |v| v["cpu"]["percent"].as_f64(),
    scale: PERCENT,
};

pub const MEMORY_GRAPH: GraphSpec = GraphSpec {
    series: "mem.used",
    live: |v| v["mem"]["usedBytes"].as_f64(),
    scale: Scale::Auto { floor: 1.0 },
};

pub const POWER_GRAPH: GraphSpec = GraphSpec {
    series: "power.package",
    live: |v| v["power"]["packageWatts"].as_f64(),
    scale: Scale::Auto { floor: 1.0 },
};

pub fn pressure_graph(series: &'static str) -> GraphSpec {
    GraphSpec {
        series,
        live: match series {
            // some60, as the card's number and history store (review of
            // PR #8: the live line was some10).
            "psi.cpu" => |v| v["pressure"]["cpu"]["some60"].as_f64(),
            "psi.memory" => |v| v["pressure"]["memory"]["some60"].as_f64(),
            _ => |v| v["pressure"]["io"]["some60"].as_f64(),
        },
        scale: PERCENT,
    }
}

/// GPU `n` (0 or 1, as history keeps them).
pub fn gpu_graph(n: usize) -> Option<GraphSpec> {
    Some(GraphSpec {
        series: ["gpu.0.busy", "gpu.1.busy"].get(n)?,
        live: [
            |v: &Value| v["gpus"][0]["busyPercent"].as_f64(),
            |v: &Value| v["gpus"][1]["busyPercent"].as_f64(),
        ][n],
        scale: PERCENT,
    })
}

/// Points of a graph: `(unix seconds, value)`.
pub type Points = Vec<(i64, f64)>;

/// A `history.query` reply's buckets for `series`: averages and peaks.
pub fn history_points(reply: &Value, series: &str) -> (Points, Points) {
    let mut avg = Vec::new();
    let mut max = Vec::new();
    for p in reply["series"][series]["points"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let (Some(t), Some(a), Some(m)) = (p[0].as_i64(), p[1].as_f64(), p[2].as_f64()) else {
            continue;
        };
        avg.push((t, a));
        max.push((t, m));
    }
    (avg, max)
}
