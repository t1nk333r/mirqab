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
use std::time::{Duration, Instant};

const DEFAULT_SECS: u64 = 5;
const MIN_SECS: u64 = 2;
/// An hour: past it a bar would show its first reading all day.
const MAX_SECS: u64 = 3600;
/// Events come at the shortest interval any subscriber asked for, so the
/// bar holds a changed reading back until its own interval has passed,
/// less this, so jitter does not skip every other event.
const SLACK: Duration = Duration::from_millis(250);

/// Whether a changed reading may print now: always after a reading with no
/// value (the first real one should not wait), otherwise once `secs` (less
/// the slack) have passed since the last line (review of PR #10: with the
/// window open live, `--interval 5` printed every second).
struct Throttle {
    last_emit: Option<Instant>,
    last_unknown: bool,
}

impl Throttle {
    fn allow(&self, now: Instant, secs: u64) -> bool {
        match self.last_emit {
            None => true,
            Some(_) if self.last_unknown => true,
            Some(at) => now.saturating_duration_since(at) + SLACK >= Duration::from_secs(secs),
        }
    }

    fn emitted(&mut self, now: Instant, unknown: bool) {
        self.last_emit = Some(now);
        self.last_unknown = unknown;
    }
}

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
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
                    crate::out::line(&format!("{name:<7} {about}"));
                }
                return Ok(None);
            }
            "--watch" => watch = true,
            "--interval" => {
                let v = it
                    .next()
                    .ok_or_else(|| usage(&strings::missing_value("--interval")))?;
                secs = v
                    .parse::<u64>()
                    .map_err(|_| usage(&strings::bar_bad_interval(v)))?
                    .min(MAX_SECS);
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

/// The output line and whether it carries no value yet.
fn reading_line(o: &Opts, combo: &[String], data: &Value) -> (String, bool) {
    let d: SystemData = serde_json::from_value(data.clone()).unwrap_or_default();
    // Known items always read; `parse` refused the others.
    let r = items::read(&o.item, &d, combo).unwrap_or(items::Reading {
        text: strings::NO_VALUE.to_string(),
        tooltip: String::new(),
        class: "unknown",
        percentage: None,
    });
    (format::line(o.format, &o.item, &r), r.class == "unknown")
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
                crate::out::line(&reading_line(&o, &combo, &v).0);
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
        .with_params(json!({"interval": o.secs.saturating_mul(1000)}));
    if (&conn).write_all(&frame::encode(&req)).is_err() {
        no_daemon(socket, "the connection closed");
        return ExitCode::from(EXIT_NO_DAEMON);
    }
    let mut reader = BufReader::new(&conn);
    let mut last = String::new();
    let mut throttle = Throttle {
        last_emit: None,
        last_unknown: false,
    };
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
                let (out, unknown) = reading_line(o, combo, &ev.data);
                let now = Instant::now();
                if out != last && throttle.allow(now, o.secs) {
                    // A closed bar ends the process inside out::line.
                    crate::out::line(&out);
                    throttle.emitted(now, unknown);
                    last = out;
                } else if mirqab_core::sys::stdout_closed() {
                    // Checked on every event that prints nothing, held back
                    // or unchanged, so it cannot keep a closed bar's process
                    // alive.
                    return ExitCode::SUCCESS;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_reading_waits_for_the_interval_unless_the_last_had_no_value() {
        let t0 = Instant::now();
        let mut t = Throttle {
            last_emit: None,
            last_unknown: false,
        };
        assert!(t.allow(t0, 5), "the first line");
        t.emitted(t0, false);
        assert!(!t.allow(t0 + Duration::from_secs(1), 5), "held back at 1 s");
        assert!(
            !t.allow(t0 + Duration::from_millis(4700), 5),
            "still held at 4.7 s"
        );
        assert!(
            t.allow(t0 + Duration::from_millis(4800), 5),
            "jitter allowed"
        );
        // After a value, a reading with no value is held like any change.
        t.emitted(t0, true);
        assert!(
            t.allow(t0 + Duration::from_secs(1), 5),
            "after a reading with no value, the first real one goes at once"
        );
    }

    #[test]
    fn the_interval_is_clamped_to_two_seconds_and_an_hour() {
        let secs = |v: &str| {
            parse(&["cpu".into(), "--interval".into(), v.into()])
                .ok()
                .flatten()
                .map(|o| o.secs)
        };
        assert_eq!(secs("1"), Some(MIN_SECS));
        assert_eq!(secs("99999999999999999"), Some(MAX_SECS));
        assert_eq!(secs("7"), Some(7));
    }
}
