//! Power: package and battery watts with a graph; without a package
//! reading, one line that the root helper enables it (plan 002).

use crate::app::Ctx;
use crate::tabs::system::graph_card::GraphCard;
use crate::widgets::stat::Stat;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::tabs::system::{POWER_GRAPH, PowerCard as View};
use std::rc::Rc;

pub struct PowerCard {
    pub graph: Rc<GraphCard>,
    package: Stat,
    battery: Stat,
    helper: gtk::Label,
}

impl PowerCard {
    pub fn new(ctx: &Rc<Ctx>) -> PowerCard {
        let graph = GraphCard::new(
            ctx,
            strings::SYSTEM_POWER,
            "system.power",
            POWER_GRAPH,
            Rc::new(|w| format!("{w:.1} {}", strings::WATTS)),
        );
        let package = Stat::new(strings::SYSTEM_PACKAGE);
        let battery = Stat::new(strings::SYSTEM_BATTERY);
        let helper = gtk::Label::new(None);
        helper.set_text(strings::PACKAGE_POWER_NEEDS_HELPER);
        helper.add_css_class("caption");
        helper.add_css_class("dim-label");
        helper.set_xalign(0.0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("mirqab-card-header");
        row.append(&package.root);
        row.append(&battery.root);
        graph.card.append(&row);
        graph.card.append(&helper);
        PowerCard {
            graph,
            package,
            battery,
            helper,
        }
    }

    pub fn update(&self, v: &View) {
        self.package.root.set_visible(v.package.is_some());
        self.package
            .set_value(v.package.as_deref().unwrap_or_default());
        self.battery.root.set_visible(v.battery.is_some());
        self.battery
            .set_value(v.battery.as_deref().unwrap_or_default());
        self.helper.set_visible(v.needs_helper);
        self.graph.graph.set_visible(!v.needs_helper);
    }
}
