//! The memory card: used of total as a meter split into used, cache and
//! free, its graph, and swap, compressed swap and paging lines (plan 002).

use crate::app::Ctx;
use crate::tabs::system::graph_card::GraphCard;
use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{MEMORY_GRAPH, MemoryCard as View};
use std::rc::Rc;

pub struct MemoryCard {
    pub graph: Rc<GraphCard>,
    total: gtk::Label,
    used: Meter,
    cache: Meter,
    legend: gtk::Label,
    lines: gtk::Label,
}

impl MemoryCard {
    pub fn new(ctx: &Rc<Ctx>) -> MemoryCard {
        let graph = GraphCard::new(
            ctx,
            strings::SYSTEM_MEMORY,
            "system.memory",
            MEMORY_GRAPH,
            Rc::new(|b| format::bytes(b.max(0.0) as u64)),
        );
        let total = gtk::Label::new(None);
        total.add_css_class("title-2");
        total.add_css_class("numeric");
        total.set_xalign(0.0);
        // Used, then cache stacked under it: the second meter's fill
        // starts where the first ends, drawn as used + cache.
        let used = Meter::new();
        used.set_series(0);
        let cache = Meter::new();
        cache.set_series(1);
        let legend = gtk::Label::new(None);
        legend.add_css_class("caption");
        legend.set_xalign(0.0);
        let lines = gtk::Label::new(None);
        lines.add_css_class("caption");
        lines.add_css_class("dim-label");
        lines.set_xalign(0.0);
        lines.set_wrap(true);
        graph.card.append(&total);
        graph.card.append(&used);
        graph.card.append(&cache);
        graph.card.append(&legend);
        graph.card.append(&lines);
        MemoryCard {
            graph,
            total,
            used,
            cache,
            legend,
            lines,
        }
    }

    pub fn update(&self, v: &View) {
        self.total.set_text(&v.used_of_total);
        self.used.set_fraction(v.split.used, &v.legend[0]);
        self.cache.set_fraction(v.split.cache, &v.legend[1]);
        self.legend.set_text(&v.legend.join("  "));
        let lines: Vec<&str> = [&v.swap, &v.zram, &v.paging]
            .into_iter()
            .filter_map(|l| l.as_deref())
            .collect();
        self.lines.set_text(&lines.join("\n"));
        self.lines.set_visible(!lines.is_empty());
    }
}
