//! The tray icon (plan 004): a StatusNotifierItem served by the daemon
//! through `ksni`, on ksni's own thread. The item is a client of the
//! daemon's socket like any other: a click sends `ui.toggle`, the tooltip
//! one `get` when the host reads it, Quit `shutdown`. It holds no
//! subscription, so a tray with the window closed costs no sampling.

mod item;
mod menu;

use ksni::blocking::{Handle, TrayMethods};
use mirqab_core::proto::strings;
use mirqab_core::server;
use std::path::Path;

pub use item::Item;

/// Starts the tray for the daemon on `socket`, on a thread of its own:
/// ksni builds the menu while it registers, and the menu asks the daemon,
/// which answers only once it serves. Without a session bus it logs why;
/// without a tray host it waits for one (a `NameOwnerChanged` match, no
/// polling) and registers when it appears, and again whenever the host
/// restarts.
pub fn start(socket: &Path) {
    let socket = socket.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            // The handle lives as long as the daemon.
            let _handle = spawn(&socket);
            loop {
                std::thread::park();
            }
        });
    if let Err(e) = spawned {
        server::log(&strings::tray_failed(&e.to_string()));
    }
}

fn spawn(socket: &Path) -> Option<Handle<Item>> {
    match Item::new(socket).assume_sni_available(true).spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            server::log(&strings::tray_failed(&e.to_string()));
            None
        }
    }
}
