//! The tray (plan 004, Tests) on a private session bus with a stand-in
//! watcher: the daemon registers once the watcher appears and again after
//! it restarts; Activate becomes a `ui` toggle event; the tooltip reads
//! without leaving a subscription; `tray.enabled=false` registers nothing.
//! No test touches the real session bus.

#[path = "support/watcher.rs"]
mod watcher;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_mirqab");

/// The StatusNotifierItem `ToolTip`: icon name, pixmaps, title, text.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);
const WAIT: Duration = Duration::from_secs(10);

struct Bus {
    child: Child,
    address: String,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A private session bus, or None where `dbus-daemon` is not installed.
fn bus() -> Option<Bus> {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut address = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut address)
        .ok()?;
    Some(Bus {
        child,
        address: address.trim().to_string(),
    })
}

struct Daemon {
    child: Child,
    dir: PathBuf,
    socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A daemon on root-desktop whose session bus is `bus`; `config` is
/// written as its config.json first.
fn start(name: &str, bus: &Bus, config: Option<&str>) -> Daemon {
    // Under /tmp, not $TMPDIR: a socket path must stay under 108 bytes.
    let dir = PathBuf::from("/tmp").join(format!("mqt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(run.join("mirqab")).unwrap();
    for d in [&run, &run.join("mirqab")] {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    if let Some(c) = config {
        std::fs::write(run.join("mirqab/config.json"), c).unwrap();
    }
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop");
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", &dir)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .env("MIRQAB_TEST", "1")
        .env_remove("MIRQAB_SOCKET")
        .env_remove("NOTIFY_SOCKET")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    let socket = run.join("mirqab/daemon.sock");
    assert_eq!(ready.trim(), format!("ready {}", socket.display()));
    Daemon { child, dir, socket }
}

/// One request on its own connection; its reply's result.
fn request(d: &Daemon, line: &str) -> serde_json::Value {
    let s = UnixStream::connect(&d.socket).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    (&s).write_all(line.as_bytes()).unwrap();
    for l in BufReader::new(&s).lines() {
        let v: serde_json::Value = serde_json::from_str(&l.unwrap()).unwrap();
        if v["type"] == "reply" {
            return v["result"].clone();
        }
    }
    panic!("no reply to {line}");
}

#[test]
fn registers_when_the_watcher_appears_and_after_it_restarts() {
    let Some(bus) = bus() else {
        eprintln!("dbus-daemon is not installed: skipped");
        return;
    };
    let _d = start("appear", &bus, None);
    // No watcher yet: the tray waits for the name, without polling.
    std::thread::sleep(Duration::from_millis(500));
    let (conn, registered) = watcher::serve(&bus.address);
    let first = registered.recv_timeout(WAIT).expect("a registration");
    assert!(first.starts_with("org.kde.StatusNotifierItem-"), "{first}");
    drop(conn);
    drop(registered);
    std::thread::sleep(Duration::from_millis(300));
    let (_conn, registered) = watcher::serve(&bus.address);
    assert_eq!(
        registered
            .recv_timeout(WAIT)
            .expect("a second registration"),
        first
    );
}

#[test]
fn activate_toggles_and_the_tooltip_leaves_no_subscription() {
    let Some(bus) = bus() else {
        eprintln!("dbus-daemon is not installed: skipped");
        return;
    };
    let (conn, registered) = watcher::serve(&bus.address);
    let d = start("activate", &bus, None);
    let service = registered.recv_timeout(WAIT).expect("a registration");

    // An attached interface receives the toggle as a `ui` event.
    let ui = UnixStream::connect(&d.socket).unwrap();
    ui.set_read_timeout(Some(WAIT)).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    (&ui)
        .write_all(b"{\"id\":1,\"method\":\"subscribe\",\"stream\":\"ui\",\"params\":{\"role\":\"ui\",\"pid\":4321}}\n")
        .unwrap();
    let item = zbus::blocking::Proxy::new(
        &conn,
        service.as_str(),
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
    )
    .unwrap();
    let _: () = item.call("Activate", &(0i32, 0i32)).unwrap();
    let toggle = lines
        .by_ref()
        .map(|l| serde_json::from_str::<serde_json::Value>(&l.unwrap()).unwrap())
        .find(|v| v["type"] == "event" && v["stream"] == "ui")
        .expect("a ui event");
    assert_eq!(toggle["data"]["event"], "toggle");

    let (_icon, _pixmaps, title, _text): ToolTip = item.get_property("ToolTip").unwrap();
    assert!(
        title.starts_with("CPU ") && title.contains("Mem 25%"),
        "{title}"
    );
    assert_eq!(
        item.get_property::<String>("Category").unwrap(),
        "SystemServices"
    );
    let status = request(&d, "{\"id\":2,\"method\":\"status\"}\n");
    assert_eq!(
        status["streams"]["system"]["subscribers"], 0,
        "the tooltip used a get, not a subscription"
    );
}

#[test]
fn a_disabled_tray_registers_nothing() {
    let Some(bus) = bus() else {
        eprintln!("dbus-daemon is not installed: skipped");
        return;
    };
    let (_conn, registered) = watcher::serve(&bus.address);
    let _d = start("off", &bus, Some(r#"{"tray": {"enabled": false}}"#));
    assert!(registered.recv_timeout(Duration::from_secs(2)).is_err());
}
