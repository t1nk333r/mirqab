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
    // Under /tmp, not $TMPDIR: a socket path must stay under 108 bytes.
    let dir = PathBuf::from("/tmp").join(format!("mqc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop");
    // `open` runs `systemctl --user start mirqab-ui.service`; a stub records
    // the call, and fails when the file `fail` exists in the scratch dir.
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("systemctl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"$STUB_LOG\"\n\
         if [ -f \"$STUB_FAIL\" ]; then echo 'Unit mirqab-ui.service not found.' >&2; exit 5; fi\n",
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
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        // History and anything else the daemon keeps stay in the scratch
        // dir, never in the developer's home.
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", &dir)
        .env("PATH", path)
        .env("STUB_LOG", dir.join("systemctl.log"))
        .env("STUB_FAIL", dir.join("fail"))
        .env("MIRQAB_TEST", "1")
        .env("MIRQAB_UI_START_MS", "2000")
        // No tray on the developer's desktop: tests/tray.rs has its own bus.
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
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

/// Runs a command that must exit by itself; a daemon that wrongly starts and
/// serves is killed after 5 s and fails the test instead of hanging it.
fn output_within(cmd: &mut Command) -> std::process::Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("still running after 5 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
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
        serde_json::json!({"type": "event", "stream": "ui", "data": {"event": "show", "tab": "system", "mode": null}})
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

fn stub_log(d: &Daemon) -> String {
    std::fs::read_to_string(d.dir.join("systemctl.log")).unwrap_or_default()
}

#[test]
fn open_starts_the_interface_and_queues_the_event() {
    let d = start("start");
    let mut open = Command::new(BIN)
        .arg("--socket")
        .arg(&d.socket)
        .args(["open", "system"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The interface "starts": it attaches a moment after the unit was asked
    // for, and hears the queued event right after its subscribe reply.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while stub_log(&d).is_empty() {
        assert!(std::time::Instant::now() < deadline, "systemctl never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(stub_log(&d), "--user start mirqab-ui.service\n");
    let ui = UnixStream::connect(&d.socket).unwrap();
    ui.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    lines.next().unwrap().unwrap();
    (&ui)
        .write_all(b"{\"id\":7,\"method\":\"subscribe\",\"stream\":\"ui\",\"params\":{\"role\":\"ui\",\"pid\":4321}}\n")
        .unwrap();
    let reply = lines.next().unwrap().unwrap();
    assert!(
        reply.contains("\"type\":\"reply\"") && reply.contains("\"id\":7"),
        "{reply}"
    );
    let event: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(
        event["data"],
        serde_json::json!({"event": "show", "tab": "system", "mode": null})
    );
    let status = open.wait().unwrap();
    assert!(status.success());
    // With the interface attached, no second start.
    assert!(mirqab(&d, &["open"]).status.success());
    assert_eq!(stub_log(&d).lines().count(), 1);
}

/// Connects as the interface; returns the line reader after the subscribe
/// reply.
fn attach(d: &Daemon, timeout: Duration) -> std::io::Lines<BufReader<UnixStream>> {
    let ui = UnixStream::connect(&d.socket).unwrap();
    ui.set_read_timeout(Some(timeout)).unwrap();
    let mut lines = BufReader::new(ui.try_clone().unwrap()).lines();
    lines.next().unwrap().unwrap();
    (&ui)
        .write_all(b"{\"id\":7,\"method\":\"subscribe\",\"stream\":\"ui\",\"params\":{\"role\":\"ui\",\"pid\":4321}}\n")
        .unwrap();
    let reply = lines.next().unwrap().unwrap();
    assert!(reply.contains("\"id\":7"), "{reply}");
    lines
}

fn spawn_cli(d: &Daemon, args: &[&str]) -> Child {
    Command::new(BIN)
        .arg("--socket")
        .arg(&d.socket)
        .args(args)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_for_starts(d: &Daemon, n: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while stub_log(d).lines().count() < n {
        assert!(
            std::time::Instant::now() < deadline,
            "systemctl ran {} times",
            stub_log(d).lines().count()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn close_while_starting_clears_the_queued_show() {
    let d = start("closestart");
    let mut open = spawn_cli(&d, &["open", "system"]);
    wait_for_starts(&d, 1);
    assert!(mirqab(&d, &["close"]).status.success());
    let mut lines = attach(&d, Duration::from_millis(500));
    assert!(
        lines.next().is_none_or(|l| l.is_err()),
        "no show after close"
    );
    assert!(open.wait().unwrap().success(), "the interface attached");
}

#[test]
fn a_new_open_after_a_timed_out_start_starts_again() {
    let d = start("restart");
    let out = output_within(Command::new(BIN).arg("--socket").arg(&d.socket).arg("open"));
    assert_eq!(out.status.code(), Some(1), "nothing attached");
    let mut open = spawn_cli(&d, &["open", "network"]);
    wait_for_starts(&d, 2);
    let mut lines = attach(&d, Duration::from_secs(5));
    let event: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(event["data"]["tab"], "network");
    assert!(open.wait().unwrap().success());
}

#[test]
fn an_open_joining_a_timed_out_start_does_not_report_success() {
    // Review of PR #5: B joined A's start; A timed out and cleared the
    // event; an interface attached later; B said done with nothing sent.
    let d = start("joined");
    let mut a = spawn_cli(&d, &["open", "system"]);
    wait_for_starts(&d, 1);
    let mut b = spawn_cli(&d, &["open", "settings"]);
    let a_status = a.wait().unwrap();
    let b_status = b.wait().unwrap();
    assert_eq!(a_status.code(), Some(1));
    assert_eq!(b_status.code(), Some(1), "one attempt, one outcome");
    assert_eq!(stub_log(&d).lines().count(), 1, "started once");
}

#[test]
fn open_without_an_interface_fails_cleanly() {
    let d = start("noui");
    // Nothing to hide: done, and no unit started.
    assert!(mirqab(&d, &["close"]).status.success());
    assert_eq!(stub_log(&d), "");
    let out = output_within(Command::new(BIN).arg("--socket").arg(&d.socket).arg("open"));
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("did not connect"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stub_log(&d), "--user start mirqab-ui.service\n");
}

#[test]
fn a_unit_that_fails_to_start_is_reported() {
    let d = start("unitfail");
    std::fs::write(d.dir.join("fail"), "").unwrap();
    let out = output_within(
        Command::new(BIN)
            .arg("--socket")
            .arg(&d.socket)
            .arg("toggle"),
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("could not start mirqab-ui.service"), "{err}");
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn a_fixture_daemon_keeps_history_beside_its_socket() {
    let d = start("fixturehist");
    let out = mirqab(&d, &["status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let path = status["history"]["path"].as_str().unwrap().to_string();
    assert_eq!(
        std::path::Path::new(&path),
        d.socket.parent().unwrap().join("history.db")
    );
    assert!(
        !d.dir.join("state/mirqab").exists(),
        "nothing in XDG_STATE_HOME"
    );
}

#[test]
fn a_second_daemon_cannot_share_a_history_file() {
    let dir = PathBuf::from("/tmp").join(format!("mqc-hist2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let daemon = |sock: &str| {
        Command::new(BIN)
            .args(["daemon", "--socket"])
            .arg(run.join(sock))
            .env("XDG_RUNTIME_DIR", &run)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("HOME", &dir)
            .env_remove("NOTIFY_SOCKET")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut first = daemon("a.sock");
    let mut ready = String::new();
    BufReader::new(first.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.starts_with("ready "), "{ready}");
    let out = output_within(
        Command::new(BIN)
            .args(["daemon", "--socket"])
            .arg(run.join("b.sock"))
            .env("XDG_RUNTIME_DIR", &run)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("HOME", &dir)
            .env_remove("NOTIFY_SOCKET"),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("in use by another mirqab daemon"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = first.kill();
    let _ = first.wait();
    let _ = std::fs::remove_dir_all(&dir);
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

#[test]
fn a_file_or_directory_at_the_socket_path_is_never_removed() {
    let dir = PathBuf::from("/tmp").join(format!("mqc-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(run.join("precious/sub")).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(run.join("precious/sub/file"), "keep").unwrap();
    std::fs::write(run.join("notes.txt"), "keep").unwrap();
    std::os::unix::fs::symlink(run.join("notes.txt"), run.join("link")).unwrap();
    for target in ["precious", "notes.txt", "link"] {
        let out = output_within(
            Command::new(BIN)
                .args(["daemon", "--socket"])
                .arg(run.join(target))
                .env("XDG_RUNTIME_DIR", &run)
                .env("XDG_CONFIG_HOME", dir.join("config"))
                .env("XDG_STATE_HOME", dir.join("state"))
                .env("HOME", &dir),
        );
        assert_eq!(out.status.code(), Some(1), "{target}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("is not a socket"),
            "{target}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(run.join("precious/sub/file")).unwrap(),
        "keep"
    );
    assert_eq!(
        std::fs::read_to_string(run.join("notes.txt")).unwrap(),
        "keep"
    );
    assert!(
        std::fs::symlink_metadata(run.join("link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_daemon_refuses_even_when_the_socket_file_is_gone() {
    // Two daemons racing both find a dead socket; the lock decides. Removing
    // the first daemon's socket file stands in for the race.
    let d = start("lock");
    std::fs::remove_file(&d.socket).unwrap();
    let out = Command::new(BIN)
        .args(["daemon", "--socket"])
        .arg(&d.socket)
        .env("XDG_RUNTIME_DIR", d.dir.join("run"))
        .env("XDG_CONFIG_HOME", d.dir.join("config"))
        .env("XDG_STATE_HOME", d.dir.join("state"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already listens"));
}
