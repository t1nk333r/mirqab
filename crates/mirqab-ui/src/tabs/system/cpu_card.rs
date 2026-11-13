//! The CPU card: the big percent, its graph and range picker, the
//! user/system/iowait legend, clock and governor, and the cores (plan 002).

use crate::app::Ctx;
use crate::tabs::CardWidget;
use crate::tabs::system::cores_grid::CoresGrid;
use crate::tabs::system::graph_card::GraphCard;
use crate::widgets::card::Card;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::format;
use mirqab_view::tabs::system::{CPU_GRAPH, CpuCard as View};
use serde_json::Value;
use std::rc::{Rc, Weak};

pub struct CpuCard {
    ctx: Weak<Ctx>,
    pub graph: Rc<GraphCard>,
    percent: gtk::Label,
    legend: gtk::Label,
    clock: gtk::Label,
    cores: CoresGrid,
}

impl CpuCard {
    pub fn new(ctx: &Rc<Ctx>, compact: bool) -> CpuCard {
        let graph = GraphCard::new(
            ctx,
            strings::SYSTEM_CPU,
            Some("system.cpu"),
            CPU_GRAPH,
            Rc::new(format::percent),
            compact,
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
            ctx: Rc::downgrade(ctx),
            graph,
            percent,
            legend,
            clock,
            cores,
        }
    }

    fn show(&self, v: &View) {
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

impl CardWidget for CpuCard {
    fn card(&self) -> &Card {
        &self.graph.card
    }

    fn update(&self, stream: &str, data: &Value) {
        let Some(v) = super::view_of(&self.ctx, stream, data) else {
            return;
        };
        self.graph.card.root.set_visible(v.cpu.is_some());
        if let Some(c) = &v.cpu {
            self.show(c);
        }
    }
}
