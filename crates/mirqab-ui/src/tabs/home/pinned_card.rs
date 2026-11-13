//! A card on Home (plan 003): the tab's own card widget, compact, with a
//! drag handle shown on hover and Alt+Up / Alt+Down to move it. Its pin
//! control unpins it.

use crate::app::Ctx;
use crate::tabs::{self, CardWidget};
use crate::widgets::icon::icon_button;
use gtk::prelude::*;
use gtk::{gdk, glib};
use mirqab_proto::strings;
use mirqab_view::icons;
use mirqab_view::tabs::home;
use serde_json::json;
use std::rc::{Rc, Weak};

pub struct PinnedCard {
    pub id: &'static str,
    pub widget: Rc<dyn CardWidget>,
}

impl PinnedCard {
    /// `on_moved` runs with the card's id after Alt+Up or Alt+Down, so the
    /// page can give it focus again once it is rebuilt.
    pub fn new(
        ctx: &Rc<Ctx>,
        id: &'static str,
        on_moved: Rc<dyn Fn(&'static str)>,
    ) -> Option<PinnedCard> {
        let widget = tabs::card(id, ctx, true)?;
        let card = widget.card();
        card.root.set_focusable(true);

        let handle = icon_button(icons::DRAG, strings::DRAG_TO_REORDER);
        handle.add_css_class("mirqab-drag-handle");
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
        handle.add_controller(drag);
        card.add_header(&handle);

        let weak: Weak<Ctx> = Rc::downgrade(ctx);
        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        drop.connect_drop(move |_, value, _, _| {
            let (Some(ctx), Ok(from)) = (weak.upgrade(), value.get::<String>()) else {
                return false;
            };
            if from == id {
                return false;
            }
            let list = home::dropped(&ctx.config.borrow().pinned, &from, id);
            ctx.set_setting("home.pinned", json!(list));
            true
        });
        card.root.add_controller(drop);

        // Bubbles up from whatever inside the card has focus.
        let weak: Weak<Ctx> = Rc::downgrade(ctx);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let up = match key {
                gdk::Key::Up => true,
                gdk::Key::Down => false,
                _ => return glib::Propagation::Proceed,
            };
            if !mods.contains(gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            let Some(ctx) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let pinned = ctx.config.borrow().pinned.clone();
            let list = home::moved(&pinned, id, up);
            if list != pinned {
                on_moved(id);
                ctx.set_setting("home.pinned", json!(list));
            }
            glib::Propagation::Stop
        });
        card.root.add_controller(keys);
        Some(PinnedCard { id, widget })
    }
}
