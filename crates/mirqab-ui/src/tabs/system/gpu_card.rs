//! One card per GPU: busy percent with its graph, VRAM as a meter,
//! temperature, watts and clock; a suspended card says so and is not read
//! (plan 002).

use crate::app::Ctx;
use crate::tabs::system::graph_card::GraphCard;
use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{GpuCard as View, gpu_graph};
use std::rc::Rc;

pub struct GpuCard {
    pub id: String,
    pub graph: Rc<GraphCard>,
    busy: gtk::Label,
    vram: Meter,
    vram_text: gtk::Label,
    details: gtk::Label,
}

impl GpuCard {
    /// `n` is the card's place (history keeps two GPUs).
    pub fn new(ctx: &Rc<Ctx>, n: usize, v: &View) -> Option<GpuCard> {
        let spec = gpu_graph(n)?;
        let graph = GraphCard::new(ctx, &v.name, "system.gpu", spec, Rc::new(format::percent));
        let busy = gtk::Label::new(None);
        busy.add_css_class("title-2");
        busy.add_css_class("numeric");
        busy.set_xalign(0.0);
        let vram = Meter::new();
        vram.set_series(3);
        let vram_text = gtk::Label::new(None);
        vram_text.add_css_class("caption");
        vram_text.set_xalign(0.0);
        let details = gtk::Label::new(None);
        details.add_css_class("caption");
        details.add_css_class("dim-label");
        details.set_xalign(0.0);
        graph.card.append(&busy);
        graph.card.append(&vram);
        graph.card.append(&vram_text);
        graph.card.append(&details);
        let card = GpuCard {
            id: v.id.clone(),
            graph,
            busy,
            vram,
            vram_text,
            details,
        };
        card.update(v);
        Some(card)
    }

    pub fn update(&self, v: &View) {
        if v.suspended {
            self.busy.set_text(strings::SYSTEM_SUSPENDED);
        } else {
            self.busy
                .set_text(v.busy.as_deref().unwrap_or(strings::NO_VALUE));
        }
        self.graph
            .graph
            .set_visible(!v.suspended && v.busy.is_some());
        match &v.vram {
            Some((text, f)) => {
                self.vram.set_fraction(*f, text);
                self.vram_text
                    .set_text(&format!("{} {text}", strings::SYSTEM_VRAM));
            }
            None => self.vram_text.set_text(""),
        }
        self.vram.set_visible(v.vram.is_some());
        self.vram_text.set_visible(v.vram.is_some());
        self.details.set_text(&v.details.join(" \u{b7} "));
    }
}
