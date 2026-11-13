//! A label above a value: "Used" / "14 GB".

use gtk::prelude::*;
use mirqab_proto::strings;

#[derive(Clone)]
pub struct Stat {
    pub root: gtk::Box,
    value: gtk::Label,
}

impl Stat {
    pub fn new(label: &str) -> Stat {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("mirqab-stat");
        let name = gtk::Label::new(None);
        name.set_text(label);
        name.add_css_class("caption");
        name.add_css_class("dim-label");
        name.set_xalign(0.0);
        let value = gtk::Label::new(None);
        value.set_text(strings::NO_VALUE);
        value.add_css_class("value");
        value.add_css_class("numeric");
        value.set_xalign(0.0);
        root.append(&name);
        root.append(&value);
        Stat { root, value }
    }

    pub fn set_value(&self, text: &str) {
        self.value.set_text(text);
    }
}
