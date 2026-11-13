//! The tray menu (plan 004): Open; one entry per tab the sidebar shows;
//! Settings; Quit daemon. The tabs are read from the daemon when the host
//! builds the menu, not when the tray starts: the daemon is not accepting
//! yet then, and a hidden tab should leave the menu without a restart.

use super::item::{Item, capabilities, hidden};
use crate::cli::call;
use ksni::MenuItem;
use ksni::menu::StandardItem;
use mirqab_core::proto::{Request, strings};
use mirqab_view::shell;
use std::path::Path;

/// The tabs the sidebar would show now: id and label.
pub(super) fn tabs(socket: &Path) -> Vec<(&'static str, &'static str)> {
    shell::sidebar(shell::TABS, &capabilities(socket), &hidden(socket))
        .into_iter()
        .map(|t| (t.id, t.label))
        .collect()
}

fn entry(label: &str, activate: impl Fn(&mut Item) + Send + 'static) -> MenuItem<Item> {
    StandardItem {
        label: label.to_string(),
        activate: Box::new(activate),
        ..Default::default()
    }
    .into()
}

pub(super) fn items(item: &Item) -> Vec<MenuItem<Item>> {
    let mut out = vec![
        entry(strings::TRAY_OPEN, |i: &mut Item| i.ui("ui.show", None)),
        MenuItem::Separator,
    ];
    for (id, label) in tabs(&item.socket) {
        out.push(entry(label, move |i: &mut Item| i.ui("ui.show", Some(id))));
    }
    out.push(MenuItem::Separator);
    out.push(entry(
        strings::tab_label("settings").unwrap_or("settings"),
        |i: &mut Item| i.ui("ui.show", Some("settings")),
    ));
    out.push(entry(strings::TRAY_QUIT, |i: &mut Item| {
        let _ = call(&i.socket, Request::new(1, "shutdown"));
    }));
    out
}
