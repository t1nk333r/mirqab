//! The `mirqab` binary end to end: the daemon on a fixture root, and the
//! window commands reaching an attached interface as `ui` events (plan 001,
//! "Tests": `open system` produces a `ui` event).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_mirqab");

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

fn start(name: &str) -> Daemon {
    let dir = std::env::temp_dir().join(format!("mirqab-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop");
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
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

fn mirqab(d: &Daemon, args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .arg("--socket")
        .arg(&d.socket)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn open_reaches_the_interface_as_a_ui_event() {
    let d = start("open");
    let ui = UnixStream::connect(&d.socket).unwrap();
    ui.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    let hello = lines.next().unwrap().unwrap();
    assert!(hello.contains("\"type\":\"hello\""));
    (&ui)
        .write_all(b"{\"id\":1,\"method\":\"subscribe\",\"stream\":\"ui\",\"params\":{\"role\":\"ui\",\"pid\":4321}}\n")
        .unwrap();
    assert!(
        lines
            .next()
            .unwrap()
            .unwrap()
            .contains("\"type\":\"reply\"")
    );

    let out = mirqab(&d, &["open", "system"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let event: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(
        event,
        serde_json::json!({"type": "event", "stream": "ui", "data": {"event": "show", "tab": "system", "mode": "popover"}})
    );

    assert!(mirqab(&d, &["toggle"]).status.success());
    assert!(
        lines
            .next()
            .unwrap()
            .unwrap()
            .contains("\"event\":\"toggle\"")
    );
    assert!(mirqab(&d, &["close"]).status.success());
    assert!(
        lines
            .next()
            .unwrap()
            .unwrap()
            .contains("\"event\":\"hide\"")
    );
}

#[test]
fn open_without_an_interface_fails_cleanly() {
    let d = start("noui");
    let out = mirqab(&d, &["open"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no interface is running"));
}

#[test]
fn a_second_daemon_refuses_the_socket() {
    let d = start("twice");
    let out = Command::new(BIN)
        .args(["daemon", "--socket"])
        .arg(&d.socket)
        .env("XDG_RUNTIME_DIR", d.dir.join("run"))
        .env("XDG_CONFIG_HOME", d.dir.join("config"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already listens"));
}
