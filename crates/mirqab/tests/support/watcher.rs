//! A stand-in for a tray host's `org.kde.StatusNotifierWatcher` on a
//! private bus (plan 004, Tests): it owns the name, answers that a host is
//! registered, and reports every item that registers. Shared by
//! `tests/tray.rs` and `examples/sni_watcher.rs` (the bench's watcher).

use std::sync::mpsc::{Receiver, Sender, channel};
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;

struct Watcher {
    registered: Sender<String>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        let _ = self.registered.send(service);
    }

    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// Serves the watcher on the bus at `address` until the returned
/// connection is dropped; the receiver yields each registered service.
pub fn serve(address: &str) -> (Connection, Receiver<String>) {
    let (tx, rx) = channel();
    let conn = Builder::address(address)
        .expect("a bus address")
        .serve_at("/StatusNotifierWatcher", Watcher { registered: tx })
        .expect("the watcher object")
        .name("org.kde.StatusNotifierWatcher")
        .expect("the watcher name")
        .build()
        .expect("a connection to the private bus");
    (conn, rx)
}
