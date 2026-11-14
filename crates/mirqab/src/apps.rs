//! `mirqab apps` and `mirqab what` (plan 006): the `apps` stream and the
//! process inspector, read once. The MCP tools `top_apps` and
//! `what_is_process` print the same lines.

use crate::cli::{call, exit_for, socket_or_exit};
use crate::{Args, EXIT_USAGE};
use mirqab_core::proto::stream::{APPS, APPS_INSPECT, AppsData, Inspect};
use mirqab_core::proto::{Request, strings};
use mirqab_view::format;
use serde_json::{Value, json};
use std::process::ExitCode;

fn usage(msg: &str) -> ExitCode {
    crate::out::err(msg);
    ExitCode::from(EXIT_USAGE)
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| strings::NO_VALUE.to_string(), format::percent)
}

/// `mirqab apps`' lines: a header, a row per group, and with `all` its
/// processes under it.
pub(crate) fn apps_lines(d: &AppsData, all: bool) -> Vec<String> {
    let mut out = vec![strings::APPS_HEADER.to_string()];
    for g in &d.groups {
        out.push(strings::apps_row(
            &g.name,
            &pct(g.cpu),
            &format::bytes(g.rss_bytes),
            g.procs,
        ));
        if all {
            for p in g.processes.iter().flatten() {
                out.push(strings::apps_process_row(
                    p.pid,
                    &p.name,
                    &pct(p.cpu),
                    &format::bytes(p.rss_bytes),
                ));
            }
        }
    }
    out
}

/// `mirqab what`'s lines: the verdict, then one labelled line per field
/// the daemon could read. `private` adds the command line and the working
/// directory, which the MCP tool leaves out.
pub(crate) fn what_lines(i: &Inspect, private: bool) -> Vec<String> {
    let mut out = vec![i.verdict.text.clone()];
    let mut line = |label: &str, value: String| out.push(strings::what_line(label, &value));
    line(strings::WHAT_PROCESS, strings::proc_ref(&i.name, i.pid));
    if !i.parents.is_empty() {
        let chain: Vec<String> = i
            .parents
            .iter()
            .map(|p| strings::proc_ref(&p.name, p.pid))
            .collect();
        line(strings::WHAT_PARENTS, chain.join(", "));
    }
    match (&i.user, i.uid) {
        (Some(u), Some(uid)) => line(strings::WHAT_USER, strings::proc_ref(u, uid)),
        (None, Some(uid)) => line(strings::WHAT_USER, uid.to_string()),
        _ => {}
    }
    if let Some(age) = i.age_sec {
        line(
            strings::WHAT_STARTED,
            strings::started_ago(&format::duration(age)),
        );
    }
    if let Some(t) = i.cpu_time_sec {
        line(strings::WHAT_CPU_TIME, format::duration(t as u64));
    }
    if let Some(t) = i.threads {
        line(strings::WHAT_THREADS, t.to_string());
    }
    if let Some(s) = &i.state {
        line(strings::WHAT_STATE, strings::state_word(s).to_string());
    }
    if let Some(e) = &i.exe {
        let shown = if i.exe_deleted {
            strings::exe_replaced(e)
        } else {
            e.clone()
        };
        line(strings::WHAT_EXE, shown);
    }
    if let Some(p) = &i.package {
        line(
            strings::WHAT_PACKAGE,
            strings::package_line(&p.name, p.repo.as_deref()),
        );
    }
    if let Some(a) = &i.arch {
        line(
            strings::WHAT_ARCH,
            strings::arch_line(&a.machine, a.bits, a.foreign),
        );
    }
    if let Some(u) = &i.unit {
        line(strings::WHAT_UNIT, u.clone());
    }
    if let Some(g) = &i.group {
        line(strings::WHAT_GROUP, g.name.clone());
    }
    if let Some(f) = &i.flatpak {
        line(strings::WHAT_FLATPAK, f.clone());
    }
    if let Some(c) = &i.container {
        line(strings::WHAT_CONTAINER, c.clone());
    }
    if let Some(s) = &i.sandbox {
        line(
            strings::WHAT_SANDBOX,
            strings::sandbox_line(
                s.seccomp.as_deref(),
                s.no_new_privs,
                &s.capabilities,
                &s.namespaces,
            ),
        );
    }
    if let Some(m) = &i.memory {
        let pss = m.pss_bytes.map(format::bytes);
        let swap = m.swap_bytes.map(format::bytes);
        line(
            strings::WHAT_MEMORY,
            strings::memory_line(&format::bytes(m.rss_bytes), pss.as_deref(), swap.as_deref()),
        );
    }
    if let Some(f) = &i.files {
        line(strings::WHAT_FILES, strings::files_line(f.count, f.sockets));
    }
    if let Some(g) = &i.gpu {
        let (v, t, c) = (
            g.vram_bytes.map(format::bytes),
            g.gtt_bytes.map(format::bytes),
            g.cpu_bytes.map(format::bytes),
        );
        line(
            strings::WHAT_GPU,
            strings::gpu_memory_line(v.as_deref(), t.as_deref(), c.as_deref()),
        );
    }
    if private {
        if let Some(d) = &i.cwd {
            line(strings::WHAT_CWD, d.clone());
        }
        if !i.cmdline.is_empty() {
            line(strings::WHAT_COMMAND, i.cmdline.join(" "));
        }
    }
    out
}

pub fn run(command: &str, args: Args) -> ExitCode {
    let socket = match socket_or_exit(&args) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let (mut json_out, mut all, mut sort, mut pid) = (false, false, "cpu".to_string(), None);
    let mut it = args.rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json_out = true,
            "--all" if command == "apps" => all = true,
            "--sort" if command == "apps" => match it.next().map(String::as_str) {
                Some(s @ ("cpu" | "mem" | "name")) => sort = s.to_string(),
                Some(other) => return usage(&strings::apps_bad_sort(other)),
                None => return usage(&strings::missing_value("--sort")),
            },
            s if command == "what" && pid.is_none() && !s.starts_with('-') => {
                match s.parse::<u32>() {
                    Ok(n) if n > 0 => pid = Some(n),
                    _ => return usage(&strings::bad_pid(s)),
                }
            }
            other => return usage(&strings::unknown_option(other)),
        }
    }
    let req = if command == "apps" {
        let mut params = json!({"sort": sort});
        if all {
            params["expand"] = Value::Bool(true);
        }
        Request::new(1, "get").with_stream(APPS).with_params(params)
    } else {
        let Some(pid) = pid else {
            return usage(&strings::command_help("what").unwrap_or_default());
        };
        Request::new(1, "get")
            .with_stream(APPS_INSPECT)
            .with_params(json!({ "pid": pid }))
    };
    match call(&socket, req) {
        Ok(v) if json_out => {
            crate::out::line(&v.to_string());
            ExitCode::SUCCESS
        }
        Ok(v) => {
            let lines = if command == "apps" {
                apps_lines(&serde_json::from_value(v).unwrap_or_default(), all)
            } else {
                what_lines(&serde_json::from_value(v).unwrap_or_default(), true)
            };
            for l in lines {
                crate::out::line(&l);
            }
            ExitCode::SUCCESS
        }
        Err(e) => exit_for(&socket, e),
    }
}
