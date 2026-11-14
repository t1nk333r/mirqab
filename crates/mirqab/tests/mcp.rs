//! `mirqab mcp` end to end (plan 005, Tests): the legacy `initialize`
//! handshake and the 2026-07-28 per-request `_meta`; `tools/list` matches
//! the tools of the plans merged so far; an unknown tool and malformed JSON
//! are JSON-RPC errors and the server stays up; a missing daemon gives tool
//! errors, not a crash; `system_load` reads the fixture daemon; `top_apps`
//! and `what_is_process` (plan 006) read it too, and send no command line
//! or path to the agent.

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
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let fixture = fixtures.join("root-desktop");
    // The process inspector asks pacman (plan 006): the fixture's stub
    // answers, never the machine's package database.
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(fixtures.join("bin/pacman"), bin.join("pacman")).unwrap();
    std::fs::set_permissions(bin.join("pacman"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(BIN)
        .args(["daemon", "--root"])
        .arg(&fixture)
        .env("PATH", path)
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

    fn send_bytes(&mut self, bytes: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(bytes).unwrap();
        input.flush().unwrap();
    }

    fn next(&mut self) -> Value {
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line}"))
    }

    /// Sends a request and reads the reply, which must be the reply to it:
    /// a stray line from an earlier message fails here.
    fn ask(&mut self, msg: Value) -> Value {
        self.send(&msg.to_string());
        let reply = self.next();
        assert_eq!(reply["id"], msg["id"], "the reply to {msg}: {reply}");
        reply
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
    assert_eq!(init["result"]["resultType"], "complete");
    s.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    let list = s.ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "system_load",
            "explain_slowness",
            "top_apps",
            "what_is_process"
        ]
    );
    let what = &list["result"]["tools"][3]["inputSchema"];
    assert_eq!(what["required"], json!(["pid"]), "{what}");
    assert_eq!(what["additionalProperties"], false);
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

fn text_of(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn top_apps_and_what_is_process_read_the_fixture() {
    let d = daemon("apps");
    let mut s = Server::start(&d.socket);
    let mut call = |id: u32, name: &str, args: Value| {
        s.ask(json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": args}}))["result"]
            .clone()
    };
    let top = call(1, "top_apps", json!({"sort": "mem", "limit": 3}));
    assert_eq!(top["isError"], false, "{top}");
    let groups = top["structuredContent"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 4, "three, and System last");
    assert_eq!(groups[0]["name"], "cargo");
    assert_eq!(groups[3]["id"], "system");
    assert!(groups[0].get("pids").is_none(), "no pid lists for an agent");
    // An allow-list: a field added to the stream later fails here first.
    let keys = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(
        keys(&groups[0]),
        ["cpu", "id", "kind", "name", "procs", "rssBytes", "top"]
    );
    assert_eq!(top["structuredContent"]["totals"]["groups"], 23);
    assert_eq!(top["structuredContent"]["sort"], "mem");
    let text = top["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Example Browser"), "{text}");

    let what = call(2, "what_is_process", json!({"pid": 2001}));
    assert_eq!(what["isError"], false, "{what}");
    let data = &what["structuredContent"];
    assert_eq!(data["verdict"]["code"], "packaged");
    assert_eq!(
        data["package"],
        json!({"name": "example-browser", "repo": "extra"})
    );
    assert_eq!(data["files"]["count"], 9);
    // Exactly these fields reach an agent (review of PR #16): no cgroup,
    // command line, working directory or path, and nothing added later
    // without being named in the tool.
    let mut want = vec![
        "pid",
        "name",
        "verdict",
        "limited",
        "ppid",
        "parents",
        "uid",
        "user",
        "startedAt",
        "ageSec",
        "cpuTimeSec",
        "threads",
        "state",
        "exe",
        "exeDeleted",
        "package",
        "arch",
        "unit",
        "flatpak",
        "container",
        "group",
        "sandbox",
        "memory",
        "files",
        "gpu",
    ];
    want.sort_unstable();
    assert_eq!(keys(data), want);
    assert_eq!(keys(&data["files"]), ["count", "sockets"]);
    assert!(!text_of(&what).contains("cgroup") && !text_of(&what).contains("user.slice"));
    // Command lines and paths can hold secrets: never sent to an agent.
    for absent in ["cmdline", "cwd"] {
        assert!(data.get(absent).is_none(), "{absent}: {data}");
    }
    assert!(data["files"].get("paths").is_none(), "{data}");
    let text = what["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Packaged by extra/example-browser"),
        "{text}"
    );
    assert!(
        !text.contains("--new-window") && !text.contains(".cache"),
        "{text}"
    );

    for (id, name, args) in [
        (3, "what_is_process", json!({})),
        (4, "what_is_process", json!({"pid": "2001"})),
        (5, "top_apps", json!({"limit": 0})),
        (6, "top_apps", json!({"sort": "size"})),
    ] {
        let r = call(id, name, args.clone());
        assert_eq!(r["isError"], true, "{name} {args}: {r}");
    }
    let gone = call(7, "what_is_process", json!({"pid": 99999}));
    assert_eq!(gone["isError"], true);
    assert!(
        gone["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("no process 99999")
    );
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

    let mut number = modern(4, "ping", json!({}));
    number["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(20260728);
    let by_number = s.ask(number);
    assert_eq!(by_number["error"]["code"], -32022, "{by_number}");
    assert_eq!(by_number["error"]["data"]["requested"], 20260728);

    let mut bare = modern(5, "tools/list", json!({}));
    bare["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove("io.modelcontextprotocol/clientCapabilities");
    let no_caps = s.ask(bare);
    assert_eq!(
        no_caps["error"]["code"], -32602,
        "no capabilities: {no_caps}"
    );
    let mut wrong = modern(6, "tools/list", json!({}));
    wrong["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!([]);
    let not_object = s.ask(wrong);
    assert_eq!(not_object["error"]["code"], -32602, "{not_object}");
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
    let e = s.next();
    assert_eq!(
        (e["error"]["code"].clone(), e["id"].clone()),
        (json!(-32700), Value::Null)
    );
    // Bytes that are not UTF-8 are a parse error too, and the server goes
    // on reading (review of PR #13: it used to stop).
    s.send_bytes(b"\xff\xfe\n");
    let e = s.next();
    assert_eq!(
        (e["error"]["code"].clone(), e["id"].clone()),
        (json!(-32700), Value::Null)
    );
    // A response from the client is not answered: the next line back is
    // the ping's.
    s.send(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#);
    s.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}));
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
