//! The window's frame without the widgets: the sidebar's tiles, keyboard
//! movement between tabs, and the footer's text (plan 001 phase D,
//! architecture §7.1).

use crate::format;
use crate::icons::{self, Glyph};
use mirqab_proto::stream::SelfData;
use mirqab_proto::strings as s;

/// Where a tile sits in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Top,
    /// Pushed to the bottom: Trends and Support.
    Bottom,
}

/// A tab: its id, glyph, place, the stream that tells whether this machine
/// can serve it, and the plan that builds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    pub id: &'static str,
    pub icon: Glyph,
    pub place: Place,
    /// None for tabs every machine has (Home, Trends, Support).
    pub stream: Option<&'static str>,
    pub plan: u32,
    /// Whether that plan has landed. Until it has, the tab shows a
    /// placeholder and is never hidden for a missing stream.
    pub built: bool,
}

const fn tab(
    id: &'static str,
    icon: Glyph,
    place: Place,
    stream: Option<&'static str>,
    plan: u32,
) -> Tab {
    Tab {
        id,
        icon,
        place,
        stream,
        plan,
        built: false,
    }
}

/// Every tab, in sidebar order (architecture §7.1).
pub const TABS: &[Tab] = &[
    tab("home", icons::HOME, Place::Top, None, 3),
    tab("apps", icons::APPS, Place::Top, Some("apps"), 6),
    tab("system", icons::SYSTEM, Place::Top, Some("system"), 2),
    tab("fans", icons::FANS, Place::Top, Some("fans"), 15),
    tab("disk", icons::DISK, Place::Top, Some("disk"), 10),
    tab("network", icons::NETWORK, Place::Top, Some("net"), 11),
    tab("devices", icons::DEVICES, Place::Top, Some("devices"), 12),
    tab("battery", icons::BATTERY, Place::Top, Some("battery"), 13),
    tab("volume", icons::VOLUME, Place::Top, Some("audio"), 14),
    tab("ports", icons::PORTS, Place::Top, Some("ports"), 7),
    tab("agents", icons::AGENTS, Place::Top, Some("agents"), 8),
    tab("awake", icons::AWAKE, Place::Top, Some("awake"), 9),
    tab("time", icons::TIME, Place::Top, Some("time"), 16),
    tab("weather", icons::WEATHER, Place::Top, Some("weather"), 17),
    tab("trends", icons::TRENDS, Place::Bottom, None, 18),
    tab("support", icons::SUPPORT, Place::Bottom, None, 19),
];

/// The tab the window opens on when none is asked for.
pub const DEFAULT_TAB: &str = "home";

pub fn find(id: &str) -> Option<&'static Tab> {
    TABS.iter().find(|t| t.id == id)
}

/// One sidebar tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: Glyph,
    pub place: Place,
}

/// The tiles to show, in order, given the daemon's `capabilities` and the
/// user's `tabs.hidden`. A tab is left out when the user hid it or when its
/// plan has landed and its stream is not served here. Home is never left
/// out: it is where the window opens.
pub fn sidebar(tabs: &[Tab], capabilities: &[String], hidden: &[String]) -> Vec<Tile> {
    tabs.iter()
        .filter(|t| t.id == DEFAULT_TAB || !hidden.iter().any(|h| h == t.id))
        .filter(|t| match t.stream {
            Some(stream) if t.built => capabilities.iter().any(|c| c == stream),
            _ => true,
        })
        .map(|t| Tile {
            id: t.id,
            label: s::tab_label(t.id).unwrap_or(t.id),
            icon: t.icon,
            place: t.place,
        })
        .collect()
}

/// The tab Ctrl+`n` opens (1-based, in tile order), if there is one.
pub fn tab_for_number(tiles: &[Tile], n: usize) -> Option<&'static str> {
    n.checked_sub(1).and_then(|i| tiles.get(i)).map(|t| t.id)
}

/// The tab after (or before) `current`, wrapping: Ctrl+Tab and
/// Ctrl+Shift+Tab. A current tab not in the list moves to the first.
pub fn next_tab(tiles: &[Tile], current: &str, backwards: bool) -> Option<&'static str> {
    let n = tiles.len();
    if n == 0 {
        return None;
    }
    let i = match tiles.iter().position(|t| t.id == current) {
        Some(i) if backwards => (i + n - 1) % n,
        Some(i) => (i + 1) % n,
        None => 0,
    };
    Some(tiles[i].id)
}

/// The footer's cost line and its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cost {
    pub text: String,
    pub detail: String,
}

/// From a `self` event: daemon and interface added together, and apart in
/// the tooltip.
pub fn cost(data: &SelfData) -> Cost {
    let ui = data.ui.as_ref();
    let cpu = data.daemon.cpu + ui.map_or(0.0, |u| u.cpu);
    let rss = data.daemon.rss_bytes + ui.map_or(0, |u| u.rss_bytes);
    let d_cpu = format::percent(data.daemon.cpu);
    let d_rss = format::bytes(data.daemon.rss_bytes);
    let u = ui.map(|u| (format::percent(u.cpu), format::bytes(u.rss_bytes)));
    Cost {
        text: s::footer_cost(&format::percent(cpu), &format::bytes(rss)),
        detail: s::footer_cost_detail(
            (&d_cpu, &d_rss),
            u.as_ref().map(|(c, m)| (c.as_str(), m.as_str())),
        ),
    }
}

/// The machine line: the CPU model, shortened, and the uptime.
pub fn machine_line(cpu_model: Option<&str>, uptime_sec: Option<u64>) -> String {
    let model = cpu_model.map(short_cpu_model);
    let up = uptime_sec.map(format::duration);
    s::machine_line(model.as_deref().filter(|m| !m.is_empty()), up.as_deref())
}

/// `AMD Ryzen 9 7950X 16-Core Processor` → `AMD Ryzen 9 7950X`;
/// `Intel(R) Core(TM) i7-1165G7 @ 2.80GHz` → `Intel Core i7-1165G7`.
pub fn short_cpu_model(model: &str) -> String {
    let mut out = model
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "");
    if let Some(i) = out.find(" @ ") {
        out.truncate(i);
    }
    let words: Vec<&str> = out
        .split_whitespace()
        .filter(|w| {
            !w.eq_ignore_ascii_case("processor")
                && !w.eq_ignore_ascii_case("cpu")
                && !w.to_ascii_lowercase().ends_with("-core")
        })
        .collect();
    words.join(" ")
}

/// One line of a `wouldDo` list for the confirmation dialog: the entry's
/// fields as `key value`, sorted by key (serde_json keeps objects sorted).
pub fn would_do_line(entry: &serde_json::Value) -> String {
    match entry {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => format!("{k} {s}"),
                other => format!("{k} {other}"),
            })
            .collect::<Vec<_>>()
            .join(", "),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
