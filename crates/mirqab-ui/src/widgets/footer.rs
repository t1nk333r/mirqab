//! The footer: what mirqab costs (daemon plus interface, split in the
//! tooltip), the machine line, a notice line, and buttons for Health
//! (Support), Settings and a menu (architecture §7.1).

use crate::widgets::icon::{icon, icon_button};
use gtk::gio;
use gtk::prelude::*;
use mirqab_proto::stream::SelfData;
use mirqab_proto::strings;
use mirqab_view::icons;
use mirqab_view::shell;

#[derive(Clone)]
pub struct Footer {
    pub root: gtk::Box,
    cost: gtk::Label,
    machine: gtk::Label,
    notice: gtk::Label,
    pub health: gtk::Button,
    pub settings: gtk::Button,
}

impl Footer {
    /// `menu` is the model behind the menu button (window mode, stop).
    pub fn new(menu: &gio::Menu) -> Footer {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("mirqab-footer");
        let cost = gtk::Label::new(None);
        cost.add_css_class("caption");
        cost.add_css_class("numeric");
        cost.add_css_class("mirqab-cost");
        let machine = gtk::Label::new(None);
        machine.add_css_class("caption");
        machine.add_css_class("dim-label");
        machine.add_css_class("mirqab-machine");
        machine.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let notice = gtk::Label::new(None);
        notice.add_css_class("caption");
        notice.add_css_class("warning");
        notice.add_css_class("mirqab-notice");
        notice.set_ellipsize(gtk::pango::EllipsizeMode::End);
        notice.set_visible(false);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_hexpand(true);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        line.add_css_class("mirqab-footer-line");
        line.append(&cost);
        line.append(&machine);
        cost.set_xalign(0.0);
        machine.set_xalign(0.0);
        notice.set_xalign(0.0);
        text.append(&line);
        text.append(&notice);
        root.append(&text);

        let health = icon_button(icons::HEALTH, strings::HEALTH);
        let settings = icon_button(
            icons::SETTINGS,
            strings::tab_label("settings").unwrap_or_default(),
        );
        let more = gtk::MenuButton::builder()
            .menu_model(menu)
            .child(&icon(icons::MENU))
            .build();
        more.add_css_class("flat");
        more.set_tooltip_text(Some(strings::MORE));
        more.update_property(&[gtk::accessible::Property::Label(strings::MORE)]);
        root.append(&health);
        root.append(&settings);
        root.append(&more);
        Footer {
            root,
            cost,
            machine,
            notice,
            health,
            settings,
        }
    }

    /// From a `self` event.
    pub fn set_self(&self, data: &SelfData) {
        let c = shell::cost(data);
        self.cost.set_text(&c.text);
        self.cost.set_tooltip_text(Some(&c.detail));
        self.machine.set_text(&shell::machine_line(
            data.machine.cpu_model.as_deref(),
            data.machine.uptime_sec,
        ));
    }

    /// A one-line notice (not connected, no layer shell); empty hides it.
    pub fn set_notice(&self, text: &str) {
        self.notice.set_text(text);
        self.notice.set_visible(!text.is_empty());
    }
}
