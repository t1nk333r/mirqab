//! A process's row (plan 006, decision 7): name, pid, CPU, memory, and
//! its state when it is neither running nor sleeping. Under its group it
//! is indented; in the flat list it names its group instead.

use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_view::tabs::apps::ProcessLine;

pub struct ProcessRow {
    pub root: gtk::Box,
    name: gtk::Label,
    group: gtk::Label,
    state: gtk::Label,
    pid: gtk::Label,
    meter: Meter,
    cpu: gtk::Label,
    memory: gtk::Label,
}

impl Default for ProcessRow {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRow {
    pub fn new() -> ProcessRow {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("mirqab-apps-row");
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let group = gtk::Label::new(None);
        group.add_css_class("dim-label");
        group.add_css_class("caption");
        group.set_xalign(0.0);
        group.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let state = gtk::Label::new(None);
        state.add_css_class("caption");
        state.add_css_class("warning");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let pid = gtk::Label::new(None);
        pid.add_css_class("dim-label");
        pid.add_css_class("numeric");
        pid.add_css_class("caption");
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
        for w in [
            name.upcast_ref::<gtk::Widget>(),
            group.upcast_ref(),
            state.upcast_ref(),
            spacer.upcast_ref(),
            pid.upcast_ref(),
            meter.upcast_ref(),
            cpu.upcast_ref(),
            memory.upcast_ref(),
        ] {
            root.append(w);
        }
        ProcessRow {
            root,
            name,
            group,
            state,
            pid,
            meter,
            cpu,
            memory,
        }
    }

    pub fn show(&self, p: &ProcessLine) {
        self.root
            .set_widget_name(&format!("apps-process:{}", p.pid));
        if p.group_name.is_some() {
            self.root.remove_css_class("mirqab-apps-child");
        } else {
            self.root.add_css_class("mirqab-apps-child");
        }
        self.name.set_text(&p.name);
        self.group
            .set_text(p.group_name.as_deref().unwrap_or_default());
        self.group.set_visible(p.group_name.is_some());
        self.state.set_text(p.state.as_deref().unwrap_or_default());
        self.state.set_visible(p.state.is_some());
        self.pid.set_text(&p.pid_text);
        self.meter.set_fraction(p.fraction, &p.cpu);
        self.cpu.set_text(&p.cpu);
        self.memory.set_text(&p.memory);
        self.root
            .update_property(&[gtk::accessible::Property::Label(&p.label)]);
    }
}
