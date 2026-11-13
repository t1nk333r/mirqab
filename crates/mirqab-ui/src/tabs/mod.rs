//! One module per tab (architecture §7.2). Until a tab's plan lands, its page
//! is the placeholder naming that plan; Settings is built here from the start.

pub mod placeholder;
pub mod settings;

use crate::app::Ctx;
use std::rc::Rc;

/// Builds the page for `tab` (any id `pages::show` accepts).
pub fn build(tab: &str, ctx: &Rc<Ctx>) -> gtk::Widget {
    match tab {
        "settings" => settings::page::build(ctx),
        other => placeholder::build(other),
    }
}
