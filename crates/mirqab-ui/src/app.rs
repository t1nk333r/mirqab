//! Start-up and the life of the process (plan 001, "Interface shell"): the
//! stylesheet and colour scheme, the client, the `ui` events (show, hide,
//! toggle, config-changed), and the exit timer: 60 s after the window hides
//! the process quits (architecture §4).

use crate::client::{Client, Subscription};
use crate::listeners::Listeners;
use crate::style::Style;
use crate::window::Window;
use adw::prelude::*;
use gtk::glib;
use mirqab_proto::stream::{self, UiEvent};
use mirqab_proto::strings;
use mirqab_view::config::{Mode, UiConfig};
use mirqab_view::shell::{self, TABS, Tile};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub const APP_ID: &str = "io.github.t1nk333r.mirqab";

/// What the command line asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Opts {
    pub socket: Option<PathBuf>,
    pub window: bool,
    pub tab: Option<String>,
    pub background: bool,
    pub selftest: Option<String>,
    /// `--do STEPS` with `--selftest`: scripted steps, test runs only
    /// (architecture §7.5).
    pub steps: Option<String>,
    pub help: bool,
}

impl Opts {
    pub fn parse(args: &[String]) -> Result<Opts, String> {
        let mut o = Opts::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |name: &str| {
                it.next()
                    .cloned()
                    .ok_or_else(|| strings::missing_value(name))
            };
            match a.as_str() {
                "--socket" => o.socket = Some(PathBuf::from(value("--socket")?)),
                "--window" => o.window = true,
                "--tab" => o.tab = Some(value("--tab")?),
                "--background" => o.background = true,
                "--selftest" => o.selftest = Some(value("--selftest")?),
                "--do" => o.steps = Some(value("--do")?),
                "--help" | "-h" => o.help = true,
                other => return Err(strings::ui_unknown_option(other)),
            }
        }
        Ok(o)
    }

    /// Why these options are refused, if they are (review of PR #19,
    /// S1): `--do` outside a test run, and `--selftest` reaching an
    /// interface that is already running (`first` false: the arguments
    /// came over the session bus from a second `mirqab-ui`), which would
    /// otherwise drive the running window and exit it. And a `--selftest`
    /// of a tab the window cannot open: `show` would fall back to a tab
    /// the selftest does not wait on (audit 2026-11-21, L57). Checked where
    /// the process starts and again where forwarded arguments arrive.
    pub fn refusal(&self, first: bool, test_env: bool) -> Option<&'static str> {
        if self.steps.is_some() && !test_env {
            Some(strings::UI_DO_TEST_ONLY)
        } else if self.selftest.is_some() && !first {
            Some(strings::UI_SELFTEST_NOT_FORWARDED)
        } else if self.selftest.as_deref().is_some_and(|t| !shell::opens(t)) {
            Some(strings::UI_SELFTEST_UNKNOWN_TAB)
        } else {
            None
        }
    }
}

/// Whether this is a test run (`MIRQAB_TEST=1`).
pub fn test_env() -> bool {
    std::env::var("MIRQAB_TEST").as_deref() == Ok("1")
}

/// Something waiting for a page's first data.
type Waiter = Box<dyn FnOnce()>;

/// Shared by the window, the pages and the event handlers.
pub struct Ctx {
    pub app: adw::Application,
    pub client: Client,
    pub style: Rc<Style>,
    pub config: RefCell<UiConfig>,
    /// The last `status` reply, for Settings' daemon section.
    pub status: RefCell<Value>,
    /// The Apps page's sort, switch, open groups and selection (plan 006,
    /// decision 13): kept while the interface runs, so a page dropped on
    /// hide comes back as it was.
    pub apps: RefCell<mirqab_view::tabs::apps::AppsState>,
    window: RefCell<Option<Rc<Window>>>,
    exit_timer: RefCell<Option<glib::SourceId>>,
    hold: RefCell<Option<gtk::gio::ApplicationHoldGuard>>,
    ui_sub: RefCell<Option<Subscription>>,
    /// A show asked for before the first config arrived (a hand-run
    /// `mirqab-ui`), run once it has, or on the first failed connection.
    parked: RefCell<Option<(Option<String>, Option<Mode>)>>,
    config_listeners: Listeners,
    /// Pages that have shown their first data since the window appeared,
    /// and what waits for one (the selftest).
    ready: RefCell<std::collections::HashSet<String>>,
    ready_waiters: RefCell<Vec<(String, Waiter)>>,
}

