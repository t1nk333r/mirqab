//! The inspector (plan 006, decisions 4, 10 and 11): the side sheet of the
//! selected row. Top to bottom: icon, name and kind; the verdict; the
//! graph; CPU, memory, threads and processes from the row; for a group
//! tagged "idle but heavy", one line saying what that means; then what
//! `get apps.inspect {pid, start}` says, asked once per selection and on
//! the refresh button, never on a timer; Quit and Force quit at the foot.
//! When the pid holds another process than the row's by the time it is
//! asked, the sheet says the row is gone and forms no verdict (plan 006,
//! part 4, decision 9).
//!
//! A process row's graph follows the `process` stream live, through the
//! graph's own live rule (only while it is mapped). A group's graph is fed
//! the group's CPU from the `apps` frames every 2 s and holds no live
//! subscription: a group is many pids, and the live rule buys one.
//!
//! Quit and Force quit are the daemon's two-step flow: the first call asks
//! nothing of the daemon but a plan (`needs-confirm`), the dialog lists
//! it, and Go ahead sends the same args with `confirm` and the plan. The
//! interface repeats no refusal rule: a refusal is the daemon's message,
//! in words, with no dialog.

use crate::client::{Reply, Source};
use crate::tabs::apps::{icon_slot, set_app_icon};
use crate::widgets::confirm_dialog;
use crate::widgets::graph::{Graph, Live};
use crate::widgets::icon::icon_button;
use crate::widgets::named_box;
use adw::prelude::*;
use gtk::glib;
use mirqab_proto::stream::{self, Inspect};
use mirqab_proto::{Code, strings};
use mirqab_view::format;
use mirqab_view::graph::Scale;
use mirqab_view::icons;
use mirqab_view::tabs::apps::{self as model, Frame, Key, Outcome, Selection};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The targets of a Quit, by pid and start time, and how many frames still
/// listed them.
type Watch = Option<(Vec<(u32, u64)>, u32)>;

pub struct Inspector {
    pub root: gtk::Box,
    /// Itself, weakly, for the callbacks it hands out.
    me: Weak<Inspector>,
    source: Rc<dyn Source>,
    /// Whether the daemon serves the `process` stream.
    served: Rc<dyn Fn() -> bool>,
    image: gtk::Image,
    glyph: gtk::Label,
    title: gtk::Label,
    kind: gtk::Label,
    verdict: gtk::Label,
    pub graph: Graph,
    cpu: gtk::Label,
    memory: gtk::Label,
    threads: gtk::Label,
    procs: gtk::Label,
    /// `APPS_IDLE_HEAVY_LINE`, shown for a tagged group.
    pub idle: gtk::Label,
    details: gtk::Grid,
    pub buttons: gtk::Box,
    note: gtk::Label,
    still: gtk::Label,
    pub result: gtk::Label,
    warn: gtk::Label,
    quit: gtk::Button,
    force: gtk::Button,
    sel: RefCell<Option<Selection>>,
    /// `apps.inspect` said the process is another user's.
    limited: Cell<bool>,
    watch: RefCell<Watch>,
    /// Bumped per selection, so a late reply for an earlier one is dropped.
    asked: Cell<u64>,
    /// The dialog open now, if any (the widget tests answer it).
    dialog: RefCell<Option<adw::AlertDialog>>,
    /// An act's first call is out: one act in flight (review of PR #19).
    busy: Cell<bool>,
    /// Bumped when the sheet hides or the selection changes, so a plan
    /// reply for an act begun before is dropped: hiding frees Quit while
    /// that reply is out, and a second act's reply would open a second
    /// dialog over the first (review of PR #24).
    acts: Cell<u64>,
    /// The outcome of the last act on the selected row, for the page to
    /// take when the row goes (review of PR #19, B1).
    outcome: RefCell<Option<Outcome>>,
    /// Where an outcome goes whose row is no longer the selection.
    on_outcome: Rc<dyn Fn(Outcome)>,
}

/// What Go ahead redeems: captured when the dialog opened, so it sends
/// what the dialog showed (review of PR #19, S2).
struct Pending {
    force: bool,
    plan: String,
    key: Key,
    args: Value,
    name: String,
    targets: Vec<(u32, u64)>,
}

