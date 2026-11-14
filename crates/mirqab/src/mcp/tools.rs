//! The MCP tools (plan 005): a table of name, title, description, input
//! schema and handler. The tools of this plan read the daemon; acting tools
//! (plan 007 on) add the confirm flow (`mirqab_core::plan`) and the
//! ancestry guard (`mirqab_core::guard`).

use crate::apps::{apps_lines, what_lines};
use crate::cli::{CallError, call, load_lines, system};
use mirqab_core::proto::stream::{AppsData, Inspect, SystemData};
use mirqab_core::proto::{Request, stream, strings};
use serde_json::{Value, json};
use std::path::Path;

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub read_only: bool,
    /// The JSON Schema of the tool's arguments.
    input: fn() -> Value,
    handler: fn(&Path, &Value) -> Value,
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "system_load",
        title: strings::MCP_SYSTEM_LOAD_TITLE,
        description: strings::MCP_SYSTEM_LOAD,
        read_only: true,
        input: no_arguments,
        handler: system_load,
    },
    Tool {
        name: "explain_slowness",
        title: strings::MCP_SLOWNESS_TITLE,
        description: strings::MCP_SLOWNESS,
        read_only: true,
        input: no_arguments,
        handler: explain_slowness,
    },
    Tool {
        name: "top_apps",
        title: strings::MCP_TOP_APPS_TITLE,
        description: strings::MCP_TOP_APPS,
        read_only: true,
        input: top_apps_input,
        handler: top_apps,
    },
    Tool {
        name: "what_is_process",
        title: strings::MCP_WHAT_TITLE,
        description: strings::MCP_WHAT,
        read_only: true,
        input: what_is_process_input,
        handler: what_is_process,
    },
];

/// No arguments: only an empty object (MCP's recommendation).
fn no_arguments() -> Value {
    json!({"type": "object", "additionalProperties": false})
}

/// The most apps `top_apps` returns, and how many when not asked.
const TOP_APPS_MAX: u64 = 50;
const TOP_APPS_DEFAULT: u64 = 10;

fn top_apps_input() -> Value {
    json!({
        "type": "object",
        "properties": {
            "sort": {"type": "string", "enum": ["cpu", "mem", "name"], "description": strings::MCP_ARG_SORT},
            "limit": {"type": "integer", "minimum": 1, "maximum": TOP_APPS_MAX, "description": strings::MCP_ARG_LIMIT},
        },
        "additionalProperties": false,
    })
}

fn what_is_process_input() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pid": {"type": "integer", "minimum": 1, "description": strings::MCP_ARG_PID},
        },
        "required": ["pid"],
        "additionalProperties": false,
    })
}

/// `tools/list`'s entry for `t`.
pub fn describe(t: &Tool) -> Value {
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        "inputSchema": (t.input)(),
        "annotations": {
            "title": t.title,
            "readOnlyHint": t.read_only,
            "destructiveHint": !t.read_only,
            "idempotentHint": t.read_only,
            "openWorldHint": false,
        },
    })
}

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn call_tool(t: &Tool, socket: &Path, args: &Value) -> Value {
    (t.handler)(socket, args)
}

/// A tool result: the text, and the same data structured. A text copy of
/// the data goes along for clients that read only `content`.
fn done(text: String, data: Value) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "structuredContent": data,
        "isError": false,
    })
}

/// A tool execution error the agent can show the user.
fn failed(text: String) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "isError": true,
    })
}

/// One request to the daemon; a failure is the tool error to return.
fn fetch(socket: &Path, req: Request) -> Result<Value, Value> {
    call(socket, req).map_err(|e| match e {
        CallError::NoDaemon(why) => failed(strings::no_daemon(&socket.display().to_string(), &why)),
        CallError::Daemon(code, message) => failed(strings::daemon_error(&code, &message)),
    })
}

fn snapshot(socket: &Path) -> Result<SystemData, Value> {
    fetch(socket, Request::new(1, "get").with_stream(stream::SYSTEM)).map(|v| system(&v))
}

