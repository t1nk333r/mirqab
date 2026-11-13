//! Stream names and data shapes. Each tab's plan adds its stream here and
//! pins its shape with `tests/golden/<stream>.json` (architecture §5.2);
//! `tools/check-docs.sh` reads `STREAMS` to check that every golden exists.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SELF: &str = "self";
pub const UI: &str = "ui";

/// Every stream the protocol defines so far.
pub const STREAMS: &[&str] = &["self", "system", "ui"];

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
