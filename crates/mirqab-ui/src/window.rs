//! The window (plan 001, "Interface shell"; architecture §4): an
//! `adw::ApplicationWindow` holding the sidebar beside a column of header,
//! page slot and footer. In popover mode it becomes a layer surface before
//! it is first shown; in window mode it is an ordinary toplevel. A surface
//! cannot change role, so switching mode builds a new window.

use crate::app::Ctx;
use crate::client::Subscription;
use crate::pages::Pages;
use crate::widgets::em;
use crate::widgets::footer::Footer;
use crate::widgets::icon::icon;
use crate::widgets::icon::icon_button;
use crate::widgets::sidebar::Sidebar;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use mirqab_proto::stream::{self, SelfData};
use mirqab_proto::strings;
use mirqab_view::config::{Anchor, Mode, UiConfig};
use mirqab_view::icons;
use mirqab_view::shell;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Whether popovers can be layer surfaces: a Wayland display whose
/// compositor offers wlr-layer-shell. Asked only on Wayland, where
/// `gtk4_layer_shell::is_supported` is defined (elsewhere it logs a GLib
/// critical).
pub fn layer_shell() -> bool {
    gdk::Display::default().is_some_and(|d| d.type_().name() == "GdkWaylandDisplay")
        && gtk4_layer_shell::is_supported()
}

/// What a keyboard shortcut does.
type Command = Box<dyn Fn(&Rc<Ctx>)>;

pub struct Window {
    pub win: adw::ApplicationWindow,
    pub mode: Mode,
    pub sidebar: Sidebar,
    pub footer: Footer,
    pub pages: Pages,
    title: gtk::Label,
    pinned: gtk::ToggleButton,
    current: RefCell<String>,
    self_sub: RefCell<Option<Subscription>>,
    was_active: Cell<bool>,
}

impl Window {
    /// Builds a window of `mode`. A popover where the compositor has no
    /// layer shell becomes a window, and the footer says so once.
    pub fn new(ctx: &Rc<Ctx>, asked: Mode) -> Rc<Window> {
        let layer = asked == Mode::Popover && layer_shell();
        let mode = if layer { Mode::Popover } else { Mode::Window };
        let win = adw::ApplicationWindow::builder()
            .application(&ctx.app)
            .title(strings::APP_NAME)
            .build();
        win.add_css_class("mirqab");
        win.set_hide_on_close(true);

        let weak = Rc::downgrade(ctx);
        let sidebar = Sidebar::new(move |id| {
            if let Some(ctx) = weak.upgrade() {
                ctx.show(Some(id), None);
            }
        });

        let menu = gio::Menu::new();
        menu.append(
            Some(if mode == Mode::Popover {
                strings::OPEN_AS_WINDOW
            } else {
                strings::OPEN_AS_POPOVER
            }),
            Some("win.switch-mode"),
        );
        menu.append(Some(strings::STOP_DAEMON), Some("win.stop-daemon"));
        let footer = Footer::new(&menu);

        let title = gtk::Label::new(None);
        title.add_css_class("title-4");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let pinned = gtk::ToggleButton::builder()
            .child(&icon(icons::PIN))
            .build();
        pinned.add_css_class("flat");
        pinned.set_tooltip_text(Some(strings::PINNED));
        pinned.update_property(&[gtk::accessible::Property::Label(strings::PINNED)]);
        let close = icon_button(icons::CLOSE, strings::CLOSE);
        let weak = Rc::downgrade(ctx);
        close.connect_clicked(move |_| {
            if let Some(ctx) = weak.upgrade() {
                ctx.hide();
            }
        });
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("mirqab-header");
        header.append(&title);
        if mode == Mode::Popover {
            header.append(&pinned);
            header.append(&close);
        } else {
            // A normal window has no header bar: the row drags it and ends
            // with the desktop's own buttons, whose close hides (plan 022).
            header.append(&gtk::WindowControls::new(gtk::PackType::End));
        }
        let header_row = gtk::WindowHandle::new();
        header_row.set_child(Some(&header));

        let pages = Pages::new();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.set_hexpand(true);
        column.append(&header_row);
        column.append(&pages.stack);
        column.append(&footer.root);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&sidebar.root);
        body.append(&column);
        win.set_content(Some(&body));

