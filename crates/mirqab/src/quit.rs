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
    let failed = failed_lines(v);
    let any = !failed.is_empty();
    out.extend(failed);
    out.extend(skipped_lines(&v["skipped"]));
    (out, any)
}

/// The lines of what failed, one per entry of `failed`.
pub(crate) fn failed_lines(v: &Value) -> Vec<String> {
    v["failed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let what = if let Some(a) = f["run"].as_array() {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                format!(
                    "{} {}",
                    f["pid"].as_u64().unwrap_or(0),
                    f["name"].as_str().unwrap_or("")
                )
            };
            strings::quit_failed_line(&what, f["error"].as_str().unwrap_or(""))
        })
        .collect()
}

/// Whether `target` names the app called `name`, ignoring case: its whole
/// name, or the form `mirqab apps` shows (`strings::shown_name`: control
/// and bidi characters as U+FFFD, a name too long for the column cut with
/// an ellipsis; audit 2026-11-21, L46). The target is compared with that
/// shown form, never taken as a prefix of the raw name, so an app always
/// matches what was shown for it, and any other app shown the same way
/// matches too and makes the name ambiguous: `-y` never quits a look-alike
/// in its place. A shorter cut names nothing.
fn names(name: &str, target: &str) -> bool {
    let t = fold(target);
    fold(name) == t || fold(&strings::shown_name(name)) == t
}

/// `s` with its case folded one character at a time, so a character folds
/// the same wherever a cut leaves it: `str::to_lowercase` makes a last 'Σ'
/// a final 'ς', which a cut name and the whole name disagree on. 'ς' folds
/// to 'σ', as Unicode case folding does (review of audit 2026-11-21, L46).
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{3c2}' { '\u{3c3}' } else { c })
        .collect()
}

/// The pid `target` names when it is a number: `None` for a name, an
/// error for 0 or a number too large to be a pid.
fn pid_of(target: &str) -> Result<Option<u32>, ExitCode> {
    if !target.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    match target.parse::<u32>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(usage(&strings::bad_pid(target))),
    }
}

