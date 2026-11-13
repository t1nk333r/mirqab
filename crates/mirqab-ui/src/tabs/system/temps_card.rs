//! Temperatures, grouped by kind, each with a small meter against its
//! critical temperature (plan 002).

use crate::app::Ctx;
use crate::tabs::system::graph_card::pinnable;
use crate::widgets::card::Card;
use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::tabs::system::TempRow;
use std::cell::RefCell;
use std::rc::Rc;

type Group = (String, Vec<TempRow>);

pub struct TempsCard {
    pub card: Card,
    list: gtk::Box,
    /// The rows' labels as built, to rebuild only when sensors change.
    shape: RefCell<Vec<(String, Vec<String>)>>,
    rows: RefCell<Vec<(gtk::Label, Meter)>>,
}

impl TempsCard {
    pub fn new(ctx: &Rc<Ctx>) -> TempsCard {
        let card = Card::new(strings::SYSTEM_TEMPERATURES);
        pinnable(&card, ctx, "system.temps");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("mirqab-card-body");
        card.append(&list);
        TempsCard {
            card,
            list,
            shape: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
        }
    }

    pub fn update(&self, groups: &[Group]) {
        self.card.root.set_visible(!groups.is_empty());
        let shape: Vec<(String, Vec<String>)> = groups
            .iter()
            .map(|(k, rows)| (k.clone(), rows.iter().map(|r| r.label.clone()).collect()))
            .collect();
        if *self.shape.borrow() != shape {
            self.rebuild(groups);
            *self.shape.borrow_mut() = shape;
        }
        let rows: Vec<&TempRow> = groups.iter().flat_map(|(_, r)| r).collect();
        for (r, (value, meter)) in rows.iter().zip(self.rows.borrow().iter()) {
            value.set_text(&r.text);
            meter.set_fraction(r.fraction, &r.text);
        }
    }

    fn rebuild(&self, groups: &[Group]) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let mut built = Vec::new();
        for (kind, rows) in groups {
            let heading = gtk::Label::new(None);
            heading.set_text(kind);
            heading.add_css_class("heading");
            heading.set_xalign(0.0);
            self.list.append(&heading);
            for r in rows {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                line.add_css_class("mirqab-card-header");
                let name = gtk::Label::new(None);
                name.set_text(&r.label);
                name.set_xalign(0.0);
                name.set_hexpand(true);
                let value = gtk::Label::new(None);
                value.add_css_class("numeric");
                let meter = Meter::new();
                meter.set_series(4);
                meter.set_hexpand(true);
                line.append(&name);
                line.append(&meter);
                line.append(&value);
                self.list.append(&line);
                built.push((value, meter));
            }
        }
        *self.rows.borrow_mut() = built;
    }
}
