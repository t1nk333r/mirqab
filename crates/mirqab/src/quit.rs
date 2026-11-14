//! `mirqab quit PID|NAME [-y] [--force] [--dry-run] [--json]` (plan 006,
//! part 2): Quit (SIGTERM) or, with `--force`, Force quit (SIGKILL), on a
//! process or an app of the `apps` view.
//!
//! Always two calls to the daemon: the first returns `needs-confirm` with
//! a plan and the `wouldDo` list, which is printed and confirmed (plan
//! 005's flow, `confirm.rs`); the second sends the plan with
//! `confirm: true`. So `-y` costs two calls as well. A name is matched
//! whole and ignoring case against the apps' names; several matches are
//! listed and nothing is guessed.

use crate::apps::parse_apps;
use crate::cli::{CallError, call, exit_for, socket_or_exit};
use crate::confirm::{self, Answer};
use crate::{Args, EXIT_CONFIRM, EXIT_FAILED, EXIT_USAGE};
use mirqab_core::proto::stream::{APPS, APPS_KILL, APPS_QUIT};
use mirqab_core::proto::{Code, Request, strings};
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

fn fail(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_FAILED)
}

/// The lines of a `wouldDo` list and its `skipped` list.
pub(crate) fn plan_lines(would_do: &Value, skipped: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for w in would_do.as_array().into_iter().flatten() {
        if let Some(argv) = w["run"].as_array() {
            let argv: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
            let procs = w["pids"].as_array().map_or(0, Vec::len);
            out.push(strings::quit_run_line(&argv.join(" "), procs));
        } else {
            out.push(strings::quit_signal_line(
                w["signal"].as_str().unwrap_or("?"),
                w["pid"].as_u64().unwrap_or(0) as u32,
                w["name"].as_str().unwrap_or(""),
                w["via"].as_u64().map(|v| v as u32),
            ));
        }
    }
    out.extend(skipped_lines(skipped));
    out
}

fn skipped_lines(skipped: &Value) -> Vec<String> {
    skipped
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            strings::quit_skipped_line(
                s["pid"].as_u64().unwrap_or(0) as u32,
                s["name"].as_str().unwrap_or(""),
                s["reason"].as_str().unwrap_or(""),
            )
        })
        .collect()
}

/// The lines of what was done, and whether anything failed.
pub(crate) fn result_lines(v: &Value) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let argv = |e: &Value| -> String {
        e["run"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default()
            .join(" ")
    };
    for d in v["done"].as_array().into_iter().flatten() {
        if d["run"].is_array() {
            out.push(strings::quit_unit_done_line(&argv(d)));
        } else {
            out.push(strings::quit_done_line(
                d["signal"].as_str().unwrap_or("?"),
                d["pid"].as_u64().unwrap_or(0) as u32,
                d["name"].as_str().unwrap_or(""),
            ));
        }
    }
    for g in v["gone"].as_array().into_iter().flatten() {
        out.push(strings::quit_gone_line(
            g["pid"].as_u64().unwrap_or(0) as u32
        ));
    }
    let failed = v["failed"].as_array().map_or(&[][..], Vec::as_slice);
    for f in failed {
        let what = if f["run"].is_array() {
            argv(f)
        } else {
            format!(
                "{} {}",
                f["pid"].as_u64().unwrap_or(0),
                f["name"].as_str().unwrap_or("")
            )
        };
        out.push(strings::quit_failed_line(
            &what,
            f["error"].as_str().unwrap_or(""),
        ));
    }
    out.extend(skipped_lines(&v["skipped"]));
    (out, !failed.is_empty())
}

/// `{"pid": N}` for a number, else the one app whose name is `name`.
fn subject(socket: &Path, target: &str) -> Result<Value, ExitCode> {
    if target.bytes().all(|b| b.is_ascii_digit()) {
        return match target.parse::<u32>() {
            Ok(n) if n > 0 => Ok(json!({ "pid": n })),
            _ => Err(usage(&strings::bad_pid(target))),
        };
    }
    // Every group, not the first 50: a name further down must be found.
    let req = Request::new(1, "get")
        .with_stream(APPS)
        .with_params(json!({"sort": "name", "limit": u32::MAX}));
    let apps = call(socket, req).map_err(|e| exit_for(socket, e))?;
    let apps = parse_apps(apps).map_err(|m| fail(&m))?;
    let wanted = target.to_lowercase();
    let found: Vec<_> = apps
        .groups
        .iter()
        .filter(|g| g.name.to_lowercase() == wanted)
        .collect();
    match found.as_slice() {
        [] => Err(fail(&strings::quit_no_app(target))),
        [g] => Ok(json!({ "group": g.id })),
        many => {
            crate::out::err(&strings::quit_ambiguous(target, many.len()));
            for g in many {
                crate::out::err(&strings::quit_candidate(&g.id, &g.pids));
            }
            Err(ExitCode::from(EXIT_FAILED))
        }
    }
}

