//! `mirqab mcp` (plan 005): the Model Context Protocol on stdin and stdout,
//! one JSON-RPC message per line, until stdin closes. Logs go to stderr.
//!
//! It serves both eras of the protocol. A client of revision 2025-11-25 or
//! earlier opens with `initialize`; a client of 2026-07-28 sends each
//! request with `_meta["io.modelcontextprotocol/protocolVersion"]` and may
//! ask `server/discover` first. Every result carries `resultType`, which
//! older clients ignore.

mod rpc;
mod tools;

use mirqab_core::proto::{VERSION, strings};
use rpc::Incoming;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

/// The revision served with per-request `_meta`.
pub const MODERN: &str = "2026-07-28";
/// The revisions served after `initialize`, newest first.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN)
        .chain(LEGACY.iter().copied())
        .collect()
}

fn server_info() -> Value {
    json!({"name": strings::APP_NAME, "version": VERSION})
}

/// The answer to one request.
pub fn handle(socket: &Path, id: Value, method: &str, params: &Value) -> Value {
    // A modern request names its version; one this server does not serve
    // is refused with the list it does (MCP 2026-07-28, -32022).
    if let Some(v) = params.pointer("/_meta").and_then(|m| m.get(META_VERSION)) {
        let v = v.as_str().unwrap_or_default();
        if v != MODERN {
            return rpc::error(
                id,
                rpc::UNSUPPORTED_VERSION,
                &strings::mcp_unsupported(v),
                Some(json!({"supported": supported(), "requested": v})),
            );
        }
    }
    match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = LEGACY.iter().find(|v| **v == asked).unwrap_or(&LEGACY[0]);
            rpc::result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": server_info(),
                    "instructions": strings::MCP_INSTRUCTIONS,
                }),
            )
        }
        "server/discover" => rpc::result(
            id,
            json!({
                "resultType": "complete",
                "supportedVersions": supported(),
                "capabilities": {"tools": {}},
                "_meta": {"io.modelcontextprotocol/serverInfo": server_info()},
                "instructions": strings::MCP_INSTRUCTIONS,
            }),
        ),
        "ping" => rpc::result(id, json!({"resultType": "complete"})),
        "tools/list" => rpc::result(
            id,
            json!({
                "resultType": "complete",
                "tools": tools::TOOLS.iter().map(tools::describe).collect::<Vec<_>>(),
            }),
        ),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let Some(tool) = tools::find(name) else {
                return rpc::error(
                    id,
                    rpc::INVALID_PARAMS,
                    &strings::mcp_unknown_tool(name),
                    None,
                );
            };
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !args.is_object() {
                return rpc::error(id, rpc::INVALID_PARAMS, strings::MCP_ARGS_NOT_OBJECT, None);
            }
            rpc::result(id, tools::call_tool(tool, socket, &args))
        }
        other => rpc::error(
            id,
            rpc::METHOD_NOT_FOUND,
            &strings::mcp_unknown_method(other),
            None,
        ),
    }
}

pub fn run(socket: &Path) -> ExitCode {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match rpc::parse(&line) {
            Incoming::Request { id, method, params } => handle(socket, id, &method, &params),
            Incoming::Notification => continue,
            Incoming::Bad(e) => e,
        };
        // The client closed its end: nothing left to serve.
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}
