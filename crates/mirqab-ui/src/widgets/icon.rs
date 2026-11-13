//! A glyph from Symbols Nerd Font in a label with the `icon` class
//! (architecture §7.2). Decorative: the control around it carries the
//! accessible label.

use gtk::prelude::*;
use mirqab_view::icons::Glyph;

pub fn icon(glyph: Glyph) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(glyph.ch.to_string())
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    l.add_css_class("icon");
    l
}

/// A flat button showing only `glyph`, with `label` as its accessible label
/// and tooltip.
pub fn icon_button(glyph: Glyph, label: &str) -> gtk::Button {
    let b = gtk::Button::builder().child(&icon(glyph)).build();
    b.add_css_class("flat");
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}
