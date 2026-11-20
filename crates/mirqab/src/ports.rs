//! `mirqab ports` and `mirqab port PORT` (plan 007): the `ports` stream,
//! read once. Part 1b fills the project, launcher and container columns.

use crate::cli::{call, exit_for, socket_or_exit};
use crate::{Args, EXIT_FAILED, EXIT_USAGE};
use mirqab_core::proto::stream::{PORTS, PortRow, PortsData};
use mirqab_core::proto::{Request, strings};
use mirqab_view::format;
use serde_json::{Value, json};
use std::process::ExitCode;

/// The daemon's `ports` answer, or the message for one this mirqab cannot
/// read: an empty list would pass for "nothing listens".
pub(crate) fn parse_ports(v: Value) -> Result<PortsData, String> {
    serde_json::from_value(v).map_err(|e| strings::bad_daemon_answer(&e.to_string()))
}

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

/// Who holds the port, for the PROCESS column.
fn process_cell(r: &PortRow) -> String {
    match (r.owner.as_str(), r.pid) {
        ("you", Some(pid)) => strings::ports_process(r.process.as_deref().unwrap_or(""), pid),
        ("you", None) => strings::PORTS_YOURS_UNKNOWN.to_string(),
        ("root", _) => strings::PORTS_ROOT.to_string(),
        ("docker", _) => r
            .container
            .as_ref()
            .and_then(|c| c.name.as_deref())
            .map(|n| strings::plain(n).into_owned())
            .unwrap_or_else(|| strings::PORTS_NONE.to_string()),
        _ => strings::ports_other(r.uid),
    }
}

/// Who holds the port, in words, for `mirqab port`.
fn process_long(r: &PortRow) -> String {
    match (r.owner.as_str(), r.pid) {
        ("you", Some(_)) | ("docker", _) => process_cell(r),
        ("you", None) => strings::PORTS_YOURS_UNKNOWN_LONG.to_string(),
        ("root", _) => strings::PORTS_ROOT_LONG.to_string(),
        _ => strings::ports_other_long(r.uid),
    }
}

fn or_none(v: Option<String>) -> String {
    v.unwrap_or_else(|| strings::PORTS_NONE.to_string())
}

/// `mirqab ports`' lines: a header, a row per listener, then a line for
/// each port of yours the network can reach.
pub(crate) fn ports_lines(d: &PortsData) -> Vec<String> {
    let mut out = vec![strings::PORTS_HEADER.to_string()];
    for r in &d.ports {
        out.push(strings::ports_row(
            r.port,
            &r.proto,
            &r.bind,
            &r.scope,
            &process_cell(r),
            &or_none(r.project.as_ref().map(|p| p.name.clone())),
            &or_none(
                r.launcher
                    .as_ref()
                    .map(|l| l.name.clone().unwrap_or_else(|| l.kind.clone())),
            ),
            &or_none(r.uptime_sec.map(format::duration)),
        ));
    }
    for r in d.ports.iter().filter(|r| r.owner == "you" && r.exposed) {
        out.push(strings::ports_exposed_line(
            r.port,
            &r.proto,
            &r.bind,
            &process_cell(r),
        ));
    }
    out
}

/// `mirqab port`'s lines: a block per listener on the port, blank lines
/// between. A field the daemon does not know is left out.
pub(crate) fn port_lines(rows: &[&PortRow]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        let mut line = |label: &str, value: &str| out.push(strings::port_line(label, value));
        line(
            strings::PORT_PORT,
            &strings::port_where(r.port, &r.proto, &r.bind, &r.families),
        );
        line(strings::PORT_REACH, strings::reach_sentence(&r.scope));
        line(strings::PORT_PROCESS, &process_long(r));
        if let Some(l) = &r.label {
            line(strings::PORT_LABEL, l);
        }
        if let Some(p) = &r.project {
            line(
                strings::PORT_PROJECT,
                &strings::port_project(&p.name, &p.path),
            );
            if let Some(f) = &p.framework {
                line(strings::PORT_FRAMEWORK, f);
            }
        }
        if let Some(s) = &r.shell {
            line(strings::PORT_SHELL, &strings::ports_process(&s.name, s.pid));
        }
        if let Some(l) = &r.launcher {
            line(strings::PORT_LAUNCHER, l.name.as_deref().unwrap_or(&l.kind));
        }
        if let Some(u) = &r.unit {
            line(strings::PORT_UNIT, u);
        }
        if let Some(c) = &r.container {
            line(strings::PORT_CONTAINER, c.name.as_deref().unwrap_or(&c.id));
        }
        if let Some(up) = r.uptime_sec {
            line(strings::PORT_UP, &format::duration(up));
        }
        line(strings::PORT_URL, &r.url);
    }
    out
}

