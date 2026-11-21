//! `mirqab quit PID|NAME|--group ID [-y] [--force] [--dry-run] [--json]`
//! (plan 006, part 2): Quit (SIGTERM) or, with `--force`, Force quit
//! (SIGKILL), on a process or an app of the `apps` view.
//!
//! Always two calls to the daemon: the first returns `needs-confirm` with
//! a plan and the `wouldDo` list, which is printed and confirmed (plan
//! 005's flow, `confirm.rs`); the second sends the plan with
//! `confirm: true`. So `-y` costs two calls as well. A name is matched
//! whole and ignoring case against the apps' names, or by the cut form
//! `mirqab apps` shows (`names`); several matches are listed and nothing
//! is guessed.

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

/// The lines of a plan: its `wouldDo` list, each service's `alsoStops`
/// under its stop (plan 006, part 3), the `skipped` list, then the
/// `warnings` the daemon gave (the sound server, a portal).
pub(crate) fn plan_lines(data: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for w in data["wouldDo"].as_array().into_iter().flatten() {
        if let Some(argv) = w["run"].as_array() {
            let argv: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
            let procs = w["pids"].as_array().map_or(0, Vec::len);
            out.push(strings::quit_run_line(&argv.join(" "), procs));
            for d in w["alsoStops"].as_array().into_iter().flatten() {
                let unit = d["unit"].as_str().unwrap_or("");
                out.push(strings::quit_also_stops_line(
                    unit,
                    d["pids"].as_array().map_or(0, Vec::len),
                    mirqab_view::tabs::apps::deeper(unit),
                ));
            }
            if w["alsoStopsUnknown"] == Value::Bool(true) {
                out.push(strings::QUIT_ALSO_STOPS_UNKNOWN.to_string());
            }
        } else {
            out.push(strings::quit_signal_line(
                w["signal"].as_str().unwrap_or("?"),
                w["pid"].as_u64().unwrap_or(0) as u32,
                w["name"].as_str().unwrap_or(""),
                w["via"].as_u64().map(|v| v as u32),
            ));
        }
    }
    out.extend(skipped_lines(&data["skipped"]));
    for w in data["warnings"].as_array().into_iter().flatten() {
        out.push(strings::quit_warning_line(w.as_str().unwrap_or("")));
    }
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

/// Whether `target` names the app called `name`, ignoring case: the whole
/// name, or the form `mirqab apps` shows a name too long for its column,
/// its first characters and an ellipsis, taken as a prefix (audit
/// 2026-11-21, L46). Only that shown form is a prefix: a shorter cut names
/// nothing, so a few letters and `-y` never quit an app by guess.
fn names(name: &str, target: &str) -> bool {
    let (name, target) = (name.to_lowercase(), target.to_lowercase());
    if name == target {
        return true;
    }
    let width = strings::APPS_NAME_WIDTH;
    target.strip_suffix('\u{2026}').is_some_and(|prefix| {
        prefix.chars().count() == width - 1
            && name.chars().count() > width
            && name.starts_with(prefix)
    })
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
    let found: Vec<_> = apps
        .groups
        .iter()
        .filter(|g| names(&g.name, target))
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
    let mut group: Option<&str> = None;
    let mut it = args.rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-y" => yes = true,
            "--force" => force = true,
            "--dry-run" => dry_run = true,
            "--json" => json_out = true,
            // One app by its id, when several share a name (audit
            // 2026-11-21, L47); the daemon resolves it, every process of it.
            "--group" if group.is_none() => match it.next().map(String::as_str) {
                Some(id) if !id.is_empty() && !id.starts_with('-') => group = Some(id),
                _ => return usage(&strings::missing_value("--group")),
            },
            s if target.is_none() && !s.starts_with('-') => target = Some(s),
            other => return usage(&strings::unknown_option(other)),
        }
    }
    let subject = match (target, group) {
        (None, Some(id)) => json!({ "group": id }),
        (Some(target), None) => match subject(&socket, target) {
            Ok(s) => s,
            Err(code) => return code,
        },
        _ => return usage(&strings::command_help("quit").unwrap_or_default()),
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
                confirm::ask_with(header, &plan_lines(&v), yes, true, false);
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
    match confirm::ask_with(header, &plan_lines(&data), yes, false, json_out) {
        Answer::Go => {}
        Answer::DryRun | Answer::No => return ExitCode::from(EXIT_CONFIRM),
    }
    match ask(json!({"confirm": true, "plan": plan})) {
        Ok(v) => {
            if json_out {
                crate::out::line(&v.to_string());
            } else {
                for l in result_lines(&v).0 {
                    crate::out::line(&l);
                }
            }
            // The act ran, but its audit row could not be written: said
            // in both modes (owner, 2026-11-14: act and warn).
            if v["recorded"] == Value::Bool(false) {
                crate::out::err(strings::QUIT_NOT_RECORDED);
            }
            ExitCode::from(exit_after(&v))
        }
        Err(e) => exit_for(&socket, e),
    }
}