        let w = Rc::new(Window {
            win,
            mode,
            sidebar,
            footer,
            pages,
            title,
            pinned,
            current: RefCell::new(shell::DEFAULT_TAB.to_string()),
            self_sub: RefCell::new(None),
            was_active: Cell::new(false),
        });
        if asked == Mode::Popover && !layer {
            w.footer.set_notice(strings::POPOVER_UNSUPPORTED);
        }
        w.size_and_place(ctx);
        w.actions(ctx);
        w.shortcuts(ctx);
        w.close_on_focus_loss(ctx);
        w.refresh_tiles(ctx);
        w
    }

    /// Makes a popover a layer surface; once, before it is first shown.
    fn size_and_place(&self, ctx: &Rc<Ctx>) {
        if self.mode == Mode::Popover {
            self.win.add_css_class("popover");
            self.win.init_layer_shell();
            self.win.set_layer(Layer::Top);
            self.win.set_namespace(Some("mirqab"));
            self.win.set_keyboard_mode(KeyboardMode::OnDemand);
        }
        self.place(&ctx.config.borrow());
    }

    /// Size in em of the base font, clamped to the monitor; for a popover
    /// also its anchors and margins. Run again when the config changes: a
    /// layer surface takes new anchors while it exists.
    pub fn place(&self, cfg: &UiConfig) {
        let e = f64::from(em(&self.win));
        let (mut w, mut h) = (cfg.width_em * e, cfg.height_em * e);
        if let Some(geo) = gdk::Display::default()
            .and_then(|d| d.monitors().item(0))
            .and_then(|m| m.downcast::<gdk::Monitor>().ok())
            .map(|m| m.geometry())
        {
            w = w.min(f64::from(geo.width()));
            h = h.min(f64::from(geo.height()));
        }
        self.win.set_default_size(w as i32, h as i32);
        if self.mode != Mode::Popover {
            return;
        }
        let a = Anchor::parse(&cfg.anchor);
        let margin = (cfg.margin_em * e) as i32;
        for (edge, on) in [
            (Edge::Top, a.top),
            (Edge::Bottom, a.bottom),
            (Edge::Left, a.left),
            (Edge::Right, a.right),
        ] {
            self.win.set_anchor(edge, on);
            self.win.set_margin(edge, if on { margin } else { 0 });
        }
    }

    fn actions(&self, ctx: &Rc<Ctx>) {
        let switch = gio::SimpleAction::new("switch-mode", None);
        let weak = Rc::downgrade(ctx);
        let other = if self.mode == Mode::Popover {
            Mode::Window
        } else {
            Mode::Popover
        };
        switch.connect_activate(move |_, _| {
            if let Some(ctx) = weak.upgrade() {
                let tab = ctx.current_tab();
                ctx.show(Some(&tab), Some(other));
            }
        });
        let stop = gio::SimpleAction::new("stop-daemon", None);
        let weak = Rc::downgrade(ctx);
        stop.connect_activate(move |_, _| {
            if let Some(ctx) = weak.upgrade() {
                ctx.stop_daemon();
            }
        });
        self.win.add_action(&switch);
        self.win.add_action(&stop);
        let weak = Rc::downgrade(ctx);
        self.footer.health.connect_clicked(move |_| {
            if let Some(ctx) = weak.upgrade() {
                ctx.show(Some("support"), None);
            }
        });
        let weak = Rc::downgrade(ctx);
        self.footer.settings.connect_clicked(move |_| {
            if let Some(ctx) = weak.upgrade() {
                ctx.show(Some("settings"), None);
            }
        });
    }

    /// Escape closes; Ctrl+1…9 and Ctrl+Tab move between tabs (one
    /// controller on the window).
    fn shortcuts(&self, ctx: &Rc<Ctx>) {
        let sc = gtk::ShortcutController::new();
        sc.set_scope(gtk::ShortcutScope::Managed);
        let add = |trigger: &str, f: Command| {
            let weak: Weak<Ctx> = Rc::downgrade(ctx);
            let action = gtk::CallbackAction::new(move |_, _| {
                if let Some(ctx) = weak.upgrade() {
                    f(&ctx);
                }
                glib::Propagation::Stop
            });
            if let Some(t) = gtk::ShortcutTrigger::parse_string(trigger) {
                sc.add_shortcut(gtk::Shortcut::new(Some(t), Some(action)));
            }
        };
        if self.mode.closes_on_escape() {
            add("Escape", Box::new(|ctx| ctx.hide()));
        } else {
            add("<Control>w", Box::new(|ctx| ctx.hide()));
        }
        for n in 1..=9usize {
            add(
                &format!("<Control>{n}"),
                Box::new(move |ctx| {
                    if let Some(id) = shell::tab_for_number(&ctx.tiles(), n) {
                        ctx.show(Some(id), None);
                    }
                }),
            );
        }
        let step = |backwards: bool| -> Command {
            Box::new(move |ctx| {
                if let Some(id) = shell::next_tab(&ctx.tiles(), &ctx.current_tab(), backwards) {
                    ctx.show(Some(id), None);
                }
            })
        };
        add("<Control>Tab", step(false));
        add("<Control><Shift>Tab", step(true));
        add("<Control><Shift>ISO_Left_Tab", step(true));
        self.win.add_controller(sc);
    }

    /// A popover closes when it loses keyboard focus (a click elsewhere),
    /// unless Keep open is on. Only a change from active to inactive counts:
    /// a layer surface may start inactive.
    fn close_on_focus_loss(&self, ctx: &Rc<Ctx>) {
        if self.mode != Mode::Popover {
            return;
        }
        let weak = Rc::downgrade(ctx);
        self.win.connect_is_active_notify(move |win| {
            let Some(ctx) = weak.upgrade() else { return };
            // A window being replaced is no longer ctx's, and its losing
            // focus closes nothing.
            let Some(w) = ctx.window().filter(|w| w.win == *win) else {
                return;
            };
            let active = win.is_active();
            let was = w.was_active.replace(active);
            if was && !active && !w.pinned.is_active() {
                ctx.hide();
            }
        });
    }

    /// The tiles for the daemon's capabilities and `tabs.hidden`.
    pub fn refresh_tiles(&self, ctx: &Rc<Ctx>) {
        let tiles = ctx.tiles();
        self.sidebar.set_tiles(&tiles, &self.current.borrow());
    }

    pub fn current(&self) -> String {
        self.current.borrow().clone()
    }

    /// Shows `tab` (a sidebar tab or `settings`).
    pub fn show_tab(&self, ctx: &Rc<Ctx>, tab: &str) {
        *self.current.borrow_mut() = tab.to_string();
        self.sidebar.set_active(tab);
        self.title.set_text(strings::tab_label(tab).unwrap_or(tab));
        self.pages.show(tab, ctx);
    }

    pub fn present(&self, ctx: &Rc<Ctx>) {
        if self.self_sub.borrow().is_none() {
            let footer = self.footer.clone();
            let sub = ctx.client.subscribe_params(
                stream::SELF,
                2000,
                serde_json::Value::Null,
                Rc::new(move |data| {
                    if let Ok(d) = serde_json::from_value::<SelfData>(data.clone()) {
                        footer.set_self(&d);
                    }
                }),
            );
            *self.self_sub.borrow_mut() = Some(sub);
        }
        self.win.present();
    }

    /// Hides the window and drops its pages and its `self` subscription.
    pub fn hide(&self) {
        self.win.set_visible(false);
        self.pages.clear();
        *self.self_sub.borrow_mut() = None;
        self.was_active.set(false);
    }

    pub fn is_visible(&self) -> bool {
        self.win.is_visible()
    }
}
