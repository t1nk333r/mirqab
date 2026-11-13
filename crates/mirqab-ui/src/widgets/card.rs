//! A card: a title, an optional range picker and "Pin to Home", and a body
//! (architecture §7.1).

use crate::widgets::icon::icon_button;
use crate::widgets::range_picker::RangePicker;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::icons;

#[derive(Clone)]
pub struct Card {
    pub root: gtk::Box,
    header: gtk::Box,
    body: gtk::Box,
}

impl Card {
    pub fn new(title: &str) -> Card {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("card");
        root.add_css_class("mirqab-card");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("mirqab-card-header");
        let t = gtk::Label::new(None);
        t.set_text(title);
        t.add_css_class("heading");
        t.set_xalign(0.0);
        t.set_hexpand(true);
        header.append(&t);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("mirqab-card-body");
        root.append(&header);
        root.append(&body);
        Card { root, header, body }
    }

    pub fn append(&self, w: &impl IsA<gtk::Widget>) {
        self.body.append(w);
    }

    pub fn set_range_picker(&self, picker: &RangePicker) {
        self.header.append(&picker.root);
    }

    /// Adds the pin control; `on_toggle` gets whether the card is now pinned.
    pub fn set_pinnable(&self, pinned: bool, on_toggle: impl Fn(bool) + 'static) {
        let glyph = if pinned {
            icons::PIN
        } else {
            icons::PIN_OUTLINE
        };
        let label = if pinned {
            strings::UNPIN_FROM_HOME
        } else {
            strings::PIN_TO_HOME
        };
        let b = icon_button(glyph, label);
        b.connect_clicked(move |_| on_toggle(!pinned));
        self.header.append(&b);
    }
}
