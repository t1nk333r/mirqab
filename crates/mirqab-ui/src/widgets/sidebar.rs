//! The sidebar: one tile per tab, glyph above a short label, the active tile
//! filled with the accent; Trends and Support pushed to the bottom
//! (architecture §7.1). The tiles come from `mirqab_view::shell::sidebar`.

use crate::widgets::badge;
use crate::widgets::icon::icon;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::shell::{Place, Tile};
use std::cell::RefCell;
use std::rc::Rc;

/// A tile's tab id, its button and its badge.
type Built = (&'static str, gtk::Button, gtk::Label);

#[derive(Clone)]
pub struct Sidebar {
    /// Scrolls when sixteen tiles are taller than the window.
    pub root: gtk::ScrolledWindow,
    column: gtk::Box,
    tiles: Rc<RefCell<Vec<Built>>>,
    on_select: Rc<dyn Fn(&str)>,
}

impl Sidebar {
    pub fn new(on_select: impl Fn(&str) + 'static) -> Sidebar {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.update_property(&[gtk::accessible::Property::Label(strings::SIDEBAR)]);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_width(true)
            .child(&column)
            .build();
        root.add_css_class("mirqab-sidebar");
        Sidebar {
            root,
            column,
            tiles: Rc::new(RefCell::new(Vec::new())),
            on_select: Rc::new(on_select),
        }
    }

    /// Rebuilds the tiles.
    pub fn set_tiles(&self, tiles: &[Tile], active: &str) {
        while let Some(c) = self.column.first_child() {
            self.column.remove(&c);
        }
        let mut built = Vec::new();
        let mut spacer_added = false;
        for t in tiles {
            if t.place == Place::Bottom && !spacer_added {
                let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
                spacer.set_vexpand(true);
                self.column.append(&spacer);
                spacer_added = true;
            }
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            content.append(&icon(t.icon));
            let label = gtk::Label::new(None);
            label.set_text(t.label);
            label.add_css_class("caption");
            content.append(&label);
            let count = badge::badge();
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&content));
            count.set_halign(gtk::Align::End);
            count.set_valign(gtk::Align::Start);
            overlay.add_overlay(&count);
            let b = gtk::Button::builder().child(&overlay).build();
            b.add_css_class("flat");
            b.add_css_class("mirqab-tile");
            b.set_tooltip_text(Some(t.label));
            b.update_property(&[gtk::accessible::Property::Label(t.label)]);
            let id = t.id;
            let f = Rc::clone(&self.on_select);
            b.connect_clicked(move |_| f(id));
            self.column.append(&b);
            built.push((t.id, b, count));
        }
        *self.tiles.borrow_mut() = built;
        self.set_active(active);
    }

    pub fn set_active(&self, id: &str) {
        for (tid, b, _) in self.tiles.borrow().iter() {
            if *tid == id {
                b.add_css_class("active");
                b.update_state(&[gtk::accessible::State::Selected(Some(true))]);
            } else {
                b.remove_css_class("active");
                b.update_state(&[gtk::accessible::State::Selected(Some(false))]);
            }
        }
    }

    pub fn set_badge(&self, id: &str, n: u32) {
        if let Some((_, _, count)) = self.tiles.borrow().iter().find(|(t, _, _)| *t == id) {
            badge::set_count(count, n);
        }
    }
}
