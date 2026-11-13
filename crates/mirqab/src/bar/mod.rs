//! `mirqab bar` (plan 004): one reading of an item for a status bar, or,
//! with `--watch`, a line each time the reading changes. A bar item is never
//! `live`: it subscribes at 5 s by default and never under 2 s.

mod format;
pub mod items;

use crate::cli::{CallError, call, no_daemon, socket_or_exit};
use crate::{Args, EXIT_NO_DAEMON, EXIT_USAGE};
use format::Format;
use mirqab_core::proto::stream::{self, SystemData};
use mirqab_core::proto::{Request, ServerFrame, frame, strings};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

const DEFAULT_SECS: u64 = 5;
const MIN_SECS: u64 = 2;

fn usage(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(EXIT_USAGE)
}

struct Opts {
    item: String,
    watch: bool,
    secs: u64,
    format: Format,
}

fn parse(rest: &[String]) -> Result<Option<Opts>, ExitCode> {
    let (mut item, mut watch, mut secs, mut format) = (None, false, DEFAULT_SECS, Format::Waybar);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => {
                for (name, about) in strings::BAR_ITEMS {
                    println!("{name:<7} {about}");
                }
                return Ok(None);
            }
            "--watch" => watch = true,
            "--interval" => {
                let v = it
                    .next()
                    .ok_or_else(|| usage(&strings::missing_value("--interval")))?;
                secs = v
                    .parse()
                    .map_err(|_| usage(&strings::bar_bad_interval(v)))?;
            }
            "--format" => {
                let v = it
                    .next()
                    .ok_or_else(|| usage(&strings::missing_value("--format")))?;
                format = Format::parse(v).ok_or_else(|| usage(&strings::bar_bad_format(v)))?;
            }
            s if !s.starts_with('-') && item.is_none() => {
                if !items::known(s) {
                    return Err(usage(&strings::bar_unknown_item(s)));
                }
                item = Some(s.to_string());
            }
            other => return Err(usage(&strings::unknown_option(other))),
        }
    }
    let item = item.ok_or_else(|| usage(strings::BAR_NEEDS_ITEM))?;
    Ok(Some(Opts {
        item,
        watch,
        secs: secs.max(MIN_SECS),
        format,
    }))
}

/// `bar.combo` from the daemon's config; the default parts when it is
/// unset or unreadable.
fn combo_parts(socket: &std::path::Path) -> Vec<String> {
    call(socket, Request::new(1, "config.get"))
        .ok()
        .and_then(|v| v["bar"]["combo"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| items::COMBO_DEFAULT.iter().map(|s| s.to_string()).collect())
}

fn reading_line(o: &Opts, combo: &[String], data: &Value) -> String {
    let d: SystemData = serde_json::from_value(data.clone()).unwrap_or_default();
    // Known items always read; `parse` refused the others.
    let r = items::read(&o.item, &d, combo).unwrap_or(items::Reading {
        text: strings::NO_VALUE.to_string(),
        tooltip: String::new(),
        class: "unknown",
        percentage: None,
    });
    format::line(o.format, &o.item, &r)
}

/// Prints `line`; false once stdout is closed.
fn emit(line: &str) -> bool {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").and_then(|_| out.flush()).is_ok()
}

pub fn run(args: Args) -> ExitCode {
    let o = match parse(&args.rest) {
        Ok(Some(o)) => o,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return code,
    };
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let combo = if o.item == "combo" {
        combo_parts(&socket)
    } else {
        Vec::new()
    };
    if !o.watch {
        return match call(&socket, Request::new(1, "get").with_stream(stream::SYSTEM)) {
            Ok(v) => {
                emit(&reading_line(&o, &combo, &v));
                ExitCode::SUCCESS
            }
            Err(e) => crate::cli::exit_for(&socket, e),
        };
    }
    watch(&o, &combo, &socket)
}

fn watch(o: &Opts, combo: &[String], socket: &std::path::Path) -> ExitCode {
    let conn = match UnixStream::connect(socket) {
        Ok(c) => c,
        Err(e) => {
            no_daemon(socket, &e.to_string());
            return ExitCode::from(EXIT_NO_DAEMON);
        }
    };
    let req = Request::new(1, "subscribe")
        .with_stream(stream::SYSTEM)
        .with_params(json!({"interval": o.secs * 1000}));
    if (&conn).write_all(&frame::encode(&req)).is_err() {
        no_daemon(socket, "the connection closed");
        return ExitCode::from(EXIT_NO_DAEMON);
    }
    let mut reader = BufReader::new(&conn);
    let mut last = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => {
                no_daemon(socket, "the connection closed");
                return ExitCode::from(EXIT_NO_DAEMON);
            }
            Ok(_) => {}
        }
        match serde_json::from_str::<ServerFrame>(&line) {
            Ok(ServerFrame::Reply(r)) if r.id == 1 => {
                if let Some(e) = r.error {
                    return crate::cli::exit_for(
                        socket,
                        CallError::Daemon(e.code.as_str().into(), e.message),
                    );
                }
            }
            Ok(ServerFrame::Event(ev)) if ev.stream == stream::SYSTEM => {
                let out = reading_line(o, combo, &ev.data);
                if out != last {
                    if !emit(&out) {
                        return ExitCode::SUCCESS;
                    }
                    last = out;
                } else if mirqab_core::sys::stdout_closed() {
                    // Checked on every event, so an unchanged reading cannot
                    // keep a closed bar's process alive.
                    return ExitCode::SUCCESS;
                }
            }
            _ => {}
        }
    }
}
