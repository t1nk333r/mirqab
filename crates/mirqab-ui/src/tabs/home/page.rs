//! The Home page (plan 003): the pinned cards on two columns, wide cards
//! across both. It subscribes to the union of its cards' streams, and only
//! while it is on screen; a config change rebuilds the grid and keeps the
//! card widgets that stay pinned, moved in place so their graphs stay
//! mapped and keep their live history.

use crate::app::Ctx;
use crate::tabs::home::pinned_card::PinnedCard;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::cards;
use mirqab_view::tabs::home;
use serde_json::Value;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

struct Page {
    ctx: Weak<Ctx>,
    grid: gtk::Grid,
    empty: gtk::Box,
    cards: RefCell<Vec<PinnedCard>>,
    /// The streams of the current cards, subscribed while mapped.
    streams: RefCell<Vec<&'static str>>,
    subs: RefCell<Vec<Box<dyn Any>>>,
    mapped: Cell<bool>,
    first: Cell<bool>,
}

/// Puts `items` on `grid` at (row, column, span), with the fewest changes:
/// a child not in `items` is removed, a new one attached, and one already
/// there moved through its layout child. Moving does not unparent, so the
/// widget stays mapped (review of PR #9: removing and re-attaching every
/// card unmapped each graph and cleared its live history).
pub fn place(grid: &gtk::Grid, items: &[(gtk::Widget, (u32, u32, u32))]) {
    let mut c = grid.first_child();
    while let Some(child) = c {
        c = child.next_sibling();
        if !items.iter().any(|(w, _)| *w == child) {
            grid.remove(&child);
        }
    }
    let layout = grid.layout_manager();
    for (w, (row, col, span)) in items {
        if w.parent().as_ref() == Some(grid.upcast_ref()) {
            if let Some(lc) = layout
                .as_ref()
                .map(|l| l.layout_child(w))
                .and_then(|lc| lc.downcast::<gtk::GridLayoutChild>().ok())
            {
                lc.set_row(*row as i32);
                lc.set_column(*col as i32);
                lc.set_column_span(*span as i32);
            }
        } else {
            grid.attach(w, *col as i32, *row as i32, *span as i32, 1);
        }
    }
}

impl Page {
    /// The cards to show: the pinned ones the daemon serves. Before the
    /// daemon has said what it serves, every known pinned card.
    fn wanted(ctx: &Ctx) -> Vec<&'static cards::Card> {
        let pinned = ctx.config.borrow().pinned.clone();
        let caps = ctx.client.capabilities();
        if caps.is_empty() {
            cards::pinned(&pinned)
        } else {
            home::shown(&pinned, &caps)
        }
    }

    fn rebuild(self: &Rc<Self>) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let wanted = Page::wanted(&ctx);
        let ids: Vec<&'static str> = wanted.iter().map(|c| c.id).collect();
        let same = {
            let have = self.cards.borrow();
            have.len() == ids.len() && have.iter().zip(&ids).all(|(c, id)| c.id == *id)
        };
        if !same {
            let mut old = std::mem::take(&mut *self.cards.borrow_mut());
            let mut built = Vec::new();
            for id in &ids {
                let card = match old.iter().position(|c| c.id == *id) {
                    Some(i) => Some(old.remove(i)),
                    None => PinnedCard::new(&ctx, id),
                };
                built.extend(card);
            }
            let shown: Vec<&cards::Card> = built.iter().filter_map(|c| cards::find(c.id)).collect();
            let items: Vec<(gtk::Widget, (u32, u32, u32))> = built
                .iter()
                .zip(home::layout(&shown))
                .map(|(c, at)| {
                    let w = c.widget.card().root.clone();
                    w.set_hexpand(true);
                    (w.upcast(), at)
                })
                .collect();
            place(&self.grid, &items);
            *self.cards.borrow_mut() = built;
        }
        let empty = self.cards.borrow().is_empty();
        self.empty.set_visible(empty);
        self.grid.set_visible(!empty);
        let streams = home::streams(&wanted);
        if *self.streams.borrow() != streams {
            *self.streams.borrow_mut() = streams;
            if self.mapped.get() {
                self.subscribe();
            }
        }
        if empty {
            self.ready();
        }
    }

    fn ready(&self) {
        if !self.first.replace(true)
            && let Some(ctx) = self.ctx.upgrade()
        {
            ctx.page_ready("home");
        }
    }

    /// One subscription per stream, at 2 s; each event goes to every card,
    /// and a card reads only its own streams.
    fn subscribe(self: &Rc<Self>) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let mut subs = Vec::new();
        for s in self.streams.borrow().iter().copied() {
            let weak: Weak<Page> = Rc::downgrade(self);
            subs.push(crate::client::Source::subscribe(
                &ctx.client,
                s,
                2000,
                Rc::new(move |data: &Value| {
                    let Some(page) = weak.upgrade() else { return };
                    for c in page.cards.borrow().iter() {
                        c.widget.update(s, data);
                    }
                    page.ready();
                }),
            ));
        }
        *self.subs.borrow_mut() = subs;
    }
}

pub fn build(ctx: &Rc<Ctx>) -> gtk::Widget {
    let grid = gtk::Grid::new();
    grid.add_css_class("mirqab-home-grid");
    grid.set_column_homogeneous(true);

    let empty = gtk::Box::new(gtk::Orientation::Vertical, 0);
    empty.add_css_class("mirqab-card-body");
    empty.set_valign(gtk::Align::Center);
    empty.set_vexpand(true);
    let hint = gtk::Label::new(Some(strings::HOME_EMPTY));
    hint.add_css_class("dim-label");
    hint.set_wrap(true);
    let go = gtk::Button::with_label(strings::HOME_GO_SYSTEM);
    go.set_halign(gtk::Align::Center);
    let weak = Rc::downgrade(ctx);
    go.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            ctx.show(Some("system"), None);
        }
    });
    empty.append(&hint);
    empty.append(&go);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("mirqab-page-column");
    column.append(&grid);
    column.append(&empty);
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&column)
        .vexpand(true)
        .build();
    root.add_css_class("mirqab-home");

    let page = Rc::new(Page {
        ctx: Rc::downgrade(ctx),
        grid,
        empty,
        cards: RefCell::new(Vec::new()),
        streams: RefCell::new(Vec::new()),
        subs: RefCell::new(Vec::new()),
        mapped: Cell::new(false),
        first: Cell::new(false),
    });
    page.rebuild();

    let weak = Rc::downgrade(&page);
    ctx.on_config(move || match weak.upgrade() {
        Some(p) => {
            p.rebuild();
            true
        }
        None => false,
    });
    let p = Rc::clone(&page);
    root.connect_map(move |_| {
        p.mapped.set(true);
        p.subscribe();
    });
    let p = Rc::clone(&page);
    root.connect_unmap(move |_| {
        p.mapped.set(false);
        p.subs.borrow_mut().clear();
    });
    root.upcast()
}
