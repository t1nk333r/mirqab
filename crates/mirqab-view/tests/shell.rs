//! The sidebar for a `capabilities` set, tab movement, and the footer
//! (plan 001, Tests).

use mirqab_proto::stream::{ProcessCost, SelfData, UiCost};
use mirqab_view::cards;
use mirqab_view::icons;
use mirqab_view::shell::{self, Place, TABS, Tab};
use std::collections::{BTreeMap, HashSet};

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn sixteen_tabs_in_the_documented_order() {
    let ids: Vec<&str> = TABS.iter().map(|t| t.id).collect();
    assert_eq!(
        ids,
        [
            "home", "apps", "system", "fans", "disk", "network", "devices", "battery", "volume",
            "ports", "agents", "awake", "time", "weather", "trends", "support"
        ]
    );
    let bottom: Vec<&str> = TABS
        .iter()
        .filter(|t| t.place == Place::Bottom)
        .map(|t| t.id)
        .collect();
    assert_eq!(bottom, ["trends", "support"]);
    for t in TABS {
        assert!(
            mirqab_proto::strings::tab_label(t.id).is_some(),
            "{} label",
            t.id
        );
    }
}

#[test]
fn unbuilt_tabs_show_whatever_the_daemon_serves() {
    let tiles = shell::sidebar(TABS, &strings(&["system"]), &[]);
    assert_eq!(tiles.len(), 16);
    // System is built (plan 002): without its stream it is hidden.
    let tiles = shell::sidebar(TABS, &[], &[]);
    assert_eq!(tiles.len(), 15);
    assert!(tiles.iter().all(|t| t.id != "system"));
}

#[test]
fn a_built_tab_hides_without_its_stream_and_hidden_tabs_hide() {
    let mut tabs: Vec<Tab> = TABS.to_vec();
    for t in &mut tabs {
        if t.id == "battery" || t.id == "fans" {
            t.built = true;
        }
    }
    let caps = strings(&["system", "fans", "history"]);
    let tiles = shell::sidebar(&tabs, &caps, &strings(&["weather", "home"]));
    let ids: HashSet<&str> = tiles.iter().map(|t| t.id).collect();
    assert!(!ids.contains("battery"), "built and not served");
    assert!(ids.contains("fans"), "built and served");
    assert!(!ids.contains("weather"), "hidden by the user");
    assert!(ids.contains("home"), "home is never hidden");
    assert_eq!(tiles.len(), 14);
}

#[test]
fn keyboard_moves_between_tiles() {
    let tiles = shell::sidebar(TABS, &strings(&["system"]), &[]);
    assert_eq!(shell::tab_for_number(&tiles, 1), Some("home"));
    assert_eq!(shell::tab_for_number(&tiles, 3), Some("system"));
    assert_eq!(shell::tab_for_number(&tiles, 0), None);
    assert_eq!(shell::tab_for_number(&tiles, 17), None);
    assert_eq!(shell::next_tab(&tiles, "home", false), Some("apps"));
    assert_eq!(shell::next_tab(&tiles, "home", true), Some("support"));
    assert_eq!(shell::next_tab(&tiles, "support", false), Some("home"));
    assert_eq!(shell::next_tab(&tiles, "settings", false), Some("home"));
    assert_eq!(shell::next_tab(&[], "home", false), None);
}

fn self_data(ui: Option<UiCost>) -> SelfData {
    SelfData {
        daemon: ProcessCost {
            cpu: 0.1,
            rss_bytes: 3_000_000,
            threads: 4,
        },
        ui,
        subscribers: BTreeMap::new(),
        uptime_sec: 60,
        machine: Default::default(),
    }
}

#[test]
fn the_footer_adds_daemon_and_interface() {
    let c = shell::cost(&self_data(None));
    assert_eq!(c.text, "self 0.1% \u{b7} 3.0 MB");
    assert_eq!(c.detail, "daemon 0.1% \u{b7} 3.0 MB");
    let c = shell::cost(&self_data(Some(UiCost {
        cpu: 0.3,
        rss_bytes: 11_000_000,
        pid: 42,
    })));
    assert_eq!(c.text, "self 0.4% \u{b7} 14 MB");
    assert_eq!(
        c.detail,
        "daemon 0.1% \u{b7} 3.0 MB\ninterface 0.3% \u{b7} 11 MB"
    );
}

#[test]
fn the_machine_line_shortens_the_cpu_model() {
    assert_eq!(
        shell::short_cpu_model("AMD Ryzen 9 7950X 16-Core Processor"),
        "AMD Ryzen 9 7950X"
    );
    assert_eq!(
        shell::short_cpu_model("Intel(R) Core(TM) i7-1165G7 @ 2.80GHz"),
        "Intel Core i7-1165G7"
    );
    assert_eq!(
        shell::machine_line(
            Some("Intel(R) Celeron(R) CPU N3050 @ 1.60GHz"),
            Some(3 * 3600 + 120)
        ),
        "Intel Celeron N3050 \u{b7} up 3h 2m"
    );
    assert_eq!(shell::machine_line(None, Some(30)), "up 30s");
    assert_eq!(shell::machine_line(Some("Processor"), None), "");
    assert_eq!(shell::machine_line(None, None), "");
}

#[test]
fn glyphs_are_unique_and_private_use() {
    let mut seen = HashSet::new();
    for g in icons::ALL {
        assert!(seen.insert(g.ch), "{} reuses a code point", g.name);
        assert!(('\u{f0000}'..='\u{fffff}').contains(&g.ch), "{}", g.name);
    }
}

#[test]
fn pinned_cards_skip_unknown_ids() {
    static TEST_CARDS: &[cards::Card] = &[
        cards::Card {
            id: "system.cpu",
            tab: "system",
            stream: "system",
        },
        cards::Card {
            id: "system.mem",
            tab: "system",
            stream: "system",
        },
    ];
    let got = cards::pinned(TEST_CARDS, &strings(&["system.mem", "gone", "system.cpu"]));
    let ids: Vec<&str> = got.iter().map(|c| c.id).collect();
    assert_eq!(ids, ["system.mem", "system.cpu"]);
    assert_eq!(cards::find("system.cpu").map(|c| c.tab), Some("system"));
    assert!(cards::find("nope").is_none());
    let mut ids: Vec<&str> = cards::CARDS.iter().map(|c| c.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), cards::CARDS.len(), "card ids are unique");
}