/// The figures an agent reasons about; nothing else of the stream.
fn summary(d: &SystemData) -> Value {
    json!({
        "cpu": d.cpu.as_ref().map(|c| json!({
            "percent": c.percent, "user": c.user, "system": c.system, "iowait": c.iowait,
            "logical": c.logical,
        })),
        "load": d.load.as_ref().map(|l| json!({"one": l.one, "five": l.five, "fifteen": l.fifteen})),
        "memory": d.mem.as_ref().map(|m| json!({
            "usedBytes": m.used_bytes, "totalBytes": m.total_bytes,
            "availableBytes": m.available_bytes,
            "swapUsedBytes": m.swap_used_bytes, "swapTotalBytes": m.swap_total_bytes,
        })),
        "pressure": d.pressure.as_ref().map(|p| json!({
            "cpu": p.cpu.as_ref().map(|s| s.some60),
            "memory": p.memory.as_ref().map(|s| s.some60),
            "io": p.io.as_ref().map(|s| s.some60),
        })),
        "thermal": d.thermal.as_ref().map(|t| json!({"state": t.state, "throttling": t.throttling})),
    })
}

fn system_load(socket: &Path, _args: &Value) -> Value {
    match snapshot(socket) {
        Ok(d) => done(load_lines(&d).join("\n"), summary(&d)),
        Err(e) => e,
    }
}

fn explain_slowness(socket: &Path, _args: &Value) -> Value {
    match snapshot(socket) {
        Ok(d) => {
            let text = format!(
                "{}\n{}",
                strings::MCP_SLOWNESS_LATER,
                load_lines(&d).join("\n")
            );
            let mut data = summary(&d);
            data["causes"] = json!([]);
            done(text, data)
        }
        Err(e) => e,
    }
}

fn top_apps(socket: &Path, args: &Value) -> Value {
    let sort = match args.get("sort") {
        None => "cpu",
        Some(v) => match v.as_str() {
            Some(s @ ("cpu" | "mem" | "name")) => s,
            _ => return failed(strings::mcp_bad_argument("sort", strings::MCP_ARG_SORT)),
        },
    };
    let limit = match args.get("limit") {
        None => TOP_APPS_DEFAULT,
        Some(v) => match v.as_u64() {
            Some(n @ 1..=TOP_APPS_MAX) => n,
            _ => return failed(strings::mcp_bad_argument("limit", strings::MCP_ARG_LIMIT)),
        },
    };
    let req = Request::new(1, "get")
        .with_stream(stream::APPS)
        .with_params(json!({"sort": sort, "limit": limit}));
    let d: AppsData = match fetch(socket, req) {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(e) => return e,
    };
    // What an agent reasons about: no pid lists, no processes.
    let groups: Vec<Value> = d
        .groups
        .iter()
        .map(|g| {
            json!({
                "id": g.id, "name": g.name, "kind": g.kind, "cpu": g.cpu,
                "rssBytes": g.rss_bytes, "procs": g.procs,
                "top": g.top.as_ref().map(|t| json!({"pid": t.pid, "name": t.name, "cpu": t.cpu})),
            })
        })
        .collect();
    let data = json!({
        "sort": d.sort,
        "groups": groups,
        "totals": {"procs": d.totals.procs, "cpu": d.totals.cpu, "cores": d.totals.cores},
    });
    done(apps_lines(&d, false).join("\n"), data)
}

fn what_is_process(socket: &Path, args: &Value) -> Value {
    let Some(pid) = args
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|p| (1..=u64::from(u32::MAX)).contains(p))
    else {
        return failed(strings::MCP_PID_REQUIRED.to_string());
    };
    let req = Request::new(1, "get")
        .with_stream(stream::APPS_INSPECT)
        .with_params(json!({ "pid": pid }));
    let mut inspect: Inspect = match fetch(socket, req) {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(e) => return e,
    };
    // A command line, a working directory or an open file's path can hold
    // a secret or a private name; the agent's vendor keeps its transcript.
    inspect.cmdline.clear();
    inspect.cwd = None;
    if let Some(f) = &mut inspect.files {
        f.paths.clear();
    }
    let text = what_lines(&inspect, false).join("\n");
    let mut data = serde_json::to_value(&inspect).unwrap_or(Value::Null);
    if let Some(files) = data.get_mut("files").and_then(Value::as_object_mut) {
        files.remove("paths");
    }
    done(text, data)
}
