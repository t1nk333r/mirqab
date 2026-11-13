//! The range buttons on a graph card: 2m (live), 1h, 24h, 7d, 30d
//! (architecture §6.4).

use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::graph::RANGES;
use std::rc::Rc;

#[derive(Clone)]
pub struct RangePicker {
    pub root: gtk::Box,
    buttons: Rc<Vec<(&'static str, gtk::ToggleButton)>>,
}

impl RangePicker {
    /// `on_change` runs with the new range when the user picks one.
    pub fn new(active: &str, on_change: impl Fn(&str) + 'static) -> RangePicker {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        root.update_property(&[gtk::accessible::Property::Label(strings::RANGE)]);
        let on_change = Rc::new(on_change);
        let mut buttons: Vec<(&'static str, gtk::ToggleButton)> = Vec::new();
        for &r in RANGES {
            let b = gtk::ToggleButton::new();
            b.set_label(strings::range_label(r));
            b.add_css_class("flat");
            b.add_css_class("caption");
            if let Some((_, first)) = buttons.first() {
                b.set_group(Some(first));
            }
            b.set_active(r == active);
            let f = Rc::clone(&on_change);
            b.connect_toggled(move |b| {
                if b.is_active() {
                    f(r);
                }
            });
            root.append(&b);
            buttons.push((r, b));
        }
        RangePicker {
            root,
            buttons: Rc::new(buttons),
        }
    }

    /// Shows `range` as active. GTK runs the toggled handler for this too,
    /// so `on_change` treats the range already in force as no change.
    pub fn set_active(&self, range: &str) {
        for (r, b) in self.buttons.iter() {
            if *r == range {
                b.set_active(true);
            }
        }
    }
}
