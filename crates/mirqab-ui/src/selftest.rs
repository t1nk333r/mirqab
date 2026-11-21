//! `--selftest <tab>` (architecture §7.5, layer 3): the config's mode (a
//! popover becomes a window without layer shell, as under Broadway), build the
//! page once the config is known, wait for the first event of the footer's
//! `self` stream and for the page's first data (or 5 s), print the widget
//! tree as JSON on stdout and exit 0; exit 1 with the reason on stderr when
//! nothing arrived.
//!
//! `--do STEPS` (accepted only with `MIRQAB_TEST=1`; plan 006, decision
//! 15): steps separated by `;`, run in order after the page's first data,
//! the tree printed after the last. `type NAME TEXT` sets an editable's
//! text; `activate NAME` toggles a switch, or activates the widget or the
//! nearest parent that can be (a `ListView` row's own box cannot, and its
//! list item activates its position: checked under Broadway); `respond ID` answers the open dialog; `wait NAME` waits for the widget;
//! `pause MS` lets the main loop run. A step that names a widget waits for
//! a mapped widget of that name for up to 5 s. NAME is the widget name the
//! dump prints.

use crate::app::Ctx;
use adw::prelude::*;
use gtk::glib;
use mirqab_proto::{stream, strings};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;

pub fn run(ctx: &Rc<Ctx>, tab: &str, steps: Option<&str>) {
    // Where the steps are consumed too, not only where they are parsed
    // (review of PR #19, S1).
    if steps.is_some() && !crate::app::test_env() {
        eprintln!("{}", strings::UI_DO_TEST_ONLY);
        return;
    }
    let steps: Rc<Vec<String>> = Rc::new(
        steps
            .unwrap_or_default()
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    );
    let started = Rc::new(Cell::new(false));
    // Once the config is in, as a real open: Home built from the defaults
    // was empty and ready before the user's pins arrived (plan 003).
    ctx.show_when_ready(Some(tab), None);
    let done = Rc::new(Cell::new(false));
    let keep: Rc<RefCell<Option<crate::client::Subscription>>> = Rc::new(RefCell::new(None));
    let weak = Rc::downgrade(ctx);
    let d = Rc::clone(&done);
    let started_by_event = Rc::clone(&started);
    // A tab `show` opens: `Opts::refusal` turned any other away.
    let tab_name = tab.to_string();
    let sub = ctx.client.subscribe_params(
        stream::SELF,
        2000,
        Value::Null,
        Rc::new(move |_| {
            if d.replace(true) {
                return;
            }
            let weak = weak.clone();
            let tab = tab_name.clone();
            let steps = Rc::clone(&steps);
            let started = Rc::clone(&started_by_event);
            // After this event's other handlers (the footer's) have run, and
            // once the page has drawn its first data.
            glib::idle_add_local_once(move || {
                let Some(ctx) = weak.upgrade() else { return };
                let w = Rc::downgrade(&ctx);
                ctx.when_ready(&tab, move || {
                    if let Some(ctx) = w.upgrade() {
                        started.set(true);
                        step(&ctx, steps, 0);
                    }
                });
            });
        }),
    );
    *keep.borrow_mut() = Some(sub);
    let weak = Rc::downgrade(ctx);
    let tab = tab.to_string();
    // mirqab:ui-timeout
    glib::timeout_add_local_once(Duration::from_secs(5), move || {
        // finish() exits the process, so still running here, with no step
        // started, means the event or the page's first data never came.
        // Each step has its own limit.
        let _ = done;
        let _keep = keep;
        if started.get() {
            return;
        }
        if let Some(ctx) = weak.upgrade() {
            finish(&ctx, 1, Some(strings::selftest_no_event(&tab)));
        }
    });
}

/// How long a step waits for its widget.
const STEP_WAIT: Duration = Duration::from_secs(5);
/// How often it looks again meanwhile.
const STEP_POLL: Duration = Duration::from_millis(50);

