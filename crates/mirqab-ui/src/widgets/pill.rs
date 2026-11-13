//! A coloured status word: libadwaita's `success`, `warning`, `error` or
//! `accent` class on a label.

use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
}

impl Tone {
    fn class(self) -> Option<&'static str> {
        match self {
            Tone::Neutral => None,
            Tone::Accent => Some("accent"),
            Tone::Success => Some("success"),
            Tone::Warning => Some("warning"),
            Tone::Error => Some("error"),
        }
    }
}

pub fn pill(text: &str, tone: Tone) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_text(text);
    l.add_css_class("caption-heading");
    set_tone(&l, tone);
    l
}

pub fn set_tone(pill: &gtk::Label, tone: Tone) {
    for t in [Tone::Accent, Tone::Success, Tone::Warning, Tone::Error] {
        if let Some(c) = t.class() {
            pill.remove_css_class(c);
        }
    }
    if let Some(c) = tone.class() {
        pill.add_css_class(c);
    }
}
