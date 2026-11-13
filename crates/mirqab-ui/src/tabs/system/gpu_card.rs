//! The graphics card: one section per GPU with busy percent and its graph,
//! VRAM as a meter, temperature, watts and clock; a suspended GPU says so
//! and is not read (plan 002). One card, so Home pins all GPUs together
//! (plan 003); its range picker drives every GPU's graph.

use crate::app::Ctx;
use crate::tabs::CardWidget;
use crate::tabs::system::graph_card::{GraphCard, pinnable};
use crate::widgets::card::Card;
use crate::widgets::meter::Meter;
use crate::widgets::range_picker::RangePicker;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{GpuCard as View, gpu_graph};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// One GPU's rows inside the card.
struct Section {
    id: String,
    root: gtk::Box,
    graph: Rc<GraphCard>,
    busy: gtk::Label,
    vram: Meter,
    vram_text: gtk::Label,
    details: gtk::Label,
}

impl Section {
    /// `n` is the GPU's place (history keeps two GPUs).
    fn new(ctx: &Rc<Ctx>, n: usize, v: &View, range: &str, compact: bool) -> Option<Section> {
        let spec = gpu_graph(n)?;
        let graph = GraphCard::new(ctx, &v.name, None, spec, Rc::new(format::percent), compact);
        graph.set_range(range);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("mirqab-card-body");
        let name = gtk::Label::new(None);
        name.set_text(&v.name);
        name.add_css_class("heading");
        name.set_xalign(0.0);
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
        root.append(&name);
        root.append(&busy);
        root.append(graph.detach_graph());
        root.append(&vram);
        root.append(&vram_text);
        root.append(&details);
        let s = Section {
            id: v.id.clone(),
            root,
            graph,
            busy,
            vram,
            vram_text,
            details,
        };
        s.show(v);
        Some(s)
    }

    fn show(&self, v: &View) {
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

pub struct GpuCards {
    ctx: Weak<Ctx>,
    card: Card,
    list: gtk::Box,
    compact: bool,
    range: Rc<RefCell<String>>,
    sections: Rc<RefCell<Vec<Section>>>,
}

impl GpuCards {
    pub fn new(ctx: &Rc<Ctx>, compact: bool) -> GpuCards {
        let card = Card::new(strings::SYSTEM_GRAPHICS);
        let range = Rc::new(RefCell::new("2m".to_string()));
        let sections: Rc<RefCell<Vec<Section>>> = Rc::new(RefCell::new(Vec::new()));
        if !compact {
            let (r, s) = (Rc::clone(&range), Rc::clone(&sections));
            let picker = RangePicker::new("2m", move |picked| {
                *r.borrow_mut() = picked.to_string();
                for sec in s.borrow().iter() {
                    sec.graph.set_range(picked);
                }
            });
            card.set_range_picker(&picker);
        }
        pinnable(&card, ctx, "system.gpu");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.append(&list);
        // Hidden until an event lists a GPU.
        card.root.set_visible(false);
        GpuCards {
            ctx: Rc::downgrade(ctx),
            card,
            list,
            compact,
            range,
            sections,
        }
    }

    fn show(&self, ctx: &Rc<Ctx>, gpus: &[View]) {
        let ids: Vec<&str> = gpus.iter().map(|g| g.id.as_str()).collect();
        let same = {
            let have = self.sections.borrow();
            have.len() == ids.len() && have.iter().zip(&ids).all(|(s, id)| s.id == *id)
        };
        if !same {
            while let Some(c) = self.list.first_child() {
                self.list.remove(&c);
            }
            let range = self.range.borrow().clone();
            let built: Vec<Section> = gpus
                .iter()
                .enumerate()
                .filter_map(|(n, g)| Section::new(ctx, n, g, &range, self.compact))
                .collect();
            for s in &built {
                self.list.append(&s.root);
            }
            *self.sections.borrow_mut() = built;
        }
        for (s, g) in self.sections.borrow().iter().zip(gpus) {
            s.show(g);
        }
        self.card
            .root
            .set_visible(!self.sections.borrow().is_empty());
    }
}

impl CardWidget for GpuCards {
    fn card(&self) -> &Card {
        &self.card
    }

    fn update(&self, stream: &str, data: &Value) {
        let Some(v) = super::view_of(&self.ctx, stream, data) else {
            return;
        };
        if let Some(ctx) = self.ctx.upgrade() {
            self.show(&ctx, &v.gpus);
        }
    }
}