/// Runs step `i` and then the rest; prints the tree after the last.
fn step(ctx: &Rc<Ctx>, steps: Rc<Vec<String>>, i: usize) {
    let Some(text) = steps.get(i).cloned() else {
        finish(ctx, 0, None);
        return;
    };
    let mut words = text.splitn(3, ' ');
    let verb = words.next().unwrap_or_default().to_string();
    let name = words.next().unwrap_or_default().to_string();
    let rest = words.next().unwrap_or_default().to_string();
    let fail = {
        let text = text.clone();
        move |ctx: &Rc<Ctx>, why: &str| {
            finish(ctx, 1, Some(strings::selftest_step_failed(&text, why)));
        }
    };
    let next = {
        let weak = Rc::downgrade(ctx);
        let steps = Rc::clone(&steps);
        move || {
            let steps = Rc::clone(&steps);
            let weak = weak.clone();
            // After this step's handlers and a frame have run.
            glib::idle_add_local_once(move || {
                if let Some(ctx) = weak.upgrade() {
                    step(&ctx, steps, i + 1);
                }
            });
        }
    };
    match verb.as_str() {
        "pause" => {
            let Ok(ms) = name.parse::<u64>() else {
                return fail(ctx, strings::SELFTEST_BAD_STEP);
            };
            // mirqab:ui-timeout
            glib::timeout_add_local_once(Duration::from_millis(ms), next);
        }
        "respond" => {
            let dialog = ctx
                .window()
                .and_then(|w| w.win.visible_dialog())
                .and_downcast::<adw::AlertDialog>();
            let Some(d) = dialog else {
                return fail(ctx, strings::SELFTEST_NO_DIALOG);
            };
            d.emit_by_name::<()>("response", &[&name]);
            d.force_close();
            next();
        }
        "type" | "activate" | "wait" => {
            let weak = Rc::downgrade(ctx);
            let deadline = std::time::Instant::now() + STEP_WAIT;
            with_widget(
                weak,
                name,
                deadline,
                move |ctx, w| match verb.as_str() {
                    "type" => match w.dynamic_cast_ref::<gtk::Editable>() {
                        Some(e) => {
                            e.set_text(&rest);
                            next();
                        }
                        None => fail(ctx, strings::SELFTEST_NOT_EDITABLE),
                    },
                    "activate" => {
                        // A switch animates its toggle; the step wants the
                        // state now.
                        if let Some(sw) = w.downcast_ref::<gtk::Switch>() {
                            sw.set_active(!sw.is_active());
                            next();
                            return;
                        }
                        let mut at = Some(w);
                        while let Some(x) = at {
                            if x.activate() {
                                next();
                                return;
                            }
                            at = x.parent();
                        }
                        fail(ctx, strings::SELFTEST_NOT_ACTIVATABLE);
                    }
                    _ => next(),
                },
                fail_owned(text.clone()),
            );
        }
        _ => fail(ctx, strings::SELFTEST_BAD_STEP),
    }
}

fn fail_owned(text: String) -> impl Fn(&Rc<Ctx>) + 'static {
    move |ctx| {
        finish(
            ctx,
            1,
            Some(strings::selftest_step_failed(
                &text,
                strings::SELFTEST_NO_WIDGET,
            )),
        )
    }
}

/// Calls `found` with the first mapped widget named `name`, looking again
/// every 50 ms until `deadline`, then `missing`.
fn with_widget(
    weak: std::rc::Weak<Ctx>,
    name: String,
    deadline: std::time::Instant,
    found: impl FnOnce(&Rc<Ctx>, gtk::Widget) + 'static,
    missing: impl Fn(&Rc<Ctx>) + 'static,
) {
    let Some(ctx) = weak.upgrade() else { return };
    let hit = ctx.window().and_then(|w| find(w.win.upcast_ref(), &name));
    match hit {
        Some(w) => found(&ctx, w),
        None if std::time::Instant::now() >= deadline => missing(&ctx),
        None => {
            // mirqab:ui-timeout
            glib::timeout_add_local_once(STEP_POLL, move || {
                with_widget(weak, name, deadline, found, missing);
            });
        }
    }
}

/// The first mapped widget named `name` under `w`, depth first.
fn find(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if w.widget_name() == name && w.is_mapped() {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(hit) = find(&child, name) {
            return Some(hit);
        }
        c = child.next_sibling();
    }
    None
}

fn finish(ctx: &Rc<Ctx>, code: i32, error: Option<String>) {
    if let Some(e) = error {
        eprintln!("{e}");
    } else if let Some(w) = ctx.window() {
        let out = json!({
            "pages": w.pages.built(),
            "tree": dump(w.win.upcast_ref()),
        });
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{out}");
        let _ = stdout.flush();
    }
    std::process::exit(code);
}

/// A widget and its children: type, style classes, name, text, tooltip,
/// accessible role, visibility. `visible` is false under a hidden parent
/// too (plan 003: Home's hidden empty state counted as shown).
pub fn dump(w: &gtk::Widget) -> Value {
    dump_in(w, true)
}

fn dump_in(w: &gtk::Widget, parent_visible: bool) -> Value {
    let visible = parent_visible && w.is_visible();
    let mut node = json!({
        "type": w.type_().name(),
        "classes": w.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "visible": visible,
        "role": format!("{:?}", w.accessible_role()),
    });
    let name = w.widget_name();
    if name != w.type_().name() {
        node["name"] = json!(name.as_str());
    }
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        node["text"] = json!(l.text().as_str());
    }
    if let Some(t) = w.tooltip_text() {
        node["tooltip"] = json!(t.as_str());
    }
    let mut children = Vec::new();
    let mut c = w.first_child();
    while let Some(child) = c {
        children.push(dump_in(&child, visible));
        c = child.next_sibling();
    }
    if !children.is_empty() {
        node["children"] = Value::Array(children);
    }
    node
}
