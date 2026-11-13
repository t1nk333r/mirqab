//! The CPU card: the big percent, its graph and range picker, the
//! user/system/iowait legend, clock and governor, and the cores (plan 002).

use crate::app::Ctx;
use crate::tabs::system::cores_grid::CoresGrid;
use crate::tabs::system::graph_card::GraphCard;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{CPU_GRAPH, CpuCard as View};
use std::rc::Rc;

pub struct CpuCard {
    pub graph: Rc<GraphCard>,
    percent: gtk::Label,
    legend: gtk::Label,
    clock: gtk::Label,
    cores: CoresGrid,
}

impl CpuCard {
    pub fn new(ctx: &Rc<Ctx>) -> CpuCard {
        let graph = GraphCard::new(
            ctx,
            strings::SYSTEM_CPU,
            "system.cpu",
            CPU_GRAPH,
            Rc::new(format::percent),
        );
        let percent = gtk::Label::new(None);
        percent.add_css_class("title-1");
        percent.add_css_class("numeric");
        percent.set_xalign(0.0);
        let legend = gtk::Label::new(None);
        legend.add_css_class("caption");
        legend.set_xalign(0.0);
        let clock = gtk::Label::new(None);
        clock.add_css_class("caption");
        clock.add_css_class("dim-label");
        clock.set_xalign(0.0);
        let cores = CoresGrid::new();
        graph.card.append(&percent);
        graph.card.append(&legend);
        graph.card.append(&clock);
        graph.card.append(&cores.root);
        CpuCard {
            graph,
            percent,
            legend,
            clock,
            cores,
        }
    }

    pub fn update(&self, v: &View) {
        self.percent.set_text(&v.percent);
        self.legend.set_text(&v.legend.join("  "));
        let mut clock: Vec<&str> = Vec::new();
        if let Some(c) = &v.clock {
            clock.push(c);
        }
        if let Some(g) = &v.governor {
            clock.push(g);
        }
        self.clock.set_text(&clock.join(" \u{b7} "));
        self.cores.update(&v.core_groups);
    }
}