/// The exit code after a confirmed call (decision 11, review of PR #17):
/// 1 when anything failed, or when nothing was done and something was left
/// alone (refused or started since the confirmation); 0 otherwise, so a
/// target that had already exited is still success.
pub(crate) fn exit_after(v: &Value) -> u8 {
    let empty = |k: &str| v[k].as_array().is_none_or(Vec::is_empty);
    if !empty("failed") || (empty("done") && !empty("skipped")) {
        EXIT_FAILED
    } else {
        0
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
        let lines = plan_lines(&json!({"wouldDo": would, "skipped": skipped}));
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

    #[test]
    fn a_stop_lists_what_it_takes_with_it_and_the_warnings() {
        let data = json!({
            "wouldDo": [
                {"run": ["systemctl", "--user", "stop", "--no-block", "--", "pipewire.service"],
                 "unit": "pipewire.service", "pids": [1210],
                 "alsoStops": [{"unit": "wireplumber.service", "pids": [1211]},
                               {"unit": "x\u{1b}.target", "pids": []}]},
                {"run": ["systemctl", "--user", "stop", "--no-block", "--", "a.service"],
                 "unit": "a.service", "pids": [1, 2], "alsoStopsUnknown": true},
            ],
            "skipped": [],
            "warnings": ["pipewire.service is part of your sound\n"],
        });
        let lines = plan_lines(&data);
        assert_eq!(lines.len(), 6, "{lines:?}");
        assert_eq!(lines[1], "  also stops wireplumber.service (1 process)");
        assert!(lines[2].contains("(0 processes)"));
        // A target takes its own dependents with it, unlisted (review of
        // PR #18, S3).
        assert!(lines[2].ends_with("and what depends on it"), "{}", lines[2]);
        assert_eq!(lines[4], strings::QUIT_ALSO_STOPS_UNKNOWN);
        assert!(lines[5].starts_with("warning: pipewire.service"));
        for l in &lines {
            assert!(!l.chars().any(char::is_control), "{l:?}");
        }
    }

    #[test]
    fn a_name_is_matched_whole_or_by_the_form_apps_shows() {
        let long = "Example Browser Developer Edition X";
        let row = strings::apps_row(long, "0.0%", "1 MiB", 1);
        let shown = row.split("  ").next().unwrap().trim_end();
        assert_eq!(shown, "Example Browser Developer E\u{2026}");
        assert!(names(long, shown));
        assert!(names(long, &shown.to_uppercase()));
        assert!(names(long, long));
        assert!(names("Foot", "foot"));
        assert!(!names(long, "Example Browser\u{2026}"), "a shorter cut");
        assert!(!names(long, "Example Browser Developer E"), "no ellipsis");
        // A name of exactly the width is shown whole, never cut.
        let exact = "Example Browser Developer Ed";
        assert!(!names(exact, "Example Browser Developer E\u{2026}"));
    }

    #[test]
    fn nothing_done_and_something_left_alone_is_a_failure() {
        let d = json!({"pid": 1});
        let reply = |done: Value, gone: Value, failed: Value, skipped: Value| {
            exit_after(&json!({"done": done, "gone": gone, "failed": failed, "skipped": skipped}))
        };
        assert_eq!(reply(json!([d]), json!([]), json!([]), json!([])), 0);
        assert_eq!(reply(json!([]), json!([d]), json!([]), json!([])), 0);
        assert_eq!(
            reply(json!([]), json!([]), json!([]), json!([d])),
            EXIT_FAILED
        );
        assert_eq!(reply(json!([d]), json!([]), json!([]), json!([d])), 0);
        assert_eq!(
            reply(json!([]), json!([]), json!([d]), json!([])),
            EXIT_FAILED
        );
    }
}
