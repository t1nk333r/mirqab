//! Stream names and data shapes. Each tab's plan adds its stream here and
//! pins its shape with `tests/golden/<stream>.json` (architecture §5.2);
//! `tools/check-docs.sh` reads `STREAMS` to check that every golden exists.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SELF: &str = "self";
pub const UI: &str = "ui";

/// Every stream the protocol defines so far.
pub const STREAMS: &[&str] = &["self", "system", "ui"];
pub const SYSTEM: &str = "system";

/// The `self` stream: what mirqab costs (plan 001).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfData {
    pub daemon: ProcessCost,
    /// None while no interface is attached.
    pub ui: Option<UiCost>,
    /// Subscribers per stream.
    pub subscribers: BTreeMap<String, u32>,
    pub uptime_sec: u64,
    /// The footer's machine line (plan 001 phase D).
    #[serde(default)]
    pub machine: Machine,
}

/// The machine the daemon runs on, for the footer: what `/proc/cpuinfo`
/// calls the CPU and how long the machine has been up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub cpu_model: Option<String>,
    pub uptime_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCost {
    /// Percent of one core since the previous sample.
    pub cpu: f64,
    pub rss_bytes: u64,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCost {
    pub cpu: f64,
    pub rss_bytes: u64,
    pub pid: u32,
}

/// The `ui` stream: events only, for the interface (plan 001).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum UiEvent {
    Show {
        tab: Option<String>,
        mode: Option<String>,
    },
    Hide,
    Toggle {
        tab: Option<String>,
    },
    ConfigChanged,
}

/// The `system` stream (plan 002). Every part may be absent: hardware
/// differs, and a reading the machine cannot give is `None`, never zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemData {
    pub cpu: Option<Cpu>,
    pub load: Option<Load>,
    pub mem: Option<Mem>,
    pub pressure: Option<Pressure>,
    pub temps: Vec<Temp>,
    pub thermal: Option<Thermal>,
    pub gpus: Vec<Gpu>,
    pub power: Option<Power>,
    pub uptime_sec: Option<u64>,
    pub boot_time: Option<i64>,
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cpu {
    /// Busy share of all cores since the previous sample; None on the first.
    pub percent: Option<f64>,
    pub user: Option<f64>,
    pub system: Option<f64>,
    pub iowait: Option<f64>,
    pub cores: Vec<Core>,
    pub mhz_avg: Option<u32>,
    pub mhz_max: Option<u32>,
    pub governor: Option<String>,
    pub epp: Option<String>,
    pub model: Option<String>,
    pub physical: Option<u32>,
    pub logical: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Core {
    pub id: u32,
    pub percent: Option<f64>,
    pub mhz: Option<u32>,
    /// `p` (performance) or `e` (efficiency) on hybrid CPUs; None otherwise.
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub runnable: u32,
    pub tasks: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mem {
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// Total − available.
    pub used_bytes: u64,
    /// Reclaimable cache: buffers + cached + reclaimable slab − shared.
    pub cache_bytes: u64,
    pub shmem_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub zram: Option<Zram>,
    /// KiB a second paged in and out (vmstat counts pgpgin in KiB).
    pub page_in_per_sec: Option<f64>,
    pub page_out_per_sec: Option<f64>,
    /// Pages a second swapped in and out.
    pub swap_in_per_sec: Option<f64>,
    pub swap_out_per_sec: Option<f64>,
    pub oom_kills: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Zram {
    pub orig_bytes: u64,
    pub compr_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pressure {
    pub cpu: Option<Stall>,
    pub memory: Option<Stall>,
    pub io: Option<Stall>,
}

/// Pressure stall percentages: the share of time some (or all) tasks
/// waited, averaged over 10 and 60 s by the kernel.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stall {
    pub some10: f64,
    pub some60: f64,
    pub full10: Option<f64>,
    pub full60: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Temp {
    pub id: String,
    pub label: String,
    pub celsius: f64,
    pub critical: Option<f64>,
    /// `cpu`, `gpu`, `disk`, `memory` or `other`.
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thermal {
    /// `cool`, `warm`, `hot` or `critical`.
    pub state: String,
    pub hottest: Option<String>,
    pub throttling: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Gpu {
    pub id: String,
    pub name: String,
    /// `amd`, `intel`, `nvidia` or `other`.
    pub vendor: String,
    /// Runtime-suspended: nothing else is read, so it is not woken.
    pub suspended: bool,
    pub busy_percent: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub celsius: Option<f64>,
    pub watts: Option<f64>,
    pub mhz: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Power {
    pub package_watts: Option<f64>,
    pub battery_watts: Option<f64>,
    /// `rapl`, `battery`, `both` or `none`.
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Advice {
    pub code: String,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ui_events_have_the_documented_shape() {
        let show = UiEvent::Show {
            tab: Some("system".into()),
            mode: Some("popover".into()),
        };
        assert_eq!(
            serde_json::to_value(&show).unwrap(),
            json!({"event": "show", "tab": "system", "mode": "popover"})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::Hide).unwrap(),
            json!({"event": "hide"})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::Toggle { tab: None }).unwrap(),
            json!({"event": "toggle", "tab": null})
        );
        assert_eq!(
            serde_json::to_value(UiEvent::ConfigChanged).unwrap(),
            json!({"event": "config-changed"})
        );
    }

    #[test]
    fn ui_events_match_the_golden_file() {
        let golden: Vec<UiEvent> =
            serde_json::from_str(include_str!("../../../tests/golden/ui.json")).unwrap();
        assert_eq!(
            golden,
            [
                UiEvent::Show {
                    tab: Some("system".into()),
                    mode: Some("popover".into())
                },
                UiEvent::Hide,
                UiEvent::Toggle { tab: None },
                UiEvent::ConfigChanged,
            ]
        );
    }

    #[test]
    fn stream_names_are_unique() {
        let mut names = STREAMS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), STREAMS.len());
    }
}