fn label(classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn now() -> i64 {
    glib::real_time() / 1_000_000
}

impl Inspector {
    /// Requests go to `source`; `served` says whether the daemon serves
    /// the `process` stream; `on_close` runs when the sheet's close button
    /// is pressed; `on_outcome` gets an act's outcome when its row is no
    /// longer the selection by the time the reply comes.
    pub fn new(
        source: Rc<dyn Source>,
        served: Rc<dyn Fn() -> bool>,
        on_close: Rc<dyn Fn()>,
        on_outcome: Rc<dyn Fn(Outcome)>,
    ) -> Rc<Inspector> {
        let root = named_box(gtk::Orientation::Vertical, gtk::AccessibleRole::Group);
        root.add_css_class("mirqab-apps-inspector");
        root.set_widget_name("apps-inspector");
        root.update_property(&[gtk::accessible::Property::Label(strings::APPS_INSPECTOR)]);

        let (image, glyph) = icon_slot();
        let title = label(&["title-4"]);
        title.set_hexpand(true);
        let kind = label(&["dim-label", "caption"]);
        let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
        names.set_hexpand(true);
        names.append(&title);
        names.append(&kind);
        let refresh = icon_button(icons::REFRESH, strings::APPS_REFRESH);
        let close = icon_button(icons::CLOSE, strings::APPS_CLOSE_INSPECTOR);
        close.connect_clicked(move |_| on_close());
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        head.add_css_class("mirqab-apps-inspector-head");
        head.append(&image);
        head.append(&glyph);
        head.append(&names);
        head.append(&refresh);
        head.append(&close);

        let verdict = label(&["mirqab-apps-verdict"]);
        verdict.set_widget_name("apps-verdict");
        let graph = Graph::new();
        graph.set_compact(true);
        graph.set_scale(Scale::Auto { floor: 100.0 });
        graph.set_hexpand(true);
        graph.set_range("2m");

        let stats = gtk::Grid::new();
        stats.add_css_class("mirqab-apps-stats");
        stats.set_column_homogeneous(true);
        let stat = |col: i32, name: &str| -> gtk::Label {
            let n = label(&["dim-label", "caption"]);
            n.set_text(name);
            let v = label(&["numeric"]);
            stats.attach(&n, col, 0, 1, 1);
            stats.attach(&v, col, 1, 1, 1);
            v
        };
        let cpu = stat(0, strings::APPS_CPU);
        let memory = stat(1, strings::APPS_MEMORY);
        let threads = stat(2, strings::APPS_THREADS);
        let procs = stat(3, strings::APPS_PROCESSES);

        let idle = label(&["dim-label"]);
        idle.set_text(strings::APPS_IDLE_HEAVY_LINE);
        idle.set_widget_name("apps-idle-heavy-line");
        idle.set_visible(false);

        let details = gtk::Grid::new();
        details.add_css_class("mirqab-apps-details");

        let quit = gtk::Button::with_label(strings::APPS_QUIT);
        quit.set_widget_name("apps-quit");
        let force = gtk::Button::with_label(strings::APPS_FORCE_QUIT);
        force.set_widget_name("apps-force-quit");
        force.add_css_class("destructive-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        buttons.add_css_class("mirqab-apps-buttons");
        buttons.set_halign(gtk::Align::End);
        buttons.append(&quit);
        buttons.append(&force);
        let note = label(&["dim-label"]);
        let still = label(&["warning"]);
        still.set_text(strings::APPS_STILL_RUNNING);
        let result = label(&[]);
        result.set_widget_name("apps-result");
        let warn = label(&["warning"]);
        warn.set_text(strings::APPS_NOT_RECORDED);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("mirqab-apps-inspector-body");
        for w in [
            verdict.upcast_ref::<gtk::Widget>(),
            graph.upcast_ref(),
            stats.upcast_ref(),
            idle.upcast_ref(),
            details.upcast_ref(),
        ] {
            body.append(w);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&body)
            .vexpand(true)
            .build();
        let foot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        foot.add_css_class("mirqab-apps-inspector-foot");
        for w in [
            note.upcast_ref::<gtk::Widget>(),
            result.upcast_ref(),
            warn.upcast_ref(),
            still.upcast_ref(),
            buttons.upcast_ref(),
        ] {
            foot.append(w);
        }
        root.append(&head);
        root.append(&scroll);
        root.append(&foot);

        let i = Rc::new_cyclic(|me| Inspector {
            root,
            me: me.clone(),
            source,
            served,
            image,
            glyph,
            title,
            kind,
            verdict,
            graph,
            cpu,
            memory,
            threads,
            procs,
            idle,
            details,
            buttons,
            note,
            still,
            result,
            warn,
            quit: quit.clone(),
            force: force.clone(),
            sel: RefCell::new(None),
            limited: Cell::new(false),
            watch: RefCell::new(None),
            asked: Cell::new(0),
            dialog: RefCell::new(None),
            busy: Cell::new(false),
            acts: Cell::new(0),
            outcome: RefCell::new(None),
            on_outcome,
        });
        i.clear_messages();
        let weak = Rc::downgrade(&i);
        refresh.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                i.inspect();
            }
        });
        let weak = Rc::downgrade(&i);
        quit.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                i.act(false);
            }
        });
        let weak = Rc::downgrade(&i);
        force.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                i.act(true);
            }
        });
        // The sheet off screen (the page unmapped, a tab switch, the window
        // hid): the dialog goes with it. It sits in the window's dialog
        // host, so it would outlive a dropped page, and its Go ahead would
        // find no inspector and send nothing (audit).
        let weak = Rc::downgrade(&i);
        i.root.connect_unmap(move |_| {
            if let Some(i) = weak.upgrade() {
                i.hidden();
            }
        });
        i
    }

    /// The sheet went off screen: an open dialog is closed, sending
    /// nothing, and Quit and Force quit are free again.
    pub fn hidden(&self) {
        self.acts.set(self.acts.get() + 1);
        self.close_dialog();
    }

    fn clear_messages(&self) {
        self.result.set_text("");
        self.result.set_visible(false);
        self.warn.set_visible(false);
        self.still.set_visible(false);
        *self.watch.borrow_mut() = None;
    }

    fn say(&self, lines: &[String]) {
        self.result.set_text(&lines.join("\n"));
        self.result.set_visible(!lines.is_empty());
    }

    /// The selection now, from the frame. `tick` is true for a new frame
    /// (it feeds a group's graph and the "still running" count), false for
    /// a click.
    pub fn show(&self, sel: Option<Selection>, frame: &Frame, tick: bool) {
        let Some(sel) = sel else {
            *self.sel.borrow_mut() = None;
            self.graph.set_live(None);
            // A dialog for a row that is gone would redeem nothing the
            // user can see (review of PR #19, S2).
            self.close_dialog();
            return;
        };
        let changed = self.sel.borrow().as_ref().map(|s| s.key) != Some(sel.key);
        *self.sel.borrow_mut() = Some(sel.clone());
        if changed {
            self.selected(&sel);
        }
        set_app_icon(&self.image, &self.glyph, sel.icon.as_deref(), sel.glyph);
        self.title.set_text(&sel.name);
        self.kind.set_text(sel.kind);
        self.cpu.set_text(&sel.cpu_text);
        self.memory.set_text(&sel.memory);
        self.threads.set_text(&sel.threads);
        self.procs.set_text(&sel.procs);
        self.idle.set_visible(sel.idle_heavy);
        self.update_buttons();
        let live = sel.process.is_some() && self.process_served();
        if !live && (tick || changed) {
            self.graph.push_live(now(), sel.cpu.unwrap_or(0.0));
        }
        if tick {
            let mut watch = self.watch.borrow_mut();
            let mut drop_watch = false;
            if let Some((targets, n)) = watch.as_mut() {
                if frame.any_listed(targets) {
                    *n += 1;
                    self.still.set_visible(*n >= 2);
                } else {
                    drop_watch = true;
                }
            }
            if drop_watch {
                *watch = None;
                self.still.set_visible(false);
            }
        }
    }

    fn process_served(&self) -> bool {
        (self.served)()
    }

    /// The confirmation open now, if any.
    pub fn dialog(&self) -> Option<adw::AlertDialog> {
        self.dialog.borrow().clone()
    }

    /// Whether the "not recorded" warning shows.
    pub fn warned(&self) -> bool {
        self.warn.is_visible()
    }

    /// What a Quit's "still running" line watches.
    pub fn watching(&self) -> Vec<(u32, u64)> {
        self.watch
            .borrow()
            .as_ref()
            .map(|(t, _)| t.clone())
            .unwrap_or_default()
    }

    /// The last act's outcome if it was on `key`; taken once.
    pub fn take_outcome(&self, key: Key) -> Option<Outcome> {
        let mut o = self.outcome.borrow_mut();
        if o.as_ref().is_some_and(|o| o.key == key) {
            o.take()
        } else {
            None
        }
    }

    /// Closes an open dialog; nothing is sent for it.
    fn close_dialog(&self) {
        let d = self.dialog.borrow_mut().take();
        if let Some(d) = d {
            d.force_close();
        }
        self.set_busy(false);
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.quit.set_sensitive(!busy);
        self.force.set_sensitive(!busy);
    }

    /// A new row was selected: its graph, its messages, its inspection.
    fn selected(&self, sel: &Selection) {
        self.acts.set(self.acts.get() + 1);
        self.close_dialog();
        *self.outcome.borrow_mut() = None;
        self.clear_messages();
        self.limited.set(false);
        self.verdict.set_text("");
        while let Some(c) = self.details.first_child() {
            self.details.remove(&c);
        }
        let (name, live) = match sel.process {
            Some((pid, start)) if self.process_served() => (
                strings::APPS_GRAPH_PROCESS,
                Some(Live {
                    source: Rc::clone(&self.source),
                    stream: stream::PROCESS.to_string(),
                    params: json!({"pid": pid, "start": start}),
                    extract: Rc::new(|v: &Value| {
                        if v["gone"] == Value::Bool(true) {
                            return None;
                        }
                        v["cpu"].as_f64()
                    }),
                }),
            ),
            Some(_) => (strings::APPS_GRAPH_PROCESS, None),
            None => (strings::APPS_GRAPH_GROUP, None),
        };
        self.graph.set_name(name, Rc::new(format::percent));
        self.graph.set_live(live);
        self.inspect();
    }

    /// `get apps.inspect` for the selection's pid and start time, once.
    fn inspect(&self) {
        let Some(params) = self
            .sel
            .borrow()
            .as_ref()
            .and_then(|s| s.inspect_params.clone())
        else {
            return;
        };
        let n = self.asked.get() + 1;
        self.asked.set(n);
        let weak = self.weak();
        let asked = params.clone();
        let reply: Reply = Box::new(move |r| {
            let Some(i) = weak.upgrade() else { return };
            if i.asked.get() != n {
                return;
            }
            match r {
                Err(e) if model::inspect_gone(&e, &asked) => i.gone(),
                r => match r
                    .ok()
                    .and_then(|v| serde_json::from_value::<Inspect>(v).ok())
                {
                    Some(info) => i.inspected(&info),
                    None => i.verdict.set_text(""),
                },
            }
        });
        self.source.get(stream::APPS_INSPECT, params, reply);
    }

    /// The row's process is no longer at its pid: no verdict, no details,
    /// and the sheet says so until the next frame drops the row.
    fn gone(&self) {
        self.verdict.set_text("");
        while let Some(c) = self.details.first_child() {
            self.details.remove(&c);
        }
        self.say(&[strings::APPS_ROW_GONE.to_string()]);
    }

    fn weak(&self) -> Weak<Inspector> {
        self.me.clone()
    }

    fn inspected(&self, info: &Inspect) {
        self.verdict.set_text(&strings::plain(&info.verdict.text));
        self.limited.set(info.limited);
        while let Some(c) = self.details.first_child() {
            self.details.remove(&c);
        }
        for (row, (name, value)) in model::inspect_rows(info, false).into_iter().enumerate() {
            let n = label(&["dim-label", "caption"]);
            n.set_text(name);
            n.set_valign(gtk::Align::Start);
            let v = label(&[]);
            v.set_text(&value);
            v.set_hexpand(true);
            v.set_selectable(true);
            self.details.attach(&n, 0, row as i32, 1, 1);
            self.details.attach(&v, 1, row as i32, 1, 1);
        }
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let sel = self.sel.borrow();
        let Some(sel) = sel.as_ref() else { return };
        let not_yours = sel.not_yours || self.limited.get();
        self.buttons.set_visible(!not_yours);
        let note = if not_yours {
            Some(strings::APPS_NOT_YOURS)
        } else if sel.holds_self {
            Some(strings::APPS_QUIT_SELF)
        } else {
            None
        };
        self.note.set_text(note.unwrap_or_default());
        self.note.set_visible(note.is_some());
    }

    /// Quit (`force` false) or Force quit: the first call, which only
    /// plans.
    fn act(&self, force: bool) {
        // One act at a time: a second click, or a click with the dialog
        // open, would stack a second plan under the first (review of
        // PR #19, S3).
        if self.busy.get() || self.dialog.borrow().is_some() {
            return;
        }
        let Some(sel) = self.sel.borrow().clone() else {
            return;
        };
        self.clear_messages();
        self.set_busy(true);
        let name = if force {
            stream::APPS_KILL
        } else {
            stream::APPS_QUIT
        };
        let weak = self.weak();
        let key = sel.key;
        let act = self.acts.get();
        let reply: Reply = Box::new(move |r| {
            let Some(i) = weak.upgrade() else { return };
            if i.acts.get() != act || i.sel.borrow().as_ref().map(|s| s.key) != Some(key) {
                return;
            }
            i.set_busy(false);
            match r {
                Err(e) if e.code == Code::NeedsConfirm => {
                    let data = e.data.unwrap_or(Value::Null);
                    i.confirm(force, &data);
                }
                Err(e) => i.say(&[strings::plain(&e.message).into_owned()]),
                Ok(_) => i.say(&[strings::bad_daemon_answer(strings::QUIT_NO_CONFIRM_ASKED)]),
            }
        });
        self.source
            .request("action", json!({"name": name, "args": sel.args}), reply);
    }

    /// The dialog for a plan; Go ahead redeems it.
    fn confirm(&self, force: bool, data: &Value) {
        // The plan came back after the sheet left the screen (another tab,
        // the window hid): a dialog now would sit over whatever shows
        // instead. Nothing is asked and nothing is sent.
        if !self.root.is_mapped() {
            return;
        }
        let Some(sel) = self.sel.borrow().clone() else {
            return;
        };
        // A process row's pid now names another process: ask nothing (D5).
        if let Key::Process { pid, start } = sel.key
            && model::start_differs(data, pid, start)
        {
            self.say(&[strings::APPS_ROW_GONE.to_string()]);
            return;
        }
        let Some(plan) = data["plan"].as_str().map(str::to_string) else {
            self.say(&[strings::bad_daemon_answer(strings::QUIT_NO_PLAN)]);
            return;
        };
        let heading = if force {
            strings::FORCE_QUIT_WILL
        } else {
            strings::QUIT_WILL
        };
        let pending = RefCell::new(Some(Pending {
            force,
            plan,
            key: sel.key,
            args: sel.args.clone(),
            name: sel.name.clone(),
            targets: sel.targets.clone(),
        }));
        let d = confirm_dialog::confirm_dialog_lines(heading, &model::confirm_lines(data));
        d.set_widget_name("apps-dialog");
        let weak = self.weak();
        d.connect_response(None, move |d, response| {
            let Some(i) = weak.upgrade() else { return };
            // Only the dialog open now answers: one closed for a row that
            // went, or for another selection, sends nothing.
            let mine = i.dialog.borrow().as_ref() == Some(d);
            if mine {
                *i.dialog.borrow_mut() = None;
            }
            if !mine || response != confirm_dialog::CONFIRM {
                return;
            }
            if let Some(p) = pending.borrow_mut().take() {
                i.redeem(p);
            }
        });
        *self.dialog.borrow_mut() = Some(d.clone());
        d.present(Some(&self.root));
    }

    /// The second call: the args the dialog was opened for, `confirm`
    /// and the plan. The outcome is shown in the sheet when its row is
    /// still the selection, and kept for the page either way.
    fn redeem(&self, p: Pending) {
        let action = if p.force {
            stream::APPS_KILL
        } else {
            stream::APPS_QUIT
        };
        let weak = self.weak();
        let Pending {
            force,
            plan,
            key,
            args,
            name,
            targets,
        } = p;
        let request = json!({"name": action, "args": args, "confirm": true, "plan": plan});
        let reply: Reply = Box::new(move |r| {
            let Some(i) = weak.upgrade() else { return };
            let o = model::outcome(key, &name, &r);
            if i.sel.borrow().as_ref().map(|s| s.key) != Some(key) {
                (i.on_outcome)(o);
                return;
            }
            i.say(&o.lines);
            i.warn.set_visible(o.warn);
            if let (Ok(v), false) = (&r, force) {
                *i.watch.borrow_mut() = Some((model::watched(v, &targets), 0));
            }
            *i.outcome.borrow_mut() = Some(o);
        });
        self.source.request("action", request, reply);
    }
}
