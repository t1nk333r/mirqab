//! Pressure: one small graph per resource (CPU, memory, I/O) of the share
//! of time tasks stalled, with a line on what that means (plan 002).

use crate::app::Ctx;
use crate::tabs::CardWidget;
use crate::tabs::system::graph_card::{GraphCard, pinnable};
use crate::widgets::card::Card;
use crate::widgets::stat::Stat;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{PressureRow, pressure_graph};
use serde_json::Value;
use std::rc::{Rc, Weak};

pub struct PressureCard {
    ctx: Weak<Ctx>,
    pub card: Card,
    rows: Vec<(&'static str, Stat, Rc<GraphCard>)>,
}

impl PressureCard {
    pub fn new(ctx: &Rc<Ctx>, compact: bool) -> PressureCard {
        let card = Card::new(strings::SYSTEM_PRESSURE);
        pinnable(&card, ctx, "system.pressure");
        let hint = gtk::Label::new(None);
        hint.set_text(strings::SYSTEM_PRESSURE_HINT);
        hint.add_css_class("caption");
        hint.add_css_class("dim-label");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        card.append(&hint);
        let mut rows = Vec::new();
        for (name, series) in [
            (strings::SYSTEM_CPU, "psi.cpu"),
            (strings::SYSTEM_MEMORY, "psi.memory"),
            (strings::SYSTEM_IO, "psi.io"),
        ] {
            let stat = Stat::new(name);
            let g = GraphCard::new(
                ctx,
                name,
                None,
                pressure_graph(series),
                Rc::new(format::percent),
                compact,
            );
            card.append(&stat.root);
            card.append(g.detach_graph());
            rows.push((series, stat, g));
        }
        PressureCard {
            ctx: Rc::downgrade(ctx),
            card,
            rows,
        }
    }

    fn show(&self, rows: &[PressureRow]) {
        self.card.root.set_visible(!rows.is_empty());
        for (series, stat, g) in &self.rows {
            let row = rows.iter().find(|r| r.series == *series);
            stat.root.set_visible(row.is_some());
            g.graph.set_visible(row.is_some());
            if let Some(r) = row {
                stat.set_value(&r.text);
            }
        }
    }
}

impl CardWidget for PressureCard {
    fn card(&self) -> &Card {
        &self.card
    }

    fn update(&self, stream: &str, data: &Value) {
        if let Some(v) = super::view_of(&self.ctx, stream, data) {
            self.show(&v.pressure);
        }
    }
}
