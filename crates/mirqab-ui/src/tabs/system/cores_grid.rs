//! The cores: one small meter per core, grouped performance then efficiency
//! on hybrid CPUs, behind a "Cores" disclosure (plan 002).

use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::tabs::system::CoreCell;
use std::cell::RefCell;

pub struct CoresGrid {
    pub root: gtk::Expander,
    column: gtk::Box,
    /// The meters and labels by group, rebuilt when the shape changes.
    cells: RefCell<Vec<Vec<(Meter, gtk::Label)>>>,
}

impl Default for CoresGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl CoresGrid {
    pub fn new() -> CoresGrid {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.add_css_class("mirqab-card-body");
        let root = gtk::Expander::builder().child(&column).build();
        root.set_label(Some(strings::SYSTEM_CORES));
        CoresGrid {
            root,
            column,
            cells: RefCell::new(Vec::new()),
        }
    }

    pub fn update(&self, groups: &[Vec<CoreCell>]) {
        let shape: Vec<usize> = groups.iter().map(Vec::len).collect();
        let have: Vec<usize> = self.cells.borrow().iter().map(Vec::len).collect();
        if shape != have {
            self.rebuild(groups);
        }
        for (g, cells) in groups.iter().zip(self.cells.borrow().iter()) {
            for (c, (meter, label)) in g.iter().zip(cells) {
                meter.set_fraction(c.fraction.unwrap_or(0.0), &c.text);
                label.set_text(&c.label);
            }
        }
    }

    fn rebuild(&self, groups: &[Vec<CoreCell>]) {
        while let Some(c) = self.column.first_child() {
            self.column.remove(&c);
        }
        let mut built = Vec::new();
        for g in groups {
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(true)
                .min_children_per_line(4)
                .max_children_per_line(16)
                .build();
            let mut cells = Vec::new();
            for _ in g {
                let cell = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let meter = Meter::new();
                let label = gtk::Label::new(None);
                label.add_css_class("caption");
                label.add_css_class("dim-label");
                cell.append(&meter);
                cell.append(&label);
                flow.insert(&cell, -1);
                cells.push((meter, label));
            }
            self.column.append(&flow);
            built.push(cells);
        }
        *self.cells.borrow_mut() = built;
    }
}
