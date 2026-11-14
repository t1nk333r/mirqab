//! The MCP tools (plan 005): a table of name, title, description, input
//! schema and handler. The tools of this plan read the daemon; acting tools
//! (plan 007 on) add the confirm flow (`mirqab_core::plan`) and the
//! ancestry guard (`mirqab_core::guard`).

use crate::cli::{CallError, call, load_lines, system};
use mirqab_core::proto::stream::SystemData;
use mirqab_core::proto::{Request, stream, strings};
use serde_json::{Value, json};
use std::path::Path;

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub read_only: bool,
    handler: fn(&Path, &Value) -> Value,
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "system_load",
        title: strings::MCP_SYSTEM_LOAD_TITLE,
        description: strings::MCP_SYSTEM_LOAD,
        read_only: true,
        handler: system_load,
    },
    Tool {
        name: "explain_slowness",
        title: strings::MCP_SLOWNESS_TITLE,
        description: strings::MCP_SLOWNESS,
        read_only: true,
        handler: explain_slowness,
    },
];

/// `tools/list`'s entry for `t`.
pub fn describe(t: &Tool) -> Value {
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        // No arguments: only an empty object (MCP's recommendation).
        "inputSchema": {"type": "object", "additionalProperties": false},
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

fn snapshot(socket: &Path) -> Result<SystemData, Value> {
    match call(socket, Request::new(1, "get").with_stream(stream::SYSTEM)) {
        Ok(v) => Ok(system(&v)),
        Err(CallError::NoDaemon(why)) => Err(failed(strings::no_daemon(
            &socket.display().to_string(),
            &why,
        ))),
        Err(CallError::Daemon(code, message)) => {
            Err(failed(strings::daemon_error(&code, &message)))
        }
    }
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
