//! The Apps tab (plan 006, part 3): one row per app with its processes on
//! demand, an inspector in a side sheet with Quit and Force quit, and the
//! `apps.top` card for Home. The view model is
//! `mirqab_view::tabs::apps`; these files lay its rows out.

pub mod group_row;
pub mod inspector;
pub mod page;
pub mod process_row;
pub mod top_card;

use crate::widgets::em;
use crate::widgets::icon::icon;
use gtk::prelude::*;
use mirqab_view::icons::Glyph;

/// An app's icon slot: an image for a theme icon and a glyph for the rest,
/// one of them shown. Both are decorative; the row carries the label.
pub fn icon_slot() -> (gtk::Image, gtk::Label) {
    let image = gtk::Image::builder()
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    image.add_css_class("mirqab-app-icon");
    let glyph = icon(mirqab_view::icons::KIND_APP);
    glyph.add_css_class("mirqab-app-icon");
    (image, glyph)
}

/// Shows `name` from the display's icon theme when the theme has it
/// (decision 8), otherwise `fallback`'s glyph. Nothing is loaded by path:
/// the view model passes theme names only.
pub fn set_app_icon(image: &gtk::Image, glyph: &gtk::Label, name: Option<&str>, fallback: Glyph) {
    let themed = name.filter(|n| gtk::IconTheme::for_display(&image.display()).has_icon(n));
    match themed {
        Some(n) => {
            image.set_icon_name(Some(n));
            // In em of the row's font, like every other size.
            image.set_pixel_size((em(image) * 1.5).round() as i32);
            image.set_visible(true);
            glyph.set_visible(false);
        }
        None => {
            image.set_visible(false);
            glyph.set_text(&fallback.ch.to_string());
            glyph.set_visible(true);
        }
    }
}
