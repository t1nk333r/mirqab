//! The CLI: connect, send one request, print the answer (architecture §11).
//! Plan 005 adds the read commands and `--watch`; `bar` is in `bar/`.

use crate::{Args, EXIT_FAILED, EXIT_NO_DAEMON, EXIT_USAGE};
use mirqab_core::proto::stream::SystemData;
use mirqab_core::proto::{Reply, Request, ServerFrame, frame, strings};
use mirqab_core::server;
use mirqab_view::format;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

pub(crate) enum CallError {
    NoDaemon(String),
    Daemon(String, String),
}

/// Sends one request and returns its result.
pub(crate) fn call(socket: &Path, req: Request) -> Result<Value, CallError> {
    let stream = UnixStream::connect(socket).map_err(|e| CallError::NoDaemon(e.to_string()))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
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
            Ok(0) | Err(_) => return Err(CallError::NoDaemon("the connection closed".into())),
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
                Some(e) => Err(CallError::Daemon(e.code.as_str().into(), e.message)),
                None => Ok(result.unwrap_or(Value::Null)),
            };
        }
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("{msg}");
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
    if let Some(m) = v["unavailable"].as_object() {
        for (k, why) in m {
            notes.push(format!("unavailable {k}: {}", why.as_str().unwrap_or("")));
        }
    }
    crate::out::print(
        &strings::status_lines(
            v["version"].as_str().unwrap_or("?"),
            v["socket"].as_str().unwrap_or("?"),
            v["uptimeSec"].as_u64().unwrap_or(0),
            v["daemon"]["rssBytes"].as_u64().unwrap_or(0),
            &streams,
            &notes,
        )
        .to_string(),
    );
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
            eprintln!("mirqab: history keeps no series named {key}");
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

fn system(v: &Value) -> SystemData {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| strings::NO_VALUE.to_string(), format::percent)
}

/// `mirqab load`: CPU, load, memory, swap, pressure, thermal word.
fn print_load(v: &Value) {
    let d = system(v);
    let line = |k: &str, val: String| crate::out::line(&strings::load_line(k, &val).to_string());
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
}

/// `mirqab sensors`: temperatures, GPUs, power. Celsius; plan 005 reads
/// `units.temperature`.
fn print_sensors(v: &Value) {
    let d = system(v);
    let c = format::TempUnit::Celsius;
    for t in &d.temps {
        let crit = t.critical.map(|x| format::temperature(x, c));
        crate::out::line(
            &strings::sensor_line(
                strings::temp_kind(&t.kind),
                &t.label,
                &format::temperature(t.celsius, c),
                crit.as_deref(),
            )
            .to_string(),
        );
    }
    let watts = |w: f64| format!("{w:.1} {}", strings::WATTS);
    for g in &d.gpus {
        if g.suspended {
            crate::out::line(&strings::gpu_suspended(&g.name).to_string());
            continue;
        }
        let mut parts = Vec::new();
        parts.extend(g.celsius.map(|x| format::temperature(x, c)));
        parts.extend(g.watts.map(watts));
        parts.extend(g.busy_percent.map(|b| strings::busy(&format::percent(b))));
        parts.extend(g.mhz.map(strings::mhz));
        crate::out::line(&strings::gpu_line(&g.name, &parts).to_string());
    }
    if let Some(p) = &d.power {
        let pk = p.package_watts.map(watts);
        let bt = p.battery_watts.map(watts);
        crate::out::line(
            &strings::power_line(pk.as_deref(), bt.as_deref(), p.needs_helper).to_string(),
        );
    }
}

/// The socket to use, or the exit for having none.
pub(crate) fn socket_or_exit(args: &Args) -> Result<PathBuf, ExitCode> {
    args.socket
        .clone()
        .or_else(server::default_socket)
        .ok_or_else(|| {
            eprintln!(
                "{}",
                strings::no_daemon(
                    "$XDG_RUNTIME_DIR/mirqab/daemon.sock",
                    "XDG_RUNTIME_DIR is not set"
                )
            );
            ExitCode::from(EXIT_NO_DAEMON)
        })
}

pub(crate) fn no_daemon(socket: &Path, why: &str) {
    eprintln!("{}", strings::no_daemon(&socket.display().to_string(), why));
}

/// The message and exit code for a failed call.
pub(crate) fn exit_for(socket: &Path, e: CallError) -> ExitCode {
    match e {
        CallError::NoDaemon(why) => {
            no_daemon(socket, &why);
            ExitCode::from(EXIT_NO_DAEMON)
        }
        CallError::Daemon(code, message) => {
            eprintln!("{}", strings::daemon_error(&code, &message));
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
