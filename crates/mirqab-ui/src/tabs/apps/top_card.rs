//! The `apps.top` card (plan 006, decision 12): the five groups using the
//! most CPU of whatever `apps` frame arrives, System included when it is
//! among them. Activating a row opens the Apps tab with that group
//! selected: the context keeps the group's id, and the page resolves it on
//! its next frame.

use crate::app::Ctx;
use crate::tabs::CardWidget;
use crate::tabs::apps::{icon_slot, set_app_icon};
use crate::widgets::card::Card;
use crate::widgets::meter::Meter;
use gtk::prelude::*;
use mirqab_proto::stream::{self, AppsData};
use mirqab_proto::strings;
use mirqab_view::tabs::apps::{self as model, TOP};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct Line {
    button: gtk::Button,
    image: gtk::Image,
    glyph: gtk::Label,
    name: gtk::Label,
    meter: Meter,
    cpu: gtk::Label,
}

pub struct TopCard {
    card: Card,
    lines: Vec<Line>,
    ids: Rc<RefCell<Vec<String>>>,
}

impl TopCard {
    pub fn new(ctx: &Rc<Ctx>) -> TopCard {
        let card = Card::new(strings::APPS_TOP);
        let ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let mut lines = Vec::new();
        for i in 0..TOP {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.add_css_class("mirqab-apps-row");
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
            row.append(&image);
            row.append(&glyph);
            row.append(&name);
            row.append(&meter);
            row.append(&cpu);
            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("flat");
            button.add_css_class("mirqab-apps-top-row");
            button.set_visible(false);
            let weak: Weak<Ctx> = Rc::downgrade(ctx);
            let ids_c = Rc::clone(&ids);
            button.connect_clicked(move |_| {
                let (Some(ctx), Some(id)) = (weak.upgrade(), ids_c.borrow().get(i).cloned()) else {
                    return;
                };
                ctx.apps.borrow_mut().pending_group = Some(id);
                ctx.show(Some("apps"), None);
            });
            card.append(&button);
            lines.push(Line {
                button,
                image,
                glyph,
                name,
                meter,
                cpu,
            });
        }
        TopCard { card, lines, ids }
    }
}

impl CardWidget for TopCard {
    fn card(&self) -> &Card {
        &self.card
    }

    fn update(&self, stream_name: &str, data: &Value) {
        if stream_name != stream::APPS {
            return;
        }
        let Ok(d) = serde_json::from_value::<AppsData>(data.clone()) else {
            return;
        };
        let top = model::top_five(&d.groups);
        *self.ids.borrow_mut() = top.iter().map(|t| t.id.clone()).collect();
        for (i, line) in self.lines.iter().enumerate() {
            let Some(t) = top.get(i) else {
                line.button.set_visible(false);
                continue;
            };
            line.button.set_visible(true);
            line.button.set_widget_name(&format!("apps-top:{}", t.id));
            line.button
                .update_property(&[gtk::accessible::Property::Label(&t.label)]);
            set_app_icon(&line.image, &line.glyph, t.icon.as_deref(), t.glyph);
            line.name.set_text(&t.name);
            line.meter.set_fraction(t.fraction, &t.cpu);
            line.cpu.set_text(&t.cpu);
        }
    }
}
