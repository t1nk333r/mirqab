//! The CLI: connect, send one request, print the answer (architecture §11).
//! Plan 005 adds the read commands and `--watch`; `bar` is in `bar/`.

use crate::{Args, EXIT_FAILED, EXIT_NO_DAEMON, EXIT_USAGE};
use mirqab_core::proto::stream::SystemData;
use mirqab_core::proto::{Reply, Request, ServerFrame, frame, strings};
use mirqab_core::server;
use mirqab_view::format;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

pub(crate) enum CallError {
    NoDaemon(String),
    /// The daemon took the request but sent no reply within this many
    /// seconds. It is running, and an action may have run (audit: a slow
    /// reply used to read as "no daemon").
    Timeout(u64),
    /// The daemon's error: its code, message and `data` (a `needs-confirm`
    /// reply's plan and `wouldDo`, plan 006).
    Daemon(String, String, Option<Value>),
}

/// How long a read waits for its reply.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an `action` waits. The daemon bounds every step itself, but a
/// confirmed quit runs `systemctl show` twice and `stop` and `kill` per
/// unit (2 s and 5 s timeouts each), so a group of several units on a slow
/// user manager can pass 10 s while still succeeding.
const ACTION_TIMEOUT: Duration = Duration::from_secs(60);

fn timeout_for(req: &Request) -> Duration {
    if req.method == "action" {
        ACTION_TIMEOUT
    } else {
        READ_TIMEOUT
    }
}

/// Sends one request and returns its result.
pub(crate) fn call(socket: &Path, req: Request) -> Result<Value, CallError> {
    let timeout = timeout_for(&req);
    call_within(socket, req, timeout)
}

fn call_within(socket: &Path, req: Request, timeout: Duration) -> Result<Value, CallError> {
    let stream = UnixStream::connect(socket).map_err(|e| CallError::NoDaemon(e.to_string()))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let mut writer = &stream;
    let mut reader = BufReader::new(&stream);
    let id = req.id;
    writer
        .write_all(&frame::encode(&req))
        .map_err(|e| CallError::NoDaemon(e.to_string()))?;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err(CallError::NoDaemon("the connection closed".into())),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(CallError::Timeout(timeout.as_secs().max(1)));
            }
            Err(_) => return Err(CallError::NoDaemon("the connection closed".into())),
            Ok(_) => {}
        }
        if let Ok(ServerFrame::Reply(Reply {
            id: rid,
            result,
            error,
        })) = serde_json::from_str(&line)
        {
            if rid != id {
                continue;
            }
            return match error {
                Some(e) => Err(CallError::Daemon(e.code.as_str().into(), e.message, e.data)),
                None => Ok(result.unwrap_or(Value::Null)),
            };
        }
    }
}

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

