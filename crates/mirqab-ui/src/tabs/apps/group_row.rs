//! An app's row (plan 006, decision 7): the expander (hidden for a single
//! process), the icon, the name, a CPU meter full at 100 % of one core
//! with the number beside it (which may pass 100), memory and the process
//! count, then "busy while not in use" in a dim caption when the group is
//! tagged "idle but heavy" (plan 006, part 4, decision 16: a word, no
//! colour and no icon). One widget, rebound to whichever group its list
//! position holds.

use crate::tabs::apps::{icon_slot, set_app_icon};
use crate::widgets::icon::icon_button;
use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::icons;
use mirqab_view::tabs::apps::GroupLine;
use std::cell::Cell;
use std::rc::Rc;

pub struct GroupRow {
    pub root: gtk::Box,
    expander: gtk::Button,
    image: gtk::Image,
    glyph: gtk::Label,
    name: gtk::Label,
    meter: Meter,
    cpu: gtk::Label,
    memory: gtk::Label,
    procs: gtk::Label,
    /// "busy while not in use", shown only on a tagged group.
    pub idle: gtk::Label,
    key: Rc<Cell<u64>>,
}

impl GroupRow {
    /// `on_toggle` gets the group's key when its expander is pressed.
    pub fn new(on_toggle: Rc<dyn Fn(u64)>) -> GroupRow {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("mirqab-apps-row");
        let expander = icon_button(icons::EXPAND, strings::APPS_EXPAND);
        expander.add_css_class("mirqab-apps-expander");
        let key = Rc::new(Cell::new(0));
        let k = Rc::clone(&key);
        expander.connect_clicked(move |_| on_toggle(k.get()));
        let (image, glyph) = icon_slot();
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meter = Meter::new();
        meter.set_valign(gtk::Align::Center);
        let cpu = gtk::Label::new(None);
        cpu.add_css_class("numeric");
        cpu.add_css_class("mirqab-apps-cpu");
        cpu.set_xalign(1.0);
        let memory = gtk::Label::new(None);
        memory.add_css_class("numeric");
        memory.add_css_class("mirqab-apps-memory");
        memory.set_xalign(1.0);
        let procs = gtk::Label::new(None);
        procs.add_css_class("dim-label");
        procs.add_css_class("caption");
        procs.add_css_class("mirqab-apps-procs");
        procs.set_xalign(1.0);
        let idle = gtk::Label::new(Some(strings::APPS_IDLE_HEAVY));
        idle.add_css_class("dim-label");
        idle.add_css_class("caption");
        idle.set_widget_name("apps-idle-heavy");
        idle.set_visible(false);
        for w in [
            expander.upcast_ref::<gtk::Widget>(),
            image.upcast_ref(),
            glyph.upcast_ref(),
            name.upcast_ref(),
            meter.upcast_ref(),
            cpu.upcast_ref(),
            memory.upcast_ref(),
            procs.upcast_ref(),
            idle.upcast_ref(),
        ] {
            root.append(w);
        }
        GroupRow {
            root,
            expander,
            image,
            glyph,
            name,
            meter,
            cpu,
            memory,
            procs,
            idle,
            key,
        }
    }

    pub fn show(&self, g: &GroupLine) {
        self.key.set(g.key);
        self.root.set_widget_name(&format!("apps-group:{}", g.id));
        // The expander's space is kept, so names line up.
        self.expander
            .set_opacity(if g.expandable { 1.0 } else { 0.0 });
        self.expander.set_sensitive(g.expandable);
        let (glyph, label) = if g.open {
            (icons::COLLAPSE, strings::APPS_COLLAPSE)
        } else {
            (icons::EXPAND, strings::APPS_EXPAND)
        };
        if let Some(l) = self.expander.child().and_downcast::<gtk::Label>() {
            l.set_text(&glyph.ch.to_string());
        }
        self.expander.set_tooltip_text(Some(label));
        self.expander
            .update_property(&[gtk::accessible::Property::Label(label)]);
        self.expander
            .update_state(&[gtk::accessible::State::Expanded(Some(g.open))]);
        set_app_icon(&self.image, &self.glyph, g.icon.as_deref(), g.glyph);
        self.name.set_text(&g.name);
        self.meter.set_fraction(g.fraction, &g.cpu);
        self.cpu.set_text(&g.cpu);
        self.memory.set_text(&g.memory);
        self.procs.set_text(&g.procs);
        self.idle.set_visible(g.idle_heavy);
        self.root
            .update_property(&[gtk::accessible::Property::Label(&g.label)]);
    }
}
