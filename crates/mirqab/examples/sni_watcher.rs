//! A stand-in tray host for `tools/bench-idle.sh --tray`: owns
//! `org.kde.StatusNotifierWatcher` on the bus in DBUS_SESSION_BUS_ADDRESS
//! and prints `registered <service>` for each item, until killed.

#[path = "../tests/support/watcher.rs"]
mod watcher;

use std::io::Write;

fn main() {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("DBUS_SESSION_BUS_ADDRESS");
    let (_conn, registered) = watcher::serve(&address);
    for service in registered {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "registered {service}");
        let _ = out.flush();
    }
}
