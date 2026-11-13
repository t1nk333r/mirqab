//! `mirqab mcp` end to end (plan 005, Tests): the legacy `initialize`
//! handshake and the 2026-07-28 per-request `_meta`; `tools/list` matches
//! the tools of the plans merged so far; an unknown tool and malformed JSON
//! are JSON-RPC errors and the server stays up; a missing daemon gives tool
//! errors, not a crash; `system_load` reads the fixture daemon.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_mirqab");

struct Daemon {
    child: Child,
    dir: PathBuf,
    socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn daemon(name: &str) -> Daemon {
    // Under /tmp, not $TMPDIR: a socket path must stay under 108 bytes.
    let dir = PathBuf::from("/tmp").join(format!("mqp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop");
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", &dir)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env_remove("MIRQAB_SOCKET")
        .env_remove("NOTIFY_SOCKET")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    let socket = run.join("mirqab/daemon.sock");
    assert_eq!(ready.trim(), format!("ready {}", socket.display()));
    Daemon { child, dir, socket }
}

/// A running `mirqab mcp` talking to the daemon on `socket`.
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Server {
    fn start(socket: &std::path::Path) -> Server {
        let mut child = Command::new(BIN)
            .arg("--socket")
            .arg(socket)
            .arg("mcp")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Server {
            child,
            input,
            output,
        }
    }

    fn send(&mut self, line: &str) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
    }

    fn ask(&mut self, msg: Value) -> Value {
        self.send(&msg.to_string());
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line}"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn modern(id: u32, method: &str, params: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[test]
fn a_legacy_client_initializes_lists_and_reads_the_load() {
    let d = daemon("legacy");
    let mut s = Server::start(&d.socket);
    let init = s.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "1"}}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "mirqab");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    s.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    let list = s.ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, ["system_load", "explain_slowness"]);
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{t}");
        assert!(
            t["description"].as_str().unwrap().starts_with("Reads only"),
            "the side effect comes first: {t}"
        );
    }

    let load = s.ask(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "system_load", "arguments": {}}}));
    let r = &load["result"];
    assert_eq!(r["isError"], false, "{load}");
    assert_eq!(
        r["structuredContent"]["memory"]["totalBytes"],
        67_108_864_000u64
    );
    assert_eq!(r["structuredContent"]["thermal"]["state"], "cool");
    assert!(
        r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("17 GB used of 67 GB")
    );

    let slow = s.ask(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "explain_slowness"}}));
    assert_eq!(slow["result"]["structuredContent"]["causes"], json!([]));
}

#[test]
fn an_unknown_version_falls_back_to_the_newest_legacy_one() {
    let mut s = Server::start(std::path::Path::new("/nonexistent/mirqab.sock"));
    let init = s.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-01-01", "capabilities": {}}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn a_modern_client_discovers_and_an_unknown_version_is_refused() {
    let d = daemon("modern");
    let mut s = Server::start(&d.socket);
    let disc = s.ask(modern(1, "server/discover", json!({})));
    let r = &disc["result"];
    assert_eq!(r["resultType"], "complete");
    assert_eq!(
        (r["ttlMs"].clone(), r["cacheScope"].clone()),
        (json!(0), json!("public"))
    );
    assert_eq!(r["supportedVersions"][0], "2026-07-28");
    assert_eq!(
        r["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "mirqab"
    );
    let list = s.ask(modern(2, "tools/list", json!({})));
    assert_eq!(list["result"]["resultType"], "complete");
    assert_eq!(list["result"]["ttlMs"], 0);
    assert_eq!(list["result"]["cacheScope"], "public");

    let mut old = modern(3, "tools/list", json!({}));
    old["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1900-01-01");
    let refused = s.ask(old);
    assert_eq!(refused["error"]["code"], -32022, "{refused}");
    assert_eq!(refused["error"]["data"]["requested"], "1900-01-01");
    assert!(
        refused["error"]["data"]["supported"]
            .as_array()
            .unwrap()
            .contains(&json!("2026-07-28"))
    );
}

#[test]
fn bad_input_is_an_error_and_the_server_stays_up() {
    let mut s = Server::start(std::path::Path::new("/nonexistent/mirqab.sock"));
    s.send("{not json");
    let mut line = String::new();
    s.output.read_line(&mut line).unwrap();
    let e: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        (e["error"]["code"].clone(), e["id"].clone()),
        (json!(-32700), Value::Null)
    );
    let unknown = s.ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "kill_everything", "arguments": {}}}));
    assert_eq!(unknown["error"]["code"], -32602);
    let bad_args = s.ask(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "system_load", "arguments": [1]}}));
    assert_eq!(bad_args["error"]["code"], -32602);
    let nope = s.ask(json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}));
    assert_eq!(nope["error"]["code"], -32601);
    let ping = s.ask(json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}));
    assert_eq!(ping["result"]["resultType"], "complete", "still serving");
}

#[test]
fn no_daemon_is_a_tool_error_the_agent_can_show() {
    let mut s = Server::start(std::path::Path::new("/nonexistent/mirqab.sock"));
    let r = s.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "system_load", "arguments": {}}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("systemctl --user start mirqab.socket")
    );
}

#[test]
fn closing_stdin_ends_the_server() {
    let mut s = Server::start(std::path::Path::new("/nonexistent/mirqab.sock"));
    drop(s.input.take());
    let status = s.child.wait().unwrap();
    assert!(status.success());
}
