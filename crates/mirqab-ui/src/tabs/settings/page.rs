//! Settings (plan 001, "Interface shell"): window mode and corner, colours
//! and a followed theme file, units, the live interval, hidden tabs, and the
//! daemon's version, socket, history size and helper. Every change is a
//! `config.set`; the daemon writes the file and the rows follow
//! `config-changed`.

use crate::app::Ctx;
use adw::prelude::*;
use mirqab_proto::strings;
use mirqab_view::config::{ANCHORS, SCHEMES, SchemeChoice, UiConfig};
use mirqab_view::format::{self, TempUnit};
use mirqab_view::shell::{DEFAULT_TAB, TABS};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;

const MODES: &[&str] = &["window", "popover"];
const TEMPERATURES: &[&str] = &["c", "f"];
const CLOCKS: &[&str] = &["auto", "12", "24"];

/// A row choosing one of `values`, shown with `label`.
fn combo(
    title: &str,
    values: &'static [&'static str],
    label: fn(&str) -> &str,
    on_pick: impl Fn(&'static str) + 'static,
    quiet: Rc<Cell<bool>>,
) -> adw::ComboRow {
    let names: Vec<&str> = values.iter().map(|v| label(v)).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&names))
        .build();
    row.connect_selected_notify(move |r| {
        if quiet.get() {
            return;
        }
        if let Some(v) = values.get(r.selected() as usize) {
            on_pick(v);
        }
    });
    row
}

fn select(row: &adw::ComboRow, values: &[&str], current: &str) {
    if let Some(i) = values.iter().position(|v| *v == current) {
        row.set_selected(i as u32);
    }
}

fn info_row(title: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder().title(title).build();
    r.add_css_class("property");
    r
}