fn print_status(v: &Value) {
    let streams: Vec<(String, u32, Option<u64>)> = v["streams"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, s)| {
                    (
                        k.clone(),
                        s["subscribers"].as_u64().unwrap_or(0) as u32,
                        s["intervalMs"].as_u64(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut notes = Vec::new();
    if let Some(why) = v["config"]["broken"].as_str() {
        notes.push(strings::config_broken(why));
    }
    // The focus source (plan 006, part 4), when the daemon says.
    if let Some(f) = v.get("focus") {
        notes.push(strings::status_focus(f["source"].as_str()));
    }
    if let Some(m) = v["unavailable"].as_object() {
        for (k, why) in m {
            notes.push(format!("unavailable {k}: {}", why.as_str().unwrap_or("")));
        }
    }
    crate::out::print(&strings::status_lines(
        v["version"].as_str().unwrap_or("?"),
        v["socket"].as_str().unwrap_or("?"),
        v["uptimeSec"].as_u64().unwrap_or(0),
        v["daemon"]["rssBytes"].as_u64().unwrap_or(0),
        &streams,
        &notes,
    ));
}

/// One line per bucket: unix time, series, average and peak; or, with
/// `--list`, one series per line.
fn print_history(v: &Value, list: bool) {
    if list {
        for s in v.as_array().into_iter().flatten() {
            crate::out::line(&format!(
                "{}\t{}",
                s["key"].as_str().unwrap_or("?"),
                s["unit"].as_str().unwrap_or("")
            ));
        }
        return;
    }
    for (key, s) in v["series"].as_object().into_iter().flatten() {
        if s.is_null() {
            crate::out::err(&strings::no_such_series(key));
            continue;
        }
        let unit = s["unit"].as_str().unwrap_or("");
        for p in s["points"].as_array().into_iter().flatten() {
            crate::out::line(&format!(
                "{}\t{key}\tavg {:.2}\tmax {:.2}\t{unit}",
                p[0].as_i64().unwrap_or(0),
                p[1].as_f64().unwrap_or(0.0),
                p[2].as_f64().unwrap_or(0.0)
            ));
        }
    }
}

pub(crate) fn system(v: &Value) -> SystemData {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| strings::NO_VALUE.to_string(), format::percent)
}

/// `mirqab load`: CPU, load, memory, swap, pressure, thermal word.
fn print_load(v: &Value) {
    for l in load_lines(&system(v)) {
        crate::out::line(&l);
    }
}

/// `mirqab load`'s lines, which the MCP tools reuse (plan 005).
pub(crate) fn load_lines(d: &SystemData) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = |k: &str, val: String| out.push(strings::load_line(k, &val));
    if let Some(c) = &d.cpu {
        line(
            strings::LOAD_CPU,
            strings::load_cpu(
                &pct(c.percent),
                &pct(c.user),
                &pct(c.system),
                &pct(c.iowait),
            ),
        );
    }
    if let Some(l) = &d.load {
        line(
            strings::LOAD_LOAD,
            strings::load_avgs(l.one, l.five, l.fifteen, l.runnable, l.tasks),
        );
    }
    if let Some(m) = &d.mem {
        line(
            strings::LOAD_MEMORY,
            strings::load_memory(
                &format::bytes(m.used_bytes),
                &format::bytes(m.total_bytes),
                &format::bytes(m.available_bytes),
            ),
        );
        if m.swap_total_bytes > 0 {
            line(
                strings::LOAD_SWAP,
                strings::load_swap(
                    &format::bytes(m.swap_used_bytes),
                    &format::bytes(m.swap_total_bytes),
                ),
            );
        }
    }
    if let Some(p) = &d.pressure {
        let some =
            |s: &Option<mirqab_core::proto::stream::Stall>| pct(s.as_ref().map(|s| s.some60));
        line(
            strings::LOAD_PRESSURE,
            strings::load_pressure(&some(&p.cpu), &some(&p.memory), &some(&p.io)),
        );
    }
    if let Some(t) = &d.thermal {
        line(
            strings::LOAD_THERMAL,
            strings::thermal_word(&t.state, t.throttling),
        );
    }
    out
}

/// `mirqab sensors`: temperatures, GPUs, power. Celsius; plan 005 reads
/// `units.temperature`.
fn print_sensors(v: &Value) {
    let d = system(v);
    let c = format::TempUnit::Celsius;
    for t in &d.temps {
        let crit = t.critical.map(|x| format::temperature(x, c));
        crate::out::line(&strings::sensor_line(
            strings::temp_kind(&t.kind),
            &t.label,
            &format::temperature(t.celsius, c),
            crit.as_deref(),
        ));
    }
    let watts = |w: f64| format!("{w:.1} {}", strings::WATTS);
    for g in &d.gpus {
        if g.suspended {
            crate::out::line(&strings::gpu_suspended(&g.name));
            continue;
        }
        let mut parts = Vec::new();
        parts.extend(g.celsius.map(|x| format::temperature(x, c)));
        parts.extend(g.watts.map(watts));
        parts.extend(g.busy_percent.map(|b| strings::busy(&format::percent(b))));
        parts.extend(g.mhz.map(strings::mhz));
        crate::out::line(&strings::gpu_line(&g.name, &parts));
    }
    if let Some(p) = &d.power {
        let pk = p.package_watts.map(watts);
        let bt = p.battery_watts.map(watts);
        crate::out::line(&strings::power_line(
            pk.as_deref(),
            bt.as_deref(),
            p.needs_helper,
        ));
    }
}

/// The socket to use, or the exit for having none.
pub(crate) fn socket_or_exit(args: &Args) -> Result<PathBuf, ExitCode> {
    args.socket
        .clone()
        .or_else(server::default_socket)
        .ok_or_else(|| {
            crate::out::err(&strings::no_daemon(
                "$XDG_RUNTIME_DIR/mirqab/daemon.sock",
                "XDG_RUNTIME_DIR is not set",
            ));
            ExitCode::from(EXIT_NO_DAEMON)
        })
}

pub(crate) fn no_daemon(socket: &Path, why: &str) {
    crate::out::err(&strings::no_daemon(&socket.display().to_string(), why));
}

/// The message and exit code for a failed call.
pub(crate) fn exit_for(socket: &Path, e: CallError) -> ExitCode {
    match e {
        CallError::NoDaemon(why) => {
            no_daemon(socket, &why);
            ExitCode::from(EXIT_NO_DAEMON)
        }
        CallError::Timeout(secs) => {
            crate::out::err(&strings::daemon_no_answer(secs));
            ExitCode::from(EXIT_FAILED)
        }
        CallError::Daemon(code, message, _) => {
            crate::out::err(&strings::daemon_error(&code, &message));
            ExitCode::from(EXIT_FAILED)
        }
    }
}

pub fn run(command: &str, args: Args) -> ExitCode {
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let mut json_out = false;
    let mut window = false;
    let mut dry_run = false;
    let mut tab = None;
    let mut list = false;
    let mut range = "1h".to_string();
    let mut series: Vec<String> = Vec::new();
    let mut it = args.rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" if matches!(command, "status" | "history" | "load" | "sensors") => {
                json_out = true
            }
            "--list" if command == "history" => list = true,
            "--range" if command == "history" => match it.next() {
                Some(r) => range = r.clone(),
                None => return usage(&strings::missing_value("--range")),
            },
            s if command == "history" && !s.starts_with('-') => series.push(s.to_string()),
            "--window" if command == "open" => window = true,
            "--dry-run" if matches!(command, "open" | "toggle" | "close") => dry_run = true,
            s if !s.starts_with('-')
                && tab.is_none()
                && (command == "open" || command == "toggle") =>
            {
                tab = Some(s.to_string())
            }
            other => return usage(&strings::unknown_option(other)),
        }
    }
    if command == "history" && !list && series.is_empty() {
        return usage(&strings::command_help("history").unwrap_or_default());
    }
    let req = match command {
        "status" => Request::new(1, "status"),
        "load" | "sensors" => Request::new(1, "get").with_stream("system"),
        "history" if list => Request::new(1, "history.list"),
        "history" => {
            Request::new(1, "history.query").with_params(json!({"series": series, "range": range}))
        }
        _ => {
            let (name, args) = match command {
                // A mode only with --window: a plain `open` uses the mode
                // the user chose in Settings (architecture §4).
                "open" if window => ("ui.show", json!({"tab": tab, "mode": "window"})),
                "open" => ("ui.show", json!({"tab": tab})),
                "toggle" => ("ui.toggle", json!({"tab": tab})),
                _ => ("ui.hide", json!({})),
            };
            Request::new(1, "action")
                .with_params(json!({"name": name, "args": args, "dryRun": dry_run}))
        }
    };
    match call(&socket, req) {
        Ok(v) => {
            if command == "status" && !json_out {
                print_status(&v);
            } else if command == "history" && !json_out {
                print_history(&v, list);
            } else if command == "load" && !json_out {
                print_load(&v);
            } else if command == "sensors" && !json_out {
                print_sensors(&v);
            } else if matches!(command, "status" | "history" | "load" | "sensors") || dry_run {
                crate::out::line(&format!("{v}"));
            }
            ExitCode::SUCCESS
        }
        Err(e) => exit_for(&socket, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A socket in a fresh scratch directory. Under /tmp, not $TMPDIR: a
    /// socket path must stay under 108 bytes.
    fn scratch_socket(name: &str) -> (PathBuf, UnixListener) {
        let dir = PathBuf::from("/tmp").join(format!("mqt-{name}-{}", std::process::id()));
        let _ = mirqab_core::act::remove_path(&dir, false);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        (socket, listener)
    }

    /// A daemon that reads the request, then keeps the connection open
    /// for `hold` (silent) or closes it at once.
    fn serve_once(listener: UnixListener, hold: Duration) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            let _ = BufReader::new(&stream).read_line(&mut line);
            std::thread::sleep(hold);
        })
    }

    #[test]
    fn a_slow_reply_is_a_timeout_not_a_missing_daemon() {
        let (socket, listener) = scratch_socket("slow");
        let handle = serve_once(listener, Duration::from_millis(1500));
        let r = call_within(
            &socket,
            Request::new(1, "status"),
            Duration::from_millis(300),
        );
        assert!(matches!(r, Err(CallError::Timeout(1))), "a timeout");
        // A timeout is a failure (1), not "no daemon" (2).
        assert_eq!(
            exit_for(&socket, CallError::Timeout(1)),
            ExitCode::from(EXIT_FAILED)
        );
        assert!(strings::daemon_no_answer(1).contains("did not answer"));
        assert!(!strings::daemon_no_answer(1).contains("start it with"));
        handle.join().unwrap();
        let _ = mirqab_core::act::remove_path(socket.parent().unwrap(), false);
    }

    #[test]
    fn a_closed_connection_is_still_no_daemon() {
        let (socket, listener) = scratch_socket("closed");
        let handle = serve_once(listener, Duration::ZERO);
        let r = call_within(&socket, Request::new(1, "status"), Duration::from_secs(5));
        assert!(matches!(r, Err(CallError::NoDaemon(_))), "no daemon");
        handle.join().unwrap();
        let _ = mirqab_core::act::remove_path(socket.parent().unwrap(), false);
    }

    #[test]
    fn an_action_waits_longer_than_a_read() {
        assert_eq!(timeout_for(&Request::new(1, "status")), READ_TIMEOUT);
        assert_eq!(timeout_for(&Request::new(1, "action")), ACTION_TIMEOUT);
        assert!(ACTION_TIMEOUT > READ_TIMEOUT);
    }
}
