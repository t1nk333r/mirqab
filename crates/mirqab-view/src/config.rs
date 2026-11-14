//! The interface's reading of `config.json` (plan 001, "Config keys added
//! here"): what `config.get` returns, turned into typed settings with every
//! missing or wrong value replaced by its default. The daemon validates on
//! write; this side only has to never fail on what it reads.

use crate::format::TempUnit;
use serde_json::Value;

/// How the interface opens. A normal window is the default (plan 022): with
/// focus-follows-mouse a popover closes as soon as the pointer moves onto
/// another window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Popover,
    #[default]
    Window,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Popover => "popover",
            Mode::Window => "window",
        }
    }

    /// Escape hides a popover; a normal window that vanished on Escape would
    /// surprise, so it closes with its button, the compositor or Ctrl+W.
    pub fn closes_on_escape(self) -> bool {
        self == Mode::Popover
    }

    pub fn parse(v: &str) -> Option<Mode> {
        match v {
            "popover" => Some(Mode::Popover),
            "window" => Some(Mode::Window),
            _ => None,
        }
    }
}

/// The edges a popover is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// The anchors `window.anchor` may name, in the order Settings lists them.
pub const ANCHORS: &[&str] = &[
    "top-right",
    "top",
    "top-left",
    "bottom-right",
    "bottom",
    "bottom-left",
];

impl Anchor {
    /// `top-right` and the like; anything unknown is the default, top right.
    pub fn parse(v: &str) -> Anchor {
        let (top, bottom, left, right) = match v {
            "top-left" => (true, false, true, false),
            "top" => (true, false, false, false),
            "bottom-left" => (false, true, true, false),
            "bottom" => (false, true, false, false),
            "bottom-right" => (false, true, false, true),
            _ => (true, false, false, true),
        };
        Anchor {
            top,
            bottom,
            left,
            right,
        }
    }
}

/// `theme.scheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemeChoice {
    #[default]
    Dark,
    Light,
    /// Follow the desktop's light or dark preference.
    System,
}

pub const SCHEMES: &[&str] = &["dark", "light", "system"];

#[derive(Debug, Clone, PartialEq)]
pub struct UiConfig {
    pub mode: Mode,
    pub anchor: String,
    pub margin_em: f64,
    pub width_em: f64,
    pub height_em: f64,
    pub live_interval_ms: u64,
    pub scheme: SchemeChoice,
    pub follow: Option<String>,
    pub temperature: TempUnit,
    pub clock: String,
    pub pinned: Vec<String>,
    pub seeded: bool,
    pub remember_tab: bool,
    pub last_tab: Option<String>,
    pub hidden: Vec<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig::from_json(&Value::Null)
    }
}

impl UiConfig {
    pub fn from_json(v: &Value) -> UiConfig {
        let s = |k: &str| lookup(v, k).and_then(Value::as_str);
        let n = |k: &str, lo: f64, hi: f64, d: f64| {
            lookup(v, k)
                .and_then(Value::as_f64)
                .filter(|x| (lo..=hi).contains(x))
                .unwrap_or(d)
        };
        let list = |k: &str| -> Vec<String> {
            lookup(v, k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        UiConfig {
            mode: s("window.mode").and_then(Mode::parse).unwrap_or_default(),
            anchor: s("window.anchor")
                .filter(|a| ANCHORS.contains(a))
                .unwrap_or("top-right")
                .to_string(),
            margin_em: n("window.marginEm", 0.0, 10.0, 0.5),
            width_em: n("window.widthEm", 30.0, 120.0, 58.0),
            height_em: n("window.heightEm", 20.0, 80.0, 40.0),
            live_interval_ms: n("live.intervalMs", 1000.0, 5000.0, 1000.0) as u64,
            scheme: match s("theme.scheme") {
                Some("light") => SchemeChoice::Light,
                Some("system") => SchemeChoice::System,
                _ => SchemeChoice::Dark,
            },
            follow: s("theme.follow")
                .filter(|p| !p.is_empty())
                .map(str::to_string),
            temperature: TempUnit::from_config(s("units.temperature").unwrap_or("c")),
            clock: s("units.clock")
                .filter(|c| ["auto", "12", "24"].contains(c))
                .unwrap_or("auto")
                .to_string(),
            pinned: list("home.pinned"),
            seeded: lookup(v, "home.seeded")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            remember_tab: lookup(v, "window.rememberTab")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            last_tab: s("window.lastTab").map(str::to_string),
            hidden: list("tabs.hidden"),
        }
    }
}

fn lookup<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(v, |cur, part| cur.get(part))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_config_without_a_mode_opens_a_window() {
        assert_eq!(UiConfig::from_json(&json!({})).mode, Mode::Window);
        assert_eq!(
            UiConfig::from_json(&json!({"window": {"mode": "bogus"}})).mode,
            Mode::Window
        );
        assert_eq!(
            UiConfig::from_json(&json!({"window": {"mode": "popover"}})).mode,
            Mode::Popover
        );
    }

    #[test]
    fn only_a_popover_closes_on_escape() {
        assert!(Mode::Popover.closes_on_escape());
        assert!(!Mode::Window.closes_on_escape());
    }
}
