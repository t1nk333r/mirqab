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

    /// Adds a control to the header, after the title.
    pub fn add_header(&self, w: &impl IsA<gtk::Widget>) {
        self.header.append(w);
    }

    /// Adds the pin control; `on_click` runs on each press. The button
    /// shows `pinned` and follows `set_pinned`.
    pub fn set_pinnable(&self, pinned: bool, on_click: impl Fn() + 'static) -> gtk::Button {
        let b = icon_button(icons::PIN_OUTLINE, strings::PIN_TO_HOME);
        set_pin_state(&b, pinned);
        b.connect_clicked(move |_| on_click());
        self.header.append(&b);
        b
    }
}

/// The pin button's glyph and label for `pinned`.
pub fn set_pin_state(b: &gtk::Button, pinned: bool) {
    let (glyph, label) = if pinned {
        (icons::PIN, strings::UNPIN_FROM_HOME)
    } else {
        (icons::PIN_OUTLINE, strings::PIN_TO_HOME)
    };
    if let Some(l) = b.child().and_downcast::<gtk::Label>() {
        l.set_text(&glyph.ch.to_string());
    }
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
}