pub fn run(command: &str, args: Args) -> ExitCode {
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let (mut json_out, mut udp, mut port, mut proto) = (false, false, None, "tcp");
    let mut it = args.rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json_out = true,
            "--udp" if command == "ports" => udp = true,
            "--proto" if command == "port" => match it.next().map(String::as_str) {
                Some("tcp") => proto = "tcp",
                Some("udp") => proto = "udp",
                Some(other) => return usage(&strings::bad_proto(other)),
                None => return usage(&strings::missing_value("--proto")),
            },
            s if command == "port" && port.is_none() && !s.starts_with('-') => {
                match s.parse::<u16>() {
                    Ok(n) if n > 0 => port = Some(n),
                    _ => return usage(&strings::bad_port(s)),
                }
            }
            other => return usage(&strings::unknown_option(other)),
        }
    }
    if command == "port" {
        if port.is_none() {
            return usage(&strings::command_help("port").unwrap_or_default());
        }
        udp = proto == "udp";
    }
    let req = Request::new(1, "get")
        .with_stream(PORTS)
        .with_params(json!({ "udp": udp }));
    let v = match call(&socket, req) {
        Ok(v) => v,
        Err(e) => return exit_for(&socket, e),
    };
    if json_out && command == "ports" {
        crate::out::line(&v.to_string());
        return ExitCode::SUCCESS;
    }
    let d = match parse_ports(v) {
        Ok(d) => d,
        Err(msg) => {
            crate::out::err(&msg);
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let lines = match port {
        None => ports_lines(&d),
        Some(n) => {
            let rows: Vec<&PortRow> = d
                .ports
                .iter()
                .filter(|r| r.port == n && r.proto == proto)
                .collect();
            if rows.is_empty() {
                crate::out::err(&strings::port_not_listening(n, proto));
                return ExitCode::from(EXIT_FAILED);
            }
            if json_out {
                crate::out::line(&serde_json::to_string(&rows).unwrap_or_default());
                return ExitCode::SUCCESS;
            }
            port_lines(&rows)
        }
    };
    for l in lines {
        crate::out::line(&l);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(port: u16, owner: &str, pid: Option<u32>) -> PortRow {
        PortRow {
            port,
            proto: "tcp".into(),
            bind: "0.0.0.0".into(),
            families: vec![4],
            scope: "all".into(),
            exposed: true,
            owner: owner.into(),
            uid: Some(if owner == "root" { 0 } else { 1000 }),
            pid,
            process: pid.map(|_| "srv\u{1b}[2J\nFAKE".into()),
            url: format!("http://localhost:{port}"),
            ..Default::default()
        }
    }

    #[test]
    fn a_name_cannot_forge_rows_and_only_your_exposed_rows_get_a_line() {
        let d = PortsData {
            ports: vec![row(22, "root", None), row(8000, "you", Some(6600))],
            ..Default::default()
        };
        let lines = ports_lines(&d);
        assert_eq!(lines.len(), 4, "a header, two rows, one warning: {lines:?}");
        assert!(lines[1].contains(strings::PORTS_ROOT), "{:?}", lines[1]);
        assert!(lines[3].starts_with("exposed: :8000/tcp"), "{:?}", lines[3]);
        for l in &lines {
            assert!(!l.chars().any(char::is_control), "{l:?}");
        }
        let one = row(8000, "you", Some(6600));
        for l in port_lines(&[&one]) {
            assert!(!l.chars().any(char::is_control), "{l:?}");
        }
    }

    #[test]
    fn a_containers_name_cannot_reach_the_terminal() {
        let mut docker = row(8088, "docker", None);
        docker.container = Some(mirqab_core::proto::stream::ContainerRef {
            id: "d0c4e7a1b2c3".into(),
            name: Some("web\u{1b}[2J".into()),
            image: None,
        });
        assert!(!process_cell(&docker).contains('\u{1b}'));
        let d = PortsData {
            ports: vec![docker.clone()],
            ..Default::default()
        };
        for l in ports_lines(&d).iter().chain(port_lines(&[&docker]).iter()) {
            assert!(!l.contains('\u{1b}'), "{l:?}");
        }
    }

    #[test]
    fn an_answer_this_mirqab_cannot_read_is_an_error() {
        let err = parse_ports(json!({"ports": "x"})).unwrap_err();
        assert!(err.contains("restart the daemon"), "{err}");
        assert!(parse_ports(json!({"ports": []})).is_ok());
    }
}
