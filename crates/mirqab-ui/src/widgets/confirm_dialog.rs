//! The confirmation before an action: an `adw::AlertDialog` listing the
//! `wouldDo` lines a dry run returned (architecture §5.4).

use adw::prelude::*;
use mirqab_proto::strings;
use mirqab_view::shell::would_do_line;
use serde_json::Value;

/// The response id of the confirming button.
pub const CONFIRM: &str = "confirm";

/// The lines the dialog shows for `would_do`.
pub fn lines(would_do: &[Value]) -> Vec<String> {
    would_do.iter().map(would_do_line).collect()
}

pub fn confirm_dialog(heading: &str, would_do: &[Value]) -> adw::AlertDialog {
    let d = adw::AlertDialog::new(Some(heading), Some(&lines(would_do).join("\n")));
    d.add_response("cancel", strings::CANCEL);
    d.add_response(CONFIRM, strings::CONFIRM);
    d.set_response_appearance(CONFIRM, adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d
}