/// The one app whose name is `target`.
fn subject(socket: &Path, target: &str) -> Result<Value, ExitCode> {
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
    // One of a name or pid, or --group, never both or neither.
    let usage_error = || usage(&strings::command_help("quit").unwrap_or_default());
    if target.is_some() == group.is_some() {
        return usage_error();
    }
    // A pid that is not one is a usage error too (plan 024 L30).
    let pid = match target.map(pid_of).transpose() {
        Ok(p) => p.flatten(),
        Err(code) => return code,
    };
    // The arguments first: a usage error is 64 even with no socket to
    // name (audit 2026-11-21).
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let subject = match (pid, target, group) {
        (Some(n), _, _) => json!({ "pid": n }),
        (None, None, Some(id)) => json!({ "group": id }),
        (None, Some(target), _) => match subject(&socket, target) {
            Ok(s) => s,
            Err(code) => return code,
        },
        (None, None, None) => return usage_error(),
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
            // A reader that went away does not hide a failed act: what
            // failed goes to stderr, in both modes (review of audit
            // 2026-11-21, L40).
            if crate::out::stdout_gone() {
                for l in failed_lines(&v) {
                    crate::out::err(&l);
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
    fn the_failed_lines_are_those_result_lines_prints_for_failures() {
        // What goes to stderr when stdout is gone (review of L40).
        let v = json!({
            "done": [{"signal": "TERM", "pid": 2001, "name": "browser"}],
            "failed": [
                {"pid": 2010, "name": "x\u{1b}[2J", "error": "no such process"},
                {"run": ["systemctl", "--user", "stop", "--no-block", "--", "a.service"],
                 "error": "exit 1"},
            ],
            "skipped": [{"pid": 1050, "name": "Hyprland", "reason": "the compositor"}],
        });
        let failed = failed_lines(&v);
        assert_eq!(
            failed,
            [
                "failed 2010 x\u{fffd}[2J: no such process",
                "failed systemctl --user stop --no-block -- a.service: exit 1",
            ]
        );
        let (all, any) = result_lines(&v);
        assert!(any);
        assert_eq!(all[1..3], failed[..]);
        assert!(failed_lines(&json!({"done": []})).is_empty());
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

    /// The name column of the row `mirqab apps` prints for `name`.
    fn shown(name: &str) -> String {
        let row = strings::apps_row(name, "0.0%", "1 MiB", 1);
        let col: String = row.chars().take(strings::APPS_NAME_WIDTH).collect();
        col.trim_end().to_string()
    }

    /// The apps of `all` that `target` names, as `subject` filters them.
    fn found<'a>(all: &[&'a str], target: &str) -> Vec<&'a str> {
        all.iter().copied().filter(|n| names(n, target)).collect()
    }

    const STEM: &str = "abcdefghijklmnopqrstuvwxyz";

    #[test]
    fn a_cut_sigma_names_its_own_app_and_never_a_look_alike_alone() {
        // Review of L46: lowercased on its own, the cut name's last 'Σ' is
        // a final 'ς', while inside A's whole name it is 'σ'; B, with a
        // literal 'ς' there, matched what A shows and A did not.
        let a = format!("{STEM}\u{3a3}XYZ");
        let b = format!("{STEM}\u{3c2}xyz");
        let all = [a.as_str(), b.as_str()];
        assert_eq!(shown(&a), format!("{STEM}\u{3a3}\u{2026}"));
        let hit = found(&all, &shown(&a));
        assert!(hit.contains(&a.as_str()), "A by its shown name: {hit:?}");
        assert_ne!(hit, [b.as_str()], "never B alone");
        let hit = found(&all, &shown(&b));
        assert!(hit.contains(&b.as_str()), "B by its shown name: {hit:?}");
        // Whole names fold the same way: an upper-case Greek name is
        // found by its lower-case spelling, final sigma and all.
        assert!(names(
            "\u{39f}\u{394}\u{39f}\u{3a3}",
            "\u{3bf}\u{3b4}\u{3bf}\u{3c2}"
        ));
    }

    #[test]
    fn a_name_shown_with_a_replacement_names_its_own_app_and_its_look_alike() {
        // Review of L46: `mirqab apps` shows a control or bidi character as
        // U+FFFD. A, with U+202E in what is shown, missed its own shown
        // name; B, with a literal U+FFFD there, matched it alone.
        let a = format!("{}\u{202e}{}", &STEM[..20], &STEM[..10]);
        let b = format!("{}\u{fffd}{}", &STEM[..20], &STEM[..10]);
        let all = [a.as_str(), b.as_str()];
        assert_eq!(shown(&a), shown(&b));
        assert_eq!(found(&all, &shown(&a)), all, "both, so nothing is guessed");
        assert_eq!(found(&all, &a), [a.as_str()], "A's own name is A alone");
        // A short name is shown whole, through the same replacement.
        let (c, d) = ("foot\u{1b}bar", "foot\u{fffd}bar");
        assert_eq!(found(&[c, d], &shown(c)), [c, d]);
        assert_eq!(found(&[c, d], c), [c]);
    }

    #[test]
    fn a_cut_dotted_capital_i_names_its_own_app() {
        // 'İ' lowercases to two characters: the old prefix was one too
        // long, so A never matched its own shown name.
        let a = format!("{STEM}\u{130}XYZ");
        let b = format!("{STEM}i\u{307}xyz");
        let all = [a.as_str(), b.as_str()];
        assert_eq!(found(&all, &shown(&a)), [a.as_str()]);
        assert_eq!(found(&all, &shown(&b)), [b.as_str()]);
    }

    #[test]
    fn every_app_is_named_by_its_own_name_and_by_what_apps_shows() {
        let all = [
            "Example Browser Developer Edition X",
            "Example Browser Developer Edition Y",
            "Example Browser Developer Ed",
            "Foot",
            "\u{39f}\u{394}\u{39f}\u{3a3} \u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}\u{3a3}",
            "x\u{1b}[2J long enough to be cut by the column",
            "\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}",
        ];
        for a in all {
            for target in [a.to_string(), shown(a), shown(a).to_uppercase()] {
                assert!(found(&all, &target).contains(&a), "{a:?} by {target:?}");
            }
        }
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
        // One target had exited and the other was left alone: nothing was
        // quit, so it is a failure, as plan 006 and the help say (plan 024
        // L6).
        assert_eq!(
            reply(json!([]), json!([d]), json!([]), json!([d])),
            EXIT_FAILED
        );
        assert_eq!(
            reply(json!([]), json!([]), json!([d]), json!([])),
            EXIT_FAILED
        );
    }
}
