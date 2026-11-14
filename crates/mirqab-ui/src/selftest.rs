//! `--selftest <tab>` (architecture §7.5, layer 3): the config's mode (a
//! popover becomes a window without layer shell, as under Broadway), build the
//! page once the config is known, wait for the first event of the footer's
//! `self` stream and for the page's first data (or 5 s), print the widget
//! tree as JSON on stdout and exit 0; exit 1 with the reason on stderr when
//! nothing arrived.

use crate::app::Ctx;
use gtk::glib;
use gtk::prelude::*;
use mirqab_proto::{stream, strings};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;

pub fn run(ctx: &Rc<Ctx>, tab: &str) {
    // Once the config is in, as a real open: Home built from the defaults
    // was empty and ready before the user's pins arrived (plan 003).
    ctx.show_when_ready(Some(tab), None);
    let done = Rc::new(Cell::new(false));
    let keep: Rc<RefCell<Option<crate::client::Subscription>>> = Rc::new(RefCell::new(None));
    let weak = Rc::downgrade(ctx);
    let d = Rc::clone(&done);
    // The page `show` falls back to for an unknown tab.
    let tab_name = if tab == "settings" || mirqab_view::shell::find(tab).is_some() {
        tab.to_string()
    } else {
        mirqab_view::shell::DEFAULT_TAB.to_string()
    };
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
            // After this event's other handlers (the footer's) have run, and
            // once the page has drawn its first data.
            glib::idle_add_local_once(move || {
                let Some(ctx) = weak.upgrade() else { return };
                let w = Rc::downgrade(&ctx);
                ctx.when_ready(&tab, move || {
                    if let Some(ctx) = w.upgrade() {
                        finish(&ctx, 0, None);
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
        // finish() exits the process, so still running here means the
        // event or the page's first data never came.
        let _keep = keep;
        let _ = done;
        if let Some(ctx) = weak.upgrade() {
            finish(&ctx, 1, Some(strings::selftest_no_event(&tab)));
        }
    });
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
