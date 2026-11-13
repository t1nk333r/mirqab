//! A count on a sidebar tile (Ports: listeners; Agents: sessions waiting).
//! Hidden at zero.

use gtk::prelude::*;

pub fn badge() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("mirqab-badge");
    l.add_css_class("numeric");
    l.set_visible(false);
    l
}

pub fn set_count(badge: &gtk::Label, n: u32) {
    badge.set_text(&n.to_string());
    badge.set_visible(n > 0);
}
