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
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        // History and anything else the daemon keeps stay in the scratch
        // dir, never in the developer's home.
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", &dir)
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