pub fn run(args: Args) -> ExitCode {
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let (mut yes, mut force, mut dry_run, mut json_out) = (false, false, false, false);
    let mut target: Option<&str> = None;
    for a in &args.rest {
        match a.as_str() {
            "-y" => yes = true,
            "--force" => force = true,
            "--dry-run" => dry_run = true,
            "--json" => json_out = true,
            s if target.is_none() && !s.starts_with('-') => target = Some(s),
            other => return usage(&strings::unknown_option(other)),
        }
    }
    let Some(target) = target else {
        return usage(&strings::command_help("quit").unwrap_or_default());
    };
    let subject = match subject(&socket, target) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let name = if force { APPS_KILL } else { APPS_QUIT };
    let header = if force {
        strings::FORCE_QUIT_WILL
    } else {
        strings::QUIT_WILL
    };
    let ask = |extra: Value| {
        let mut params = json!({"name": name, "args": subject});
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        call(&socket, Request::new(1, "action").with_params(params))
    };
    if dry_run {
        return match ask(json!({"dryRun": true})) {
            Ok(v) if json_out => {
                crate::out::line(&v.to_string());
                ExitCode::SUCCESS
            }
            Ok(v) => {
                confirm::ask_with(header, &plan_lines(&v["wouldDo"], &v["skipped"]), yes, true);
                ExitCode::SUCCESS
            }
            Err(e) => exit_for(&socket, e),
        };
    }
    // The first call never acts: it answers needs-confirm with the plan.
    let data = match ask(json!({})) {
        Err(CallError::Daemon(code, _, Some(data))) if code == Code::NeedsConfirm.as_str() => data,
        Err(e) => return exit_for(&socket, e),
        Ok(_) => return fail(&strings::bad_daemon_answer(strings::QUIT_NO_CONFIRM_ASKED)),
    };
    let Some(plan) = data["plan"].as_str() else {
        return fail(&strings::bad_daemon_answer(strings::QUIT_NO_PLAN));
    };
    match confirm::ask_with(
        header,
        &plan_lines(&data["wouldDo"], &data["skipped"]),
        yes,
        false,
    ) {
        Answer::Go => {}
        Answer::DryRun | Answer::No => return ExitCode::from(EXIT_CONFIRM),
    }
    match ask(json!({"confirm": true, "plan": plan})) {
        Ok(v) if json_out => {
            crate::out::line(&v.to_string());
            if v["failed"].as_array().is_some_and(|f| !f.is_empty()) {
                ExitCode::from(EXIT_FAILED)
            } else {
                ExitCode::SUCCESS
            }
        }
        Ok(v) => {
            let (lines, failed) = result_lines(&v);
            for l in lines {
                crate::out::line(&l);
            }
            if failed {
                ExitCode::from(EXIT_FAILED)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => exit_for(&socket, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_and_results_print_one_plain_line_each() {
        let would = json!([
            {"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser"},
            {"signal": "TERM", "pid": 2010, "start": 5100, "name": "x\u{1b}[2J", "via": 2011},
            {"run": ["systemctl", "--user", "stop", "--no-block", "--", "a.service"],
             "unit": "a.service", "pids": [1, 2]},
        ]);
        let skipped = json!([{"pid": 1050, "name": "Hyprland", "reason": "the compositor"}]);
        let lines = plan_lines(&would, &skipped);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("2001") && lines[0].contains("browser"));
        assert!(
            lines[1].contains("2011"),
            "the thread asked for: {}",
            lines[1]
        );
        assert!(lines[2].contains("2 processes"));
        assert!(lines[3].contains("the compositor"));
        for l in &lines {
            assert!(!l.chars().any(char::is_control), "{l:?}");
        }
        let (out, failed) = result_lines(&json!({
            "done": [{"signal": "TERM", "pid": 2001, "name": "browser"}],
            "gone": [{"pid": 2010, "start": 5100}],
            "failed": [],
            "skipped": [],
        }));
        assert_eq!(out.len(), 2);
        assert!(!failed);
        let (_, failed) = result_lines(&json!({"failed": [{"pid": 1, "name": "x", "error": "e"}]}));
        assert!(failed);
    }
}
