//! JSON-RPC 2.0 frames for the MCP server (plan 005): one message per line
//! on stdio. Hand-written over serde_json; an SDK would bring a runtime.

use mirqab_core::proto::strings;
use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// MCP 2026-07-28: the request's protocol version is not served here.
pub const UNSUPPORTED_VERSION: i64 = -32022;

/// One line from the client.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// No id: nothing is answered.
    Notification,
    /// Not a request; answered with this error.
    Bad(Value),
}

pub fn parse(line: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Incoming::Bad(error(
            Value::Null,
            PARSE_ERROR,
            strings::MCP_PARSE_ERROR,
            None,
        ));
    };
    let id = v.get("id").cloned();
    let method = v.get("method").and_then(Value::as_str);
    match (v.get("jsonrpc").and_then(Value::as_str), method, id) {
        (Some("2.0"), Some(m), Some(id)) if id.is_string() || id.is_number() => Incoming::Request {
            id,
            method: m.to_string(),
            params: v.get("params").cloned().unwrap_or(Value::Null),
        },
        (Some("2.0"), Some(_), None) => Incoming::Notification,
        // A response from the client, or anything else: answered only when
        // it carries an id to answer.
        (_, _, id) => Incoming::Bad(error(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            strings::MCP_BAD_REQUEST,
            None,
        )),
    }
}

pub fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({"code": code, "message": message});
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": e})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_notifications_and_garbage() {
        assert!(matches!(
            parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
            Incoming::Request { method, .. } if method == "ping"
        ));
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Incoming::Notification
        );
        let Incoming::Bad(e) = parse("{not json") else {
            panic!("bad json")
        };
        assert_eq!(e["error"]["code"], PARSE_ERROR);
        assert!(e["id"].is_null());
        let Incoming::Bad(e) = parse(r#"{"jsonrpc":"1.0","id":7,"method":"x"}"#) else {
            panic!("wrong version")
        };
        assert_eq!(
            (e["id"].clone(), e["error"]["code"].clone()),
            (json!(7), json!(INVALID_REQUEST))
        );
    }
}