pub fn build(ctx: &Rc<Ctx>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    // While rows are set from the config, their change handlers stay quiet.
    let quiet = Rc::new(Cell::new(true));
    let set = |key: &'static str| {
        let weak = Rc::downgrade(ctx);
        move |v: Value| {
            if let Some(ctx) = weak.upgrade() {
                ctx.set_setting(key, v);
            }
        }
    };

    let window = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_WINDOW)
        .build();
    let s = set("window.mode");
    let mode = combo(
        strings::SETTINGS_OPENS_AS,
        MODES,
        |m| match m {
            "window" => strings::SETTINGS_WINDOW_MODE,
            _ => strings::SETTINGS_POPOVER,
        },
        move |v| s(json!(v)),
        Rc::clone(&quiet),
    );
    let s = set("window.anchor");
    let anchor = combo(
        strings::SETTINGS_CORNER,
        ANCHORS,
        strings::anchor_label,
        move |v| s(json!(v)),
        Rc::clone(&quiet),
    );
    window.add(&mode);
    window.add(&anchor);

    let appearance = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_APPEARANCE)
        .build();
    let s = set("theme.scheme");
    let scheme = combo(
        strings::SETTINGS_COLOURS,
        SCHEMES,
        strings::scheme_label,
        move |v| s(json!(v)),
        Rc::clone(&quiet),
    );
    let follow = adw::EntryRow::builder()
        .title(strings::SETTINGS_FOLLOW)
        .show_apply_button(true)
        .build();
    let s = set("theme.follow");
    follow.connect_apply(move |e| {
        let text = e.text();
        s(if text.is_empty() {
            Value::Null
        } else {
            json!(text.as_str())
        });
    });
    let problems = gtk::ListBox::new();
    problems.add_css_class("boxed-list");
    problems.set_selection_mode(gtk::SelectionMode::None);
    appearance.add(&scheme);
    appearance.add(&follow);
    appearance.add(&problems);

    let units = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_UNITS)
        .build();
    let s = set("units.temperature");
    let temperature = combo(
        strings::SETTINGS_TEMPERATURE,
        TEMPERATURES,
        strings::temperature_label,
        move |v| s(json!(v)),
        Rc::clone(&quiet),
    );
    let s = set("units.clock");
    let clock = combo(
        strings::SETTINGS_CLOCK,
        CLOCKS,
        strings::clock_label,
        move |v| s(json!(v)),
        Rc::clone(&quiet),
    );
    units.add(&temperature);
    units.add(&clock);

    let live = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_LIVE)
        .description(strings::SETTINGS_LIVE_HINT)
        .build();
    let interval = adw::SpinRow::with_range(1000.0, 5000.0, 500.0);
    interval.set_title(strings::SETTINGS_LIVE_INTERVAL);
    let s = set("live.intervalMs");
    let q = Rc::clone(&quiet);
    interval.connect_value_notify(move |r| {
        if !q.get() {
            s(json!(r.value().round() as u64));
        }
    });
    live.add(&interval);

    let tabs = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_TABS)
        .build();
    let mut switches: Vec<(&'static str, adw::SwitchRow)> = Vec::new();
    for t in TABS.iter().filter(|t| t.id != DEFAULT_TAB) {
        let row = adw::SwitchRow::builder()
            .title(strings::tab_label(t.id).unwrap_or(t.id))
            .build();
        tabs.add(&row);
        switches.push((t.id, row));
    }
    let switches = Rc::new(switches);
    for (_, row) in switches.iter() {
        // Weak: each row holds this closure, so a strong list of the rows
        // would keep them all alive after the page is dropped.
        let all = Rc::downgrade(&switches);
        let s = set("tabs.hidden");
        let q = Rc::clone(&quiet);
        row.connect_active_notify(move |_| {
            let Some(all) = all.upgrade() else { return };
            if q.get() {
                return;
            }
            let hidden: Vec<&str> = all
                .iter()
                .filter(|(_, r)| !r.is_active())
                .map(|(id, _)| *id)
                .collect();
            s(json!(hidden));
        });
    }

    let daemon = adw::PreferencesGroup::builder()
        .title(strings::SETTINGS_DAEMON)
        .build();
    let version = info_row(strings::SETTINGS_VERSION);
    let socket = info_row(strings::SETTINGS_SOCKET);
    let history = info_row(strings::SETTINGS_HISTORY);
    let helper = info_row(strings::SETTINGS_HELPER);
    helper.set_subtitle(strings::SETTINGS_HELPER_NONE);
    for r in [&version, &socket, &history, &helper] {
        daemon.add(r);
    }

    for g in [&window, &appearance, &units, &live, &tabs, &daemon] {
        page.add(g);
    }

    let refresh = {
        let page = page.downgrade();
        let ctx = Rc::downgrade(ctx);
        move || -> bool {
            let (Some(_page), Some(ctx)) = (page.upgrade(), ctx.upgrade()) else {
                return false;
            };
            let cfg: UiConfig = ctx.config.borrow().clone();
            quiet.set(true);
            select(&mode, MODES, cfg.mode.as_str());
            select(&anchor, ANCHORS, &cfg.anchor);
            select(
                &scheme,
                SCHEMES,
                match cfg.scheme {
                    SchemeChoice::Dark => "dark",
                    SchemeChoice::Light => "light",
                    SchemeChoice::System => "system",
                },
            );
            follow.set_text(cfg.follow.as_deref().unwrap_or(""));
            select(
                &temperature,
                TEMPERATURES,
                match cfg.temperature {
                    TempUnit::Celsius => "c",
                    TempUnit::Fahrenheit => "f",
                },
            );
            select(&clock, CLOCKS, &cfg.clock);
            interval.set_value(cfg.live_interval_ms as f64);
            for (id, row) in switches.iter() {
                row.set_active(!cfg.hidden.iter().any(|h| h == id));
            }
            quiet.set(false);

            while let Some(c) = problems.first_child() {
                problems.remove(&c);
            }
            let found = ctx.style.problems();
            problems.set_visible(!found.is_empty());
            for p in found {
                let row = adw::ActionRow::builder().title(p.as_str()).build();
                row.add_css_class("warning");
                problems.append(&row);
            }

            let st = ctx.status.borrow();
            let text = |v: &Value| v.as_str().unwrap_or(strings::NO_VALUE).to_string();
            version.set_subtitle(&text(&st["version"]));
            socket.set_subtitle(&text(&st["socket"]));
            history.set_subtitle(&match st["history"]["sizeBytes"].as_u64() {
                Some(n) if st["history"]["enabled"] == json!(true) => format::bytes(n),
                _ => strings::HISTORY_OFF.to_string(),
            });
            true
        }
    };
    refresh();
    ctx.on_config(refresh);
    ctx.refresh_status();
    page.upcast()
}
