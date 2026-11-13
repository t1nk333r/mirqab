//! The page of a tab whose plan has not landed: one empty state naming the
//! plan. One module for all sixteen instead of sixteen copies (plan 001,
//! "Phase D as built"); each tab's plan replaces its entry in `tabs::build`.

use crate::widgets::empty_state::empty_state;
use gtk::prelude::*;
use mirqab_proto::strings;
use mirqab_view::shell;

pub fn build(tab: &str) -> gtk::Widget {
    let plan = shell::find(tab).map_or(0, |t| t.plan);
    empty_state(
        &strings::placeholder_title(tab),
        &strings::placeholder_body(plan),
    )
    .upcast()
}
