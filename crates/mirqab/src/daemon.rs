//! `mirqab daemon`: checks the runtime directory, binds or adopts the socket,
//! tells systemd it is ready (sd_notify without a library) and serves.

use crate::{Args, EXIT_FAILED, EXIT_USAGE};
use mirqab_core::config::Config;
use mirqab_core::history::History;
use mirqab_core::proto::strings;
use mirqab_core::server::{self, ListenError, Options};
use mirqab_core::{Root, sys};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::ExitCode;

fn fail(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_FAILED)
}

/// The runtime directory must exist and belong to this user: the socket in it
/// is the daemon's only door (architecture §5.1).
fn check_runtime_dir() -> Result<(), String> {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) else {
        return Err(strings::no_runtime_dir().to_string());
    };
    let dir = PathBuf::from(dir);
    match std::fs::metadata(&dir) {
        Ok(m) if m.is_dir() && m.uid() == sys::uid() => Ok(()),
        _ => Err(strings::runtime_dir_not_ours(&dir.display().to_string())),
    }
}

/// READY=1 to $NOTIFY_SOCKET, a path or an abstract name (`@…`).
fn notify_ready() {
    let Some(target) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    let Ok(sock) = UnixDatagram::unbound() else {
        return;
    };
    let target = target.to_string_lossy().into_owned();
    if let Some(name) = target.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt;
        if let Ok(addr) = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()) {
            let _ = sock.send_to_addr(b"READY=1", &addr);
        }
    } else {
        let _ = sock.send_to(b"READY=1", &target);
    }
}

pub fn run(args: Args) -> ExitCode {
    let mut root = Root::host();
    let mut fixture = false;
    let mut it = args.rest.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => match it.next() {
                Some(v) => {
                    fixture = std::path::Path::new(&v) != std::path::Path::new("/");
                    root = Root::new(v);
                }
                None => {
                    crate::out::err(&strings::missing_value("--root"));
                    return ExitCode::from(EXIT_USAGE);
                }
            },
            other => {
                crate::out::err(&strings::unknown_option(other));
                return ExitCode::from(EXIT_USAGE);
            }
        }
    }
    if let Err(msg) = check_runtime_dir() {
        return fail(&msg);
    }
    let Some(socket) = args.socket.or_else(server::default_socket) else {
        return fail(strings::no_runtime_dir());
    };
    // A daemon reading a fixture tree keeps its history and config beside
    // its socket, never in the user's: fixture numbers must not land in the
    // real history (review of phase C, deferred to phase E).
    let (config_path, history) = match socket.parent().filter(|_| fixture) {
        Some(dir) => (dir.join("config.json"), dir.join("history.db")),
        None => (Config::default_path(), History::default_path()),
    };
    let config_path_for_tray = config_path.clone();
    let server = match server::listen(Options {
        socket: socket.clone(),
        root,
        config_path,
        samplers: Vec::new(),
        history: Some(history),
    }) {
        Ok(s) => s,
        Err(ListenError::AlreadyRunning) => {
            return fail(&strings::already_running(&socket.display().to_string()));
        }
        Err(ListenError::Failed(why)) => return fail(&format!("mirqab: {why}")),
    };
    // Not out::print: the daemon must not exit because no one reads its
    // stdout, so a failed write is ignored.
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(
            out,
            "{}",
            strings::ready(&server.socket().display().to_string())
        )
        .and_then(|_| out.flush());
    }
    notify_ready();
    // Read once at start (plan 004).
    if Config::load(&config_path_for_tray)
        .get("tray.enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
    {
        crate::tray::start(&socket);
    }
    let err = server.serve();
    fail(&format!("mirqab: accept: {err}"))
}
