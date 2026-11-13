//! The overview strip: the thermal word, uptime, load, and a quiet advice
//! line when there is one (plan 002).

use crate::app::Ctx;
use crate::tabs::CardWidget;
use crate::tabs::system::graph_card::pinnable;
use crate::widgets::card::Card;
use crate::widgets::pill::{Tone as PillTone, pill, set_tone};
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::tabs::system::{Overview, Tone};
use serde_json::Value;
use std::rc::{Rc, Weak};

pub struct OverviewStrip {
    ctx: Weak<Ctx>,
    pub card: Card,
    thermal: gtk::Label,
    uptime: gtk::Label,
    load: gtk::Label,
    advice: gtk::Label,
}

fn caption() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("dim-label");
    l
}

impl OverviewStrip {
    pub fn new(ctx: &Rc<Ctx>) -> OverviewStrip {
        let card = Card::new(strings::SYSTEM_OVERVIEW);
        pinnable(&card, ctx, "system.overview");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("mirqab-card-header");
        let thermal = pill(strings::NO_VALUE, PillTone::Neutral);
        let (uptime, load) = (caption(), caption());
        row.append(&thermal);
        row.append(&uptime);
        row.append(&load);
        let advice = gtk::Label::new(None);
        advice.add_css_class("caption");
        advice.add_css_class("warning");
        advice.set_xalign(0.0);
        advice.set_wrap(true);
        advice.set_visible(false);
        card.append(&row);
        card.append(&advice);
        OverviewStrip {
            ctx: Rc::downgrade(ctx),
            card,
            thermal,
            uptime,
            load,
            advice,
        }
    }

    fn show(&self, o: &Overview) {
        match &o.thermal {
            Some((word, tone)) => {
                self.thermal.set_text(word);
                set_tone(
                    &self.thermal,
                    match tone {
                        Tone::Success => PillTone::Success,
                        Tone::Warning => PillTone::Warning,
                        Tone::Error => PillTone::Error,
                        Tone::Neutral => PillTone::Neutral,
                    },
                );
                self.thermal.set_visible(true);
            }
            None => self.thermal.set_visible(false),
        }
        self.uptime
            .set_text(o.uptime.as_deref().unwrap_or_default());
        self.load.set_text(o.load.as_deref().unwrap_or_default());
        self.advice
            .set_text(o.advice.as_deref().unwrap_or_default());
        self.advice.set_visible(o.advice.is_some());
    }
}

impl CardWidget for OverviewStrip {
    fn card(&self) -> &Card {
        &self.card
    }

    fn update(&self, stream: &str, data: &Value) {
        if let Some(v) = super::view_of(&self.ctx, stream, data) {
            self.show(&v.overview);
        }
    }
}
