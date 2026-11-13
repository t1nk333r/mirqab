//! Start-up and the life of the process (plan 001, "Interface shell"): the
//! stylesheet and colour scheme, the client, the `ui` events (show, hide,
//! toggle, config-changed), and the exit timer: 60 s after the window hides
//! the process quits (architecture §4).

use crate::client::{Client, Subscription};
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
                "--help" | "-h" => o.help = true,
                other => return Err(strings::ui_unknown_option(other)),
            }
        }
        Ok(o)
    }
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
    window: RefCell<Option<Rc<Window>>>,
    exit_timer: RefCell<Option<glib::SourceId>>,
    hold: RefCell<Option<gtk::gio::ApplicationHoldGuard>>,
    ui_sub: RefCell<Option<Subscription>>,
    /// A show asked for before the first config arrived (a hand-run
    /// `mirqab-ui`), run once it has, or on the first failed connection.
    parked: RefCell<Option<(Option<String>, Option<Mode>)>>,
    config_listeners: RefCell<Vec<Box<dyn Fn() -> bool>>>,
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
            window: RefCell::new(None),
            exit_timer: RefCell::new(None),
            hold: RefCell::new(Some(app.hold())),
            ui_sub: RefCell::new(None),
            parked: RefCell::new(None),
            config_listeners: RefCell::new(Vec::new()),
            ready: RefCell::new(Default::default()),
            ready_waiters: RefCell::new(Vec::new()),
        });
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
        let tab = tab
            .filter(|t| *t == "settings" || shell::find(t).is_some())
            .map(str::to_string)
            .unwrap_or_else(|| self.current_tab());
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

    /// Runs `f` after every config or status change (Settings re-reads its
    /// rows) until it returns false (its page is gone).
    pub fn on_config(&self, f: impl Fn() -> bool + 'static) {
        self.config_listeners.borrow_mut().push(Box::new(f));
    }

    fn config_changed(&self) {
        let listeners = std::mem::take(&mut *self.config_listeners.borrow_mut());
        let kept: Vec<_> = listeners.into_iter().filter(|f| f()).collect();
        let mut current = self.config_listeners.borrow_mut();
        let added = std::mem::take(&mut *current);
        *current = kept;
        current.extend(added);
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
