//! One module per tab (architecture §7.2). A tab whose plan has not landed
//! shows the placeholder naming that plan.

pub mod placeholder;
pub mod settings;
pub mod system;

use crate::app::Ctx;
use std::rc::Rc;

/// Builds the page for `tab` (any id `pages::show` accepts). A page with a
/// stream calls `ctx.page_ready` on its first data; the others are ready
/// at once.
pub fn build(tab: &str, ctx: &Rc<Ctx>) -> gtk::Widget {
    match tab {
        "system" => system::page::build(ctx),
        "settings" => {
            let w = settings::page::build(ctx);
            ctx.page_ready(tab);
            w
        }
        other => {
            let w = placeholder::build(other);
            ctx.page_ready(other);
            w
        }
    }
}
