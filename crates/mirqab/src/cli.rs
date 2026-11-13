//! The CLI: connect, send one request, print the answer (architecture §11).
//! Plan 005 adds the read commands and `--watch`.

use crate::{Args, EXIT_FAILED, EXIT_NO_DAEMON, EXIT_USAGE};
use mirqab_core::proto::{Reply, Request, ServerFrame, frame, strings};
use mirqab_core::server;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

enum CallError {
    NoDaemon(String),
    Daemon(String, String),
}

/// Sends one request and returns its result.
fn call(socket: &Path, req: Request) -> Result<Value, CallError> {
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
    print!(
        "{}",
        strings::status_lines(
            v["version"].as_str().unwrap_or("?"),
            v["socket"].as_str().unwrap_or("?"),
            v["uptimeSec"].as_u64().unwrap_or(0),
            v["daemon"]["rssBytes"].as_u64().unwrap_or(0),
            &streams,
            &notes,
        )
    );
}

pub fn run(command: &str, args: Args) -> ExitCode {
    let Some(socket) = args.socket.clone().or_else(server::default_socket) else {
        eprintln!(
            "{}",
            strings::no_daemon(
                "$XDG_RUNTIME_DIR/mirqab/daemon.sock",
                "XDG_RUNTIME_DIR is not set"
            )
        );
        return ExitCode::from(EXIT_NO_DAEMON);
    };
    let mut json_out = false;
    let mut window = false;
    let mut dry_run = false;
    let mut tab = None;
    for a in &args.rest {
        match a.as_str() {
            "--json" if command == "status" => json_out = true,
            "--window" if command == "open" => window = true,
            "--dry-run" if command != "status" => dry_run = true,
            s if !s.starts_with('-')
                && tab.is_none()
                && (command == "open" || command == "toggle") =>
            {
                tab = Some(s.to_string())
            }
            other => return usage(&strings::unknown_option(other)),
        }
    }
    let req = match command {
        "status" => Request::new(1, "status"),
        _ => {
            let (name, args) = match command {
                "open" => (
                    "ui.show",
                    json!({"tab": tab, "mode": if window { "window" } else { "popover" }}),
                ),
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
            } else if command == "status" || dry_run {
                println!("{v}");
            }
            ExitCode::SUCCESS
        }
        Err(CallError::NoDaemon(why)) => {
            eprintln!(
                "{}",
                strings::no_daemon(&socket.display().to_string(), &why)
            );
            ExitCode::from(EXIT_NO_DAEMON)
        }
        Err(CallError::Daemon(code, message)) => {
            eprintln!("{}", strings::daemon_error(&code, &message));
            ExitCode::from(EXIT_FAILED)
        }
    }
}
