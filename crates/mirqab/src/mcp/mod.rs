//! `mirqab mcp` (plan 005): the Model Context Protocol on stdin and stdout,
//! one JSON-RPC message per line, until stdin closes. Logs go to stderr.
//!
//! It serves both eras of the protocol. A client of revision 2025-11-25 or
//! earlier opens with `initialize`; a client of 2026-07-28 sends each
//! request with `_meta["io.modelcontextprotocol/protocolVersion"]` and may
//! ask `server/discover` first. Every result carries `resultType`, which
//! older clients ignore; `server/discover` and `tools/list` also say how
//! long a client may cache them (`ttlMs`, `cacheScope`).

mod rpc;
mod tools;

use mirqab_core::proto::{VERSION, strings};
use rpc::Incoming;
use serde_json::{Value, json};
use std::io::{BufRead, ErrorKind, Write};
use std::path::Path;
use std::process::ExitCode;

/// The revision served with per-request `_meta`.
pub const MODERN: &str = "2026-07-28";
/// The revisions served after `initialize`, newest first.
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// The longest line read; a longer one is skipped with an error, so a
/// client cannot make the server hold an unbounded line (review of PR #13).
const MAX_LINE: usize = 8 << 20;

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
                // Cacheable results (2026-07-28): the tools never change
                // while the server runs, and nothing in them is per user.
                "ttlMs": 0,
                "cacheScope": "public",
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
                "ttlMs": 0,
                "cacheScope": "public",
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

/// One line of input, without its newline.
#[derive(Debug, PartialEq)]
enum Line {
    Text(Vec<u8>),
    /// Longer than the limit; its bytes were read and dropped.
    TooLong,
}

/// Reads the next line, keeping at most `max` bytes of it. `None` at the
/// end of input.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let (mut over, mut any) = (false, false);
    loop {
        let chunk = match r.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if chunk.is_empty() {
            return Ok(match (any, over) {
                (false, _) => None,
                (true, true) => Some(Line::TooLong),
                (true, false) => Some(Line::Text(buf)),
            });
        }
        any = true;
        let newline = chunk.iter().position(|&b| b == b'\n');
        let take = newline.map_or(chunk.len(), |i| i + 1);
        let body = newline.unwrap_or(chunk.len());
        if !over {
            if buf.len() + body > max {
                over = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(&chunk[..body]);
            }
        }
        r.consume(take);
        if newline.is_some() {
            return Ok(Some(if over { Line::TooLong } else { Line::Text(buf) }));
        }
    }
}

/// The reply to one line, if it needs one.
fn reply_to(socket: &Path, line: Line) -> Option<Value> {
    let text = match line {
        Line::TooLong => {
            return Some(rpc::error(
                Value::Null,
                rpc::INVALID_REQUEST,
                strings::MCP_TOO_LONG,
                None,
            ));
        }
        Line::Text(bytes) => match String::from_utf8(bytes) {
            Ok(t) => t,
            // Not UTF-8, so not JSON: a parse error, and the server goes on
            // (review of PR #13: it used to stop reading).
            Err(_) => {
                return Some(rpc::error(
                    Value::Null,
                    rpc::PARSE_ERROR,
                    strings::MCP_PARSE_ERROR,
                    None,
                ));
            }
        },
    };
    if text.trim().is_empty() {
        return None;
    }
    match rpc::parse(&text) {
        Incoming::Request { id, method, params } => Some(handle(socket, id, &method, &params)),
        Incoming::Notification | Incoming::Response => None,
        Incoming::Bad(e) => Some(e),
    }
}

pub fn run(socket: &Path) -> ExitCode {
    let mut input = std::io::stdin().lock();
    let mut out = std::io::stdout();
    // A read error is the client's end failing: nothing left to serve.
    while let Ok(Some(line)) = read_line(&mut input, MAX_LINE) {
        let Some(reply) = reply_to(socket, line) else {
            continue;
        };
        // The client closed its end: nothing left to serve.
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_over_the_limit_and_bad_bytes_do_not_end_the_server() {
        let mut r = std::io::Cursor::new(b"abc\n0123456789\n\xff\xfe\nlast".to_vec());
        assert_eq!(
            read_line(&mut r, 5).unwrap(),
            Some(Line::Text(b"abc".to_vec()))
        );
        assert_eq!(read_line(&mut r, 5).unwrap(), Some(Line::TooLong));
        let bad = read_line(&mut r, 5).unwrap().unwrap();
        assert_eq!(bad, Line::Text(b"\xff\xfe".to_vec()));
        assert_eq!(
            read_line(&mut r, 5).unwrap(),
            Some(Line::Text(b"last".to_vec()))
        );
        assert_eq!(read_line(&mut r, 5).unwrap(), None);
        let none = Path::new("");
        assert_eq!(
            reply_to(none, bad).unwrap()["error"]["code"],
            rpc::PARSE_ERROR
        );
        assert_eq!(
            reply_to(none, Line::TooLong).unwrap()["error"]["code"],
            rpc::INVALID_REQUEST
        );
    }

    #[test]
    fn a_response_from_the_client_is_not_answered() {
        let line = Line::Text(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#.to_vec());
        assert!(reply_to(Path::new(""), line).is_none());
    }
}
