//! Theming (architecture §7.3): `resources/style.css`, compiled in, holds
//! the rules; a second provider at application priority + 1 holds the
//! variables, generated from `mirqab_view::theme` plus the followed token
//! file plus `~/.config/mirqab/theme.json`. Both files are watched with a
//! `gio::FileMonitor`, never polled, and a change regenerates the variables.
//! Custom-drawn widgets read colours from [`theme`] and redraw on
//! [`on_change`].

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use mirqab_view::config::SchemeChoice;
use mirqab_view::theme::{Scheme, Theme};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

const BASE: &str = include_str!("../resources/style.css");

thread_local! {
    static CURRENT: RefCell<Rc<Theme>> = RefCell::new(Rc::new(Theme::new(Scheme::Dark)));
    static LISTENERS: RefCell<Vec<Box<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// The theme in force.
pub fn theme() -> Rc<Theme> {
    CURRENT.with(|c| Rc::clone(&c.borrow()))
}

/// Runs `f` after every theme change. `f` should hold its widget weakly.
pub fn on_change(f: impl Fn() + 'static) {
    LISTENERS.with(|l| l.borrow_mut().push(Box::new(f)));
}

/// A colour token as a GDK colour, for drawing code.
pub fn color(name: &str) -> gdk::RGBA {
    let t = theme();
    t.value(name)
        .and_then(|v| gdk::RGBA::parse(v).ok())
        .unwrap_or_else(|| {
            gdk::RGBA::parse(t.value("window-fg-color").unwrap_or_default())
                .unwrap_or(gdk::RGBA::BLACK)
        })
}

/// Series colour `n` (from 0, wrapping) as a GDK colour.
pub fn series(n: usize) -> gdk::RGBA {
    let name = format!("mirqab-series-{}", n % mirqab_view::theme::SERIES + 1);
    color(&name)
}

pub struct Style {
    tokens: gtk::CssProvider,
    choice: Cell<SchemeChoice>,
    follow: RefCell<Option<PathBuf>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

/// `$XDG_CONFIG_HOME/mirqab/theme.json`.
pub fn theme_json() -> PathBuf {
    glib::user_config_dir().join("mirqab/theme.json")
}

impl Style {
    /// Loads both providers on `display`.
    pub fn install(display: &gdk::Display) -> Rc<Style> {
        let base = gtk::CssProvider::new();
        base.load_from_string(BASE);
        gtk::style_context_add_provider_for_display(
            display,
            &base,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let tokens = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            display,
            &tokens,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let style = Rc::new(Style {
            tokens,
            choice: Cell::new(SchemeChoice::Dark),
            follow: RefCell::new(None),
            monitors: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&style);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(s) = weak.upgrade()
                && s.choice.get() == SchemeChoice::System
            {
                s.rebuild();
            }
        });
        style.apply(SchemeChoice::Dark, None);
        style
    }

    /// Applies `theme.scheme` and `theme.follow`; rebuilds and re-watches.
    pub fn apply(self: &Rc<Self>, choice: SchemeChoice, follow: Option<PathBuf>) {
        let same = self.choice.get() == choice && *self.follow.borrow() == follow;
        self.choice.set(choice);
        *self.follow.borrow_mut() = follow;
        adw::StyleManager::default().set_color_scheme(match choice {
            SchemeChoice::Dark => adw::ColorScheme::ForceDark,
            SchemeChoice::Light => adw::ColorScheme::ForceLight,
            SchemeChoice::System => adw::ColorScheme::Default,
        });
        if !same || self.monitors.borrow().is_empty() {
            self.watch();
        }
        self.rebuild();
    }

    fn watch(self: &Rc<Self>) {
        let mut files = vec![theme_json()];
        files.extend(self.follow.borrow().clone());
        let mut monitors = Vec::new();
        for path in files {
            let Ok(m) = gio::File::for_path(&path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            else {
                continue;
            };
            let weak = Rc::downgrade(self);
            m.connect_changed(move |_, _, _, _| {
                if let Some(s) = weak.upgrade() {
                    s.rebuild();
                }
            });
            monitors.push(m);
        }
        *self.monitors.borrow_mut() = monitors;
    }

    fn rebuild(&self) {
        let mut unreadable = Vec::new();
        let followed = self
            .follow
            .borrow()
            .as_ref()
            .and_then(|p| read_json(p, &mut unreadable));
        let scheme = match self.choice.get() {
            SchemeChoice::Dark => Scheme::Dark,
            SchemeChoice::Light => Scheme::Light,
            SchemeChoice::System => followed
                .as_ref()
                .and_then(Theme::followed_scheme)
                .unwrap_or(if adw::StyleManager::default().is_dark() {
                    Scheme::Dark
                } else {
                    Scheme::Light
                }),
        };
        let mut theme = Theme::new(scheme);
        if let Some(f) = &followed {
            theme.follow(f);
        }
        if let Some(own) = read_json(&theme_json(), &mut unreadable) {
            theme.apply(&own);
        }
        theme.problems.extend(unreadable);
        self.tokens.load_from_string(&theme.stylesheet());
        CURRENT.with(|c| *c.borrow_mut() = Rc::new(theme));
        LISTENERS.with(|l| {
            for f in l.borrow().iter() {
                f();
            }
        });
    }

    /// What went wrong reading the theme files, for Settings.
    pub fn problems(&self) -> Vec<String> {
        theme().problems.clone()
    }
}

/// A JSON file, or None when it is missing (the normal case for
/// theme.json) or does not parse (then the reason goes to `problems`).
fn read_json(path: &std::path::Path, problems: &mut Vec<String>) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            problems.push(mirqab_proto::strings::theme_unreadable(
                &path.display().to_string(),
                &e.to_string(),
            ));
            None
        }
    }
}
