//! A card with a graph and its range picker (plan 002): `2m` follows the
//! live stream through the graph's own live subscription; a stored range is
//! fetched with `history.query` on selection and then once a minute while
//! the card is on screen.

use crate::app::Ctx;
use crate::client::Source;
use crate::widgets::card::Card;
use crate::widgets::graph::{Format, Graph, Live};
use crate::widgets::range_picker::RangePicker;
use gtk::glib;
use gtk::prelude::*;
use mirqab_proto::stream;
use mirqab_view::tabs::system::{GraphSpec, history_points};
use serde_json::json;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub struct GraphCard {
    pub card: Card,
    pub graph: Graph,
    ctx: Weak<Ctx>,
    spec: GraphSpec,
    range: RefCell<String>,
    timer: RefCell<Option<glib::SourceId>>,
}

/// Adds the pin control for `card_id`, toggling it in `home.pinned`.
pub fn pinnable(card: &Card, ctx: &Rc<Ctx>, card_id: &'static str) {
    let pinned = ctx.config.borrow().pinned.iter().any(|p| p == card_id);
    let weak = Rc::downgrade(ctx);
    card.set_pinnable(pinned, move |on| {
        let Some(ctx) = weak.upgrade() else { return };
        let mut list = ctx.config.borrow().pinned.clone();
        list.retain(|p| p != card_id);
        if on {
            list.push(card_id.to_string());
        }
        ctx.set_setting("home.pinned", json!(list));
    });
}

impl GraphCard {
    pub fn new(
        ctx: &Rc<Ctx>,
        title: &str,
        card_id: &'static str,
        spec: GraphSpec,
        format: Format,
    ) -> Rc<GraphCard> {
        let card = Card::new(title);
        let graph = Graph::new();
        graph.set_scale(spec.scale);
        graph.set_name(title, format);
        graph.set_hexpand(true);
        let source: Rc<dyn Source> = Rc::new(ctx.client.clone());
        graph.set_live(Some(Live {
            source,
            stream: stream::SYSTEM.to_string(),
            extract: Rc::new(spec.live),
        }));
        graph.set_range("2m");
        let gc = Rc::new(GraphCard {
            card,
            graph,
            ctx: Rc::downgrade(ctx),
            spec,
            range: RefCell::new("2m".to_string()),
            timer: RefCell::new(None),
        });
        let weak = Rc::downgrade(&gc);
        let picker = RangePicker::new("2m", move |r| {
            if let Some(gc) = weak.upgrade() {
                gc.set_range(r);
            }
        });
        gc.card.set_range_picker(&picker);
        pinnable(&gc.card, ctx, card_id);
        gc.card.append(&gc.graph);
        let weak = Rc::downgrade(&gc);
        gc.graph.connect_map(move |_| {
            if let Some(gc) = weak.upgrade() {
                gc.follow();
            }
        });
        let weak = Rc::downgrade(&gc);
        gc.graph.connect_unmap(move |_| {
            if let Some(gc) = weak.upgrade() {
                gc.stop();
            }
        });
        gc
    }

    /// Takes the graph out of this card, for a card that shows several
    /// small graphs and no range picker of its own (Pressure).
    pub fn detach_graph(&self) -> &Graph {
        if let Some(parent) = self.graph.parent().and_downcast::<gtk::Box>() {
            parent.remove(&self.graph);
        }
        &self.graph
    }

    fn set_range(self: &Rc<Self>, range: &str) {
        if *self.range.borrow() == range {
            return;
        }
        *self.range.borrow_mut() = range.to_string();
        self.graph.set_range(range);
        self.stop();
        if self.graph.is_mapped() {
            self.follow();
        }
    }

    /// For a stored range: fetch now and once a minute while on screen.
    fn follow(self: &Rc<Self>) {
        if *self.range.borrow() == "2m" || self.timer.borrow().is_some() {
            return;
        }
        self.fetch();
        let weak = Rc::downgrade(self);
        // mirqab:sample
        let id = glib::timeout_add_seconds_local(60, move || match weak.upgrade() {
            Some(gc) => {
                gc.fetch();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        *self.timer.borrow_mut() = Some(id);
    }

    fn stop(&self) {
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
    }

    fn fetch(self: &Rc<Self>) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let range = self.range.borrow().clone();
        let series = self.spec.series;
        let weak = Rc::downgrade(self);
        ctx.client.request(
            "history.query",
            json!({"series": [series], "range": range}),
            move |r| {
                let (Some(gc), Ok(reply)) = (weak.upgrade(), r) else {
                    return;
                };
                let (avg, max) = history_points(&reply, series);
                gc.graph.set_points(avg, max);
            },
        );
    }
}

impl Drop for GraphCard {
    fn drop(&mut self) {
        self.stop();
    }
}
