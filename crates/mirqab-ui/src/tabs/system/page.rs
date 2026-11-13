//! The System page (plan 002): its cards top to bottom, fed by one
//! `system` subscription held while the page is on screen. The graphs take
//! their own live subscription at `2m` (widgets/graph.rs). The cards come
//! from the table in `tabs/mod.rs`, the same ones Home pins (plan 003).

use crate::app::Ctx;
use crate::tabs::{self, CardWidget};
use gtk::prelude::*;
use mirqab_proto::stream;
use mirqab_view::tabs::system::CARDS;
use serde_json::Value;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

struct Page {
    ctx: Weak<Ctx>,
    cards: Vec<Rc<dyn CardWidget>>,
    sub: RefCell<Option<Box<dyn Any>>>,
    first: Cell<bool>,
}

impl Page {
    fn update(&self, data: &Value) {
        for c in &self.cards {
            c.update(stream::SYSTEM, data);
        }
        if !self.first.replace(true)
            && let Some(ctx) = self.ctx.upgrade()
        {
            ctx.page_ready("system");
        }
    }
}

pub fn build(ctx: &Rc<Ctx>) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("mirqab-page-column");
    let cards: Vec<Rc<dyn CardWidget>> = CARDS
        .iter()
        .filter_map(|c| tabs::card(c.id, ctx, false))
        .collect();
    for c in &cards {
        column.append(&c.card().root);
    }
    let page = Rc::new(Page {
        ctx: Rc::downgrade(ctx),
        cards,
        sub: RefCell::new(None),
        first: Cell::new(false),
    });
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&column)
        .vexpand(true)
        .build();
    root.add_css_class("mirqab-system");

    // The numbers subscribe while the page is on screen, at 2 s; only the
    // graphs go live (architecture §5.3).
    let p = Rc::clone(&page);
    let weak_ctx = Rc::downgrade(ctx);
    root.connect_map(move |_| {
        let Some(ctx) = weak_ctx.upgrade() else {
            return;
        };
        let weak: Weak<Page> = Rc::downgrade(&p);
        let sub = crate::client::Source::subscribe(
            &ctx.client,
            stream::SYSTEM,
            2000,
            Rc::new(move |data: &Value| {
                if let Some(page) = weak.upgrade() {
                    page.update(data);
                }
            }),
        );
        *p.sub.borrow_mut() = Some(sub);
    });
    let p = Rc::clone(&page);
    root.connect_unmap(move |_| {
        *p.sub.borrow_mut() = None;
    });
    root.upcast()
}
