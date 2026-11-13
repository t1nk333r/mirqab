//! `--selftest <tab>` (architecture §7.5, layer 3): window mode, build the
//! page, wait for the first event of the footer's `self` stream (or 5 s),
//! print the widget tree as JSON on stdout and exit 0; exit 1 with the
//! reason on stderr when nothing arrived.

use crate::app::Ctx;
use gtk::glib;
use gtk::prelude::*;
use mirqab_proto::{stream, strings};
use mirqab_view::config::Mode;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::Rc;
use std::time::Duration;

pub fn run(ctx: &Rc<Ctx>, tab: &str) {
    ctx.show(Some(tab), Some(Mode::Window));
    let done = Rc::new(Cell::new(false));
    let keep: Rc<RefCell<Option<crate::client::Subscription>>> = Rc::new(RefCell::new(None));
    let weak = Rc::downgrade(ctx);
    let d = Rc::clone(&done);
    let sub = ctx.client.subscribe_params(
        stream::SELF,
        2000,
        Value::Null,
        Rc::new(move |_| {
            if d.replace(true) {
                return;
            }
            let weak = weak.clone();
            // After this event's other handlers (the footer's) have run.
            glib::idle_add_local_once(move || {
                if let Some(ctx) = weak.upgrade() {
                    finish(&ctx, 0, None);
                }
            });
        }),
    );
    *keep.borrow_mut() = Some(sub);
    let weak = Rc::downgrade(ctx);
    let tab = tab.to_string();
    // mirqab:ui-timeout
    glib::timeout_add_local_once(Duration::from_secs(5), move || {
        let _keep = keep;
        if !done.get()
            && let Some(ctx) = weak.upgrade()
        {
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
/// accessible role, visibility.
pub fn dump(w: &gtk::Widget) -> Value {
    let mut node = json!({
        "type": w.type_().name(),
        "classes": w.css_classes().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "visible": w.is_visible(),
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
        children.push(dump(&child));
        c = child.next_sibling();
    }
    if !children.is_empty() {
        node["children"] = Value::Array(children);
    }
    node
}
