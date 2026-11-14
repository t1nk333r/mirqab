//! The tray (plan 004, Tests) on a private session bus with a stand-in
//! watcher: the daemon registers once the watcher appears and again after
//! it restarts; Activate becomes a `ui` toggle event; the tooltip reads
//! without leaving a subscription; the menu follows `tabs.hidden` when the
//! host is about to show it; a slow window start does not stall the tray,
//! and its failure is logged; `tray.enabled=false` registers nothing. No
//! test touches the real session bus or the user's systemd.

#[path = "support/watcher.rs"]
mod watcher;

use mirqab_view::proto::strings;
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
/// written as its config.json first. Its `systemctl` is a stub that waits
/// `$STUB_SLEEP` seconds and fails (no window ever starts); its stderr goes
/// to `stderr` in its scratch dir.
fn start(name: &str, bus: &Bus, config: Option<&str>) -> Daemon {
    start_with(name, bus, config, 0)
}

fn start_with(name: &str, bus: &Bus, config: Option<&str>, stub_sleep: u32) -> Daemon {
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
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("systemctl"),
        "#!/bin/sh\nsleep \"$STUB_SLEEP\"\necho 'Unit mirqab-ui.service not found.' >&2\nexit 5\n",
    )
    .unwrap();
    std::fs::set_permissions(
        bin.join("systemctl"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let stderr = std::fs::File::create(dir.join("stderr")).unwrap();
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", &dir)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .env("PATH", path)
        .env("STUB_SLEEP", stub_sleep.to_string())
        .env("MIRQAB_TEST", "1")
        .env_remove("MIRQAB_SOCKET")
        .env_remove("NOTIFY_SOCKET")
        .stdout(Stdio::piped())
        .stderr(stderr)
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
    // Attached before the click, or the daemon would start a window.
    lines
        .by_ref()
        .map(|l| serde_json::from_str::<serde_json::Value>(&l.unwrap()).unwrap())
        .find(|v| v["type"] == "reply" && v["id"] == 1)
        .expect("the subscribe reply");
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
    // A name only: Quickshell never falls back to pixels when a name is
    // set, and looks up a theme path without the file's extension (plan 021).
    assert_eq!(item.get_property::<String>("IconName").unwrap(), "mirqab");
    assert_eq!(
        item.get_property::<String>("IconName").unwrap(),
        strings::TRAY_ICON
    );
    assert_eq!(item.get_property::<String>("IconThemePath").unwrap(), "");
    let pixmaps: Vec<(i32, i32, Vec<u8>)> = item.get_property("IconPixmap").unwrap();
    assert!(pixmaps.is_empty());
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

/// The menu layout's labels, as one string to search.
fn layout(conn: &zbus::blocking::Connection, service: &str) -> String {
    let menu =
        zbus::blocking::Proxy::new(conn, service, "/MenuBar", "com.canonical.dbusmenu").unwrap();
    let reply = menu
        .call_method("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .unwrap();
    let body = reply.body();
    let layout: zbus::zvariant::Structure = body.deserialize().unwrap();
    format!("{layout:?}")
}

#[test]
fn the_menu_follows_hidden_tabs_when_the_host_shows_it() {
    let Some(bus) = bus() else {
        eprintln!("dbus-daemon is not installed: skipped");
        return;
    };
    let (conn, registered) = watcher::serve(&bus.address);
    let d = start("menu", &bus, None);
    let service = registered.recv_timeout(WAIT).expect("a registration");
    let before = layout(&conn, &service);
    assert!(
        before.contains("\"System\"") && before.contains("\"Quit daemon\""),
        "{before}"
    );
    request(
        &d,
        "{\"id\":3,\"method\":\"config.set\",\"params\":{\"key\":\"tabs.hidden\",\"value\":[\"system\"]}}\n",
    );
    let menu = zbus::blocking::Proxy::new(
        &conn,
        service.as_str(),
        "/MenuBar",
        "com.canonical.dbusmenu",
    )
    .unwrap();
    let _: bool = menu.call("AboutToShow", &(0i32,)).unwrap();
    let after = layout(&conn, &service);
    assert!(
        !after.contains("\"System\""),
        "hidden without a click: {after}"
    );
    // The rest stays: an unreachable daemon would empty the tab list too.
    assert!(
        after.contains("\"Home\"") && after.contains("\"Quit daemon\""),
        "{after}"
    );
}

#[test]
fn a_slow_window_start_does_not_stall_the_tray_and_its_failure_is_logged() {
    let Some(bus) = bus() else {
        eprintln!("dbus-daemon is not installed: skipped");
        return;
    };
    let (conn, registered) = watcher::serve(&bus.address);
    // No interface attached: the click starts one, and the stub takes 2 s
    // to fail.
    let d = start_with("slow", &bus, None, 2);
    let service = registered.recv_timeout(WAIT).expect("a registration");
    let item = zbus::blocking::Proxy::new(
        &conn,
        service.as_str(),
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
    )
    .unwrap();
    let t = std::time::Instant::now();
    let _: () = item.call("Activate", &(0i32, 0i32)).unwrap();
    let _: ToolTip = item.get_property("ToolTip").unwrap();
    assert!(
        t.elapsed() < Duration::from_millis(1500),
        "the tray answered while the window started: {:?}",
        t.elapsed()
    );
    let log = d.dir.join("stderr");
    let mut text = String::new();
    for _ in 0..100 {
        text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("tray: ui.toggle failed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(text.contains("tray: ui.toggle failed"), "{text}");
}