/// The exit delay: 60 s, or `MIRQAB_UI_EXIT_MS` when `MIRQAB_TEST=1`.
fn exit_delay() -> Duration {
    if std::env::var("MIRQAB_TEST").as_deref() == Ok("1")
        && let Some(ms) = std::env::var("MIRQAB_UI_EXIT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
    {
        return Duration::from_millis(ms);
    }
    Duration::from_secs(60)
}

impl Ctx {
    pub fn new(app: &adw::Application, socket: PathBuf) -> Rc<Ctx> {
        let display = gtk::gdk::Display::default().expect("a display");
        let style = Style::install(&display);
        let ctx = Rc::new(Ctx {
            app: app.clone(),
            client: Client::new(socket),
            style,
            config: RefCell::new(UiConfig::default()),
            status: RefCell::new(Value::Null),
            apps: RefCell::new(Default::default()),
            window: RefCell::new(None),
            exit_timer: RefCell::new(None),
            hold: RefCell::new(Some(app.hold())),
            ui_sub: RefCell::new(None),
            parked: RefCell::new(None),
            config_listeners: Listeners::new(),
            ready: RefCell::new(Default::default()),
            ready_waiters: RefCell::new(Vec::new()),
        });
        // The Apps page and the `apps.top` card share one subscription
        // (plan 006, decision 2).
        ctx.client
            .set_merge(stream::APPS, mirqab_view::tabs::apps::merge_params);
        // The System page or a Home card that shows the disks or the GPUs
        // keeps its flags beside a graph's live handle (plan 024).
        ctx.client
            .set_merge(stream::SYSTEM, mirqab_view::tabs::system::merge_params);
        let weak = Rc::downgrade(&ctx);
        ctx.client.on_state(move |client| {
            let Some(ctx) = weak.upgrade() else { return };
            if client.connected() {
                ctx.refresh_config();
                ctx.refresh_status();
            } else {
                // No daemon: show what was asked for, with defaults and the
                // reconnect notice.
                ctx.unpark();
            }
            if let Some(w) = ctx.window() {
                w.footer.set_notice(&if client.connected() {
                    String::new()
                } else {
                    strings::reconnecting(client.retry_in())
                });
                w.refresh_tiles(&ctx);
            }
        });
        ctx
    }

    /// Subscribes to `ui` as the interface. Called once the first config
    /// reply is in: the daemon sends a queued `show` right after this
    /// subscription, so the first window is built with the user's config
    /// (review of PR #5: it was built with the defaults).
    fn attach_ui(self: &Rc<Self>) {
        if self.ui_sub.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let sub = self.client.subscribe_params(
            stream::UI,
            0,
            json!({"role": "ui", "pid": std::process::id()}),
            Rc::new(move |data: &Value| {
                let Some(ctx) = weak.upgrade() else { return };
                if let Ok(ev) = serde_json::from_value::<UiEvent>(data.clone()) {
                    ctx.event(ev);
                }
            }),
        );
        *self.ui_sub.borrow_mut() = Some(sub);
    }

    /// Shows the window once the config is known; at once if it already is.
    pub fn show_when_ready(self: &Rc<Self>, tab: Option<&str>, mode: Option<Mode>) {
        if self.ui_sub.borrow().is_some() {
            self.show(tab, mode);
        } else {
            *self.parked.borrow_mut() = Some((tab.map(str::to_string), mode));
        }
    }

    fn unpark(self: &Rc<Self>) {
        let parked = self.parked.borrow_mut().take();
        if let Some((tab, mode)) = parked {
            self.show(tab.as_deref(), mode);
        }
    }

    fn event(self: &Rc<Self>, ev: UiEvent) {
        match ev {
            UiEvent::Show { tab, mode } => {
                self.show(tab.as_deref(), mode.as_deref().and_then(Mode::parse));
            }
            UiEvent::Hide => self.hide(),
            UiEvent::Toggle { tab } => {
                if self.window().is_some_and(|w| w.is_visible()) {
                    self.hide();
                } else {
                    self.show(tab.as_deref(), None);
                }
            }
            UiEvent::ConfigChanged => self.refresh_config(),
        }
    }

    pub fn window(&self) -> Option<Rc<Window>> {
        self.window.borrow().clone()
    }

    /// The sidebar's tiles now.
    pub fn tiles(&self) -> Vec<Tile> {
        shell::sidebar(
            TABS,
            &self.client.capabilities(),
            &self.config.borrow().hidden,
        )
    }

    pub fn current_tab(&self) -> String {
        self.window()
            .map(|w| w.current())
            .unwrap_or_else(|| shell::DEFAULT_TAB.to_string())
    }

    /// Shows the window on `tab` (or where it was), in `mode` (or the
    /// configured one). A different mode builds a new window.
    pub fn show(self: &Rc<Self>, tab: Option<&str>, mode: Option<Mode>) {
        // Without an explicit mode the open window keeps its own: a tile
        // click must not turn `--window` back into the configured popover
        // (review of PR #5).
        // The window's own mode only while it is shown: a hidden window must
        // not pin the mode for the life of the process.
        let mode = mode
            .or_else(|| self.window().filter(|w| w.is_visible()).map(|w| w.mode))
            .unwrap_or(self.config.borrow().mode);
        let valid = |t: &&str| shell::opens(t);
        // No tab: an open window stays where it is; otherwise Home, or the
        // last tab with `window.rememberTab` (plan 003).
        let tab = tab.filter(valid).map(str::to_string).unwrap_or_else(|| {
            let cfg = self.config.borrow();
            if self.window().is_some_and(|w| w.is_visible()) {
                self.current_tab()
            } else if cfg.remember_tab {
                cfg.last_tab
                    .clone()
                    .filter(|t| valid(&t.as_str()))
                    .unwrap_or_else(|| self.current_tab())
            } else {
                shell::DEFAULT_TAB.to_string()
            }
        });
        let reuse = self
            .window()
            .filter(|w| w.mode == mode || (mode == Mode::Popover && !crate::window::layer_shell()));
        let w = match reuse {
            Some(w) => w,
            None => {
                // Taken out first: hiding fires `is-active` at once, and its
                // handler reads `self.window` (review of PR #5: a borrow
                // held across `hide` aborted the process).
                let old = self.window.borrow_mut().take();
                if let Some(old) = old {
                    old.hide();
                    old.win.destroy();
                }
                let w = Window::new(self, mode);
                let weak = Rc::downgrade(self);
                w.win.connect_close_request(move |_| {
                    if let Some(ctx) = weak.upgrade() {
                        ctx.hide();
                    }
                    glib::Propagation::Stop
                });
                *self.window.borrow_mut() = Some(Rc::clone(&w));
                w
            }
        };
        w.show_tab(self, &tab);
        w.present(self);
        let remember = {
            let mut cfg = self.config.borrow_mut();
            let changed = cfg.remember_tab && cfg.last_tab.as_deref() != Some(tab.as_str());
            if changed {
                cfg.last_tab = Some(tab.clone());
            }
            changed
        };
        if remember {
            self.set_setting("window.lastTab", json!(tab));
        }
        // Last: replacing a window may have started the exit timer.
        if let Some(id) = self.exit_timer.borrow_mut().take() {
            id.remove();
        }
    }

    /// Hides the window, drops its pages, and starts the exit timer.
    pub fn hide(self: &Rc<Self>) {
        if let Some(w) = self.window() {
            w.hide();
        }
        self.ready.borrow_mut().clear();
        if self.exit_timer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        // mirqab:ui-timeout
        let id = glib::timeout_add_local_once(exit_delay(), move || {
            if let Some(ctx) = weak.upgrade() {
                ctx.exit_timer.borrow_mut().take();
                ctx.quit();
            }
        });
        *self.exit_timer.borrow_mut() = Some(id);
    }

    pub fn quit(&self) {
        *self.ui_sub.borrow_mut() = None;
        self.client.close();
        self.hold.borrow_mut().take();
        self.app.quit();
    }

    pub fn stop_daemon(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.client.request("shutdown", Value::Null, move |_| {
            if let Some(ctx) = weak.upgrade() {
                ctx.quit();
            }
        });
    }

    /// A page has shown its first data (a placeholder or Settings at once).
    pub fn page_ready(&self, tab: &str) {
        if !self.ready.borrow_mut().insert(tab.to_string()) {
            return;
        }
        let waiters = std::mem::take(&mut *self.ready_waiters.borrow_mut());
        let (now, later): (Vec<_>, Vec<_>) = waiters.into_iter().partition(|(t, _)| t == tab);
        self.ready_waiters.borrow_mut().extend(later);
        for (_, f) in now {
            f();
        }
    }

    /// Runs `f` once `tab`'s page has shown its first data.
    pub fn when_ready(&self, tab: &str, f: impl FnOnce() + 'static) {
        if self.ready.borrow().contains(tab) {
            f();
        } else {
            self.ready_waiters
                .borrow_mut()
                .push((tab.to_string(), Box::new(f)));
        }
    }

    /// Runs `f` with `owner` after every config or status change (Settings
    /// re-reads its rows) while `owner` lives.
    pub fn on_config<W: IsA<glib::Object>>(&self, owner: &W, f: impl Fn(&W) + 'static) {
        self.config_listeners.add(owner, f);
    }

    /// Config listeners held (tests).
    #[doc(hidden)]
    pub fn config_listener_count(&self) -> usize {
        self.config_listeners.len()
    }

    fn config_changed(&self) {
        self.config_listeners.run();
    }

    pub fn refresh_config(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.client.request("config.get", Value::Null, move |r| {
            let Some(ctx) = weak.upgrade() else { return };
            if let Ok(v) = r {
                ctx.set_config(UiConfig::from_json(&v));
            }
            // Even when config.get failed: attach with the defaults.
            ctx.attach_ui();
            ctx.unpark();
        });
    }

    fn set_config(self: &Rc<Self>, cfg: UiConfig) {
        let changed_style = {
            let old = self.config.borrow();
            old.scheme != cfg.scheme || old.follow != cfg.follow
        };
        *self.config.borrow_mut() = cfg;
        self.seed_home();
        let cfg = self.config.borrow().clone();
        if changed_style {
            self.style
                .apply(cfg.scheme, cfg.follow.as_ref().map(PathBuf::from));
        }
        if let Some(w) = self.window() {
            w.refresh_tiles(self);
            // A reused popover follows a new corner, margin or size. A
            // window keeps the size the user dragged it to (review of
            // PR #5: set_default_size resized it on every config change).
            if w.mode == Mode::Popover {
                w.place(&cfg);
            }
        }
        self.config_changed();
    }

    /// A first run pins Home's seed cards, once (plan 003). Only against a
    /// daemon: the defaults used while disconnected are not the user's.
    fn seed_home(self: &Rc<Self>) {
        if !self.client.connected() {
            return;
        }
        use mirqab_view::tabs::home;
        let (seed, mark) = {
            let cfg = self.config.borrow();
            (
                home::seed(&cfg.pinned, cfg.seeded),
                home::mark_seeded(&cfg.pinned, cfg.seeded),
            )
        };
        if seed.is_none() && !mark {
            return;
        }
        self.config.borrow_mut().seeded = true;
        // Seeded first: a daemon gone between the two writes must not leave
        // pins that a later emptying would refill (review of PR #9). The
        // other way round, a first run would then keep an empty Home with
        // its hint, which is the lesser harm.
        self.set_setting("home.seeded", json!(true));
        if let Some(seed) = seed {
            self.config.borrow_mut().pinned = seed.clone();
            self.set_setting("home.pinned", json!(seed));
        }
    }

    pub fn refresh_status(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.client.request("status", Value::Null, move |r| {
            if let (Some(ctx), Ok(v)) = (weak.upgrade(), r) {
                *ctx.status.borrow_mut() = v;
                ctx.config_changed();
            }
        });
    }

    /// `config.set`; a refusal shows in the footer.
    pub fn set_setting(self: &Rc<Self>, key: &str, value: Value) {
        let weak = Rc::downgrade(self);
        self.client.request(
            "config.set",
            json!({"key": key, "value": value}),
            move |r| {
                if let (Some(ctx), Err(e)) = (weak.upgrade(), r)
                    && let Some(w) = ctx.window()
                {
                    w.footer.set_notice(&strings::setting_failed(&e.message));
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Opts;
    use mirqab_proto::strings;

    fn opts(args: &[&str]) -> Opts {
        let v: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Opts::parse(&v).unwrap()
    }

    #[test]
    fn steps_need_a_test_run_and_a_selftest_never_joins_a_running_interface() {
        let steps = opts(&["--selftest", "apps", "--do", "pause 1"]);
        assert_eq!(
            steps.refusal(true, false),
            Some(strings::UI_DO_TEST_ONLY),
            "steps without MIRQAB_TEST=1"
        );
        assert_eq!(steps.refusal(true, true), None, "steps in a test run");
        assert_eq!(
            opts(&["--selftest", "apps"]).refusal(false, true),
            Some(strings::UI_SELFTEST_NOT_FORWARDED),
            "a selftest forwarded to a running interface"
        );
        assert_eq!(opts(&["--tab", "apps"]).refusal(false, false), None);
        assert_eq!(opts(&["--selftest", "apps"]).refusal(true, false), None);
        assert_eq!(opts(&["--selftest", "settings"]).refusal(true, false), None);
    }

    /// Audit 2026-11-21, L57: the selftest waited on Home for an unknown
    /// tab while `show` opened the remembered one, and exited 1 after 5 s.
    /// It is refused at once instead.
    #[test]
    fn a_selftest_of_an_unknown_tab_is_refused() {
        assert_eq!(
            opts(&["--selftest", "nonsense"]).refusal(true, true),
            Some(strings::UI_SELFTEST_UNKNOWN_TAB)
        );
    }
}
