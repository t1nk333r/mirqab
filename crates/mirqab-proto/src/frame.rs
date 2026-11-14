//! Frames: JSON, one object per line, both directions (architecture §5.2).
//!
//! The frame shapes (a hello with capabilities on connect, then requests
//! with an id, replies and per-stream events) are adapted from haseen's
//! `core/internal/proto/proto.go` (MIT, Copyright (c) 2026 t1nk33r), which
//! adapts DankMaterialShell's `core/internal/server` (MIT, Copyright (c) 2025
//! Avenge Media LLC). NOTICE.md carries both.

use crate::code::Code;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A client's request. `stream` is set for the stream methods; everything
/// else travels in `params`. Unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: &str) -> Self {
        Request {
            id,
            method: method.to_string(),
            stream: None,
            params: Value::Null,
        }
    }

    pub fn with_stream(mut self, stream: &str) -> Self {
        self.stream = Some(stream.to_string());
        self
    }

    pub fn with_params(mut self, params: Value) -> Self {
        self.params = params;
        self
    }

    /// A field of `params`, or None.
    pub fn param(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }
}

/// The body of an error reply. `code` is one of the closed list. `data`
/// carries what the error needs beside its message: a `needs-confirm`
/// reply's plan and `wouldDo` (plan 006). It is omitted when empty, and a
/// new field does not change the protocol (architecture §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: Code,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Everything the daemon sends, told apart by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerFrame {
    Hello(Hello),
    Reply(Reply),
    Event(Event),
}

/// The first line on every connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
    pub pid: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Reply {
    pub fn ok(id: u64, result: Value) -> Self {
        Reply {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, code: Code, message: impl Into<String>) -> Self {
        Self::err_data(id, code, message, None)
    }

    /// An error with `data` beside its message.
    pub fn err_data(id: u64, code: Code, message: impl Into<String>, data: Option<Value>) -> Self {
        Reply {
            id,
            result: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub stream: String,
    pub data: Value,
}

/// One frame as a line, with its trailing newline.
pub fn encode<T: Serialize>(frame: &T) -> Vec<u8> {
    // Serialising these types cannot fail: every map key is a string.
    let mut line = serde_json::to_vec(frame).unwrap_or_default();
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(frame: ServerFrame) {
        let line = encode(&frame);
        assert_eq!(line.last(), Some(&b'\n'));
        let back: ServerFrame = serde_json::from_slice(&line).expect("parses");
        assert_eq!(back, frame);
    }

    #[test]
    fn every_server_frame_round_trips() {
        round_trip(ServerFrame::Hello(Hello {
            protocol: 1,
            version: "0.1.0".into(),
            pid: 123,
            capabilities: vec!["self".into(), "ui".into()],
        }));
        round_trip(ServerFrame::Reply(Reply::ok(
            1,
            json!({"stream": "self", "interval": 2000}),
        )));
        round_trip(ServerFrame::Reply(Reply::err(3, Code::Refused, "no")));
        round_trip(ServerFrame::Event(Event {
            stream: "self".into(),
            data: json!({"uptimeSec": 1}),
        }));
    }

    #[test]
    fn frames_carry_their_type() {
        let line = encode(&ServerFrame::Reply(Reply::err(
            2,
            Code::NeedsConfirm,
            "confirm",
        )));
        let v: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(v["type"], "reply");
        assert_eq!(v["error"]["code"], "needs-confirm");
        assert!(v.get("result").is_none());
        assert!(v["error"].get("data").is_none(), "no data, no field");
        let with = encode(&ServerFrame::Reply(Reply::err_data(
            4,
            Code::NeedsConfirm,
            "confirm",
            Some(json!({"plan": "plan-1", "wouldDo": []})),
        )));
        let v: Value = serde_json::from_slice(&with).unwrap();
        assert_eq!(v["error"]["data"]["plan"], "plan-1");
        let back: ServerFrame = serde_json::from_slice(&with).unwrap();
        assert!(
            matches!(back, ServerFrame::Reply(r) if r.error.as_ref().is_some_and(|e| e.data.is_some()))
        );
    }

    #[test]
    fn requests_round_trip_and_ignore_unknown_fields() {
        let req = Request::new(7, "subscribe")
            .with_stream("system")
            .with_params(json!({"interval": 1000, "live": true}));
        let back: Request = serde_json::from_slice(&encode(&req)).unwrap();
        assert_eq!(back, req);

        let extra: Request =
            serde_json::from_str(r#"{"id":1,"method":"status","future":{"x":1}}"#).unwrap();
        assert_eq!(extra.method, "status");
        assert_eq!(extra.stream, None);
        assert_eq!(extra.params, Value::Null);
    }

    #[test]
    fn unknown_fields_in_server_frames_are_ignored() {
        let f: ServerFrame = serde_json::from_str(
            r#"{"type":"hello","protocol":1,"version":"9","pid":1,"capabilities":[],"later":true}"#,
        )
        .unwrap();
        assert!(matches!(f, ServerFrame::Hello(_)));
    }
}
