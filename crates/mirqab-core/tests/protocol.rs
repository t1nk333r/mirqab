//! The socket protocol against a real server on a fixture root (plan 001,
//! "Tests"): hello first; subscribe, events, unsubscribe; merged intervals;
//! the 2 s floor and the live floor; unknown and unavailable streams; the
//! interface's `ui` stream; config.set; and a flooding client is dropped.

use mirqab_core::proto::{Request, ServerFrame, frame};
use mirqab_core::root::Root;
use mirqab_core::sampler::Sampler;
use mirqab_core::server::{self, Options};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A stream that cannot go live.
struct Fake;
impl Sampler for Fake {
    fn stream(&self) -> &'static str {
        "fake"
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn sample(&mut self, _root: &Root, _now: Instant) -> Value {
        json!({"n": 1})
    }
}

/// A stream this machine cannot serve.
struct Missing;
impl Sampler for Missing {
    fn stream(&self) -> &'static str {
        "missing"
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Err("no such hardware".into())
    }
    fn sample(&mut self, _root: &Root, _now: Instant) -> Value {
        Value::Null
    }
}

struct Daemon {
    socket: PathBuf,
    dir: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(name: &str) -> Daemon {
    // Under /tmp, not $TMPDIR: a socket path must stay under 108 bytes.
    let dir = PathBuf::from("/tmp").join(format!("mqp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers: vec![Box::new(Fake), Box::new(Missing)],
    })
    .expect("listens");
    std::thread::spawn(move || server.serve());
    Daemon { socket, dir }
}

struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}

impl Client {
    fn connect(d: &Daemon) -> Client {
        let stream = UnixStream::connect(&d.socket).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        Client {
            stream,
            reader,
            next: 1,
        }
    }

    fn frame(&mut self) -> Option<ServerFrame> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).expect("a frame")),
        }
    }

    fn hello(&mut self) -> Vec<String> {
        match self.frame() {
            Some(ServerFrame::Hello(h)) => h.capabilities,
            other => panic!("expected hello, got {other:?}"),
        }
    }

    fn send(&mut self, method: &str, stream: Option<&str>, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        let mut req = Request::new(id, method).with_params(params);
        if let Some(s) = stream {
            req = req.with_stream(s);
        }
        self.stream.write_all(&frame::encode(&req)).unwrap();
        id
    }

    /// The reply to `id`, skipping events. Ok(result) or Err(code).
    fn call(&mut self, method: &str, stream: Option<&str>, params: Value) -> Result<Value, String> {
        let id = self.send(method, stream, params);
        loop {
            match self.frame() {
                Some(ServerFrame::Reply(r)) if r.id == id => {
                    return match r.error {
                        Some(e) => Err(e.code.as_str().to_string()),
                        None => Ok(r.result.unwrap_or(Value::Null)),
                    };
                }
                Some(_) => continue,
                None => panic!("connection closed waiting for {method}"),
            }
        }
    }

    /// The next event on `stream`, within the read timeout.
    fn event(&mut self, stream: &str) -> Option<Value> {
        loop {
            match self.frame()? {
                ServerFrame::Event(e) if e.stream == stream => return Some(e.data),
                _ => continue,
            }
        }
    }

    fn interval(&mut self, stream: &str) -> Value {
        self.call("status", None, Value::Null).unwrap()["streams"][stream]["intervalMs"].clone()
    }
}

#[test]
fn hello_comes_first_with_the_capabilities() {
    let d = start("hello");
    let mut c = Client::connect(&d);
    let caps = c.hello();
    assert_eq!(caps, ["fake", "self", "ui"]);
    assert_eq!(
        c.call("capabilities", None, Value::Null).unwrap(),
        json!(["fake", "self", "ui"])
    );
}

#[test]
fn subscribe_brings_events_and_unsubscribe_stops_them() {
    let d = start("sub");
    let mut c = Client::connect(&d);
    c.hello();
    let t = Instant::now();
    let r = c.call("subscribe", Some("self"), json!({})).unwrap();
    assert_eq!(r, json!({"stream": "self", "interval": 2000}));
    let data = c.event("self").expect("a first event at once");
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "the first event does not wait an interval"
    );
    // The first sample on root-desktop is the golden file, uptime aside.
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/self.json")).unwrap();
    let mut got = data.clone();
    got["uptimeSec"] = json!(0);
    assert_eq!(got, golden);

    c.call("unsubscribe", Some("self"), Value::Null).unwrap();
    c.stream
        .set_read_timeout(Some(Duration::from_millis(2600)))
        .unwrap();
    assert!(c.event("self").is_none(), "no event after unsubscribe");
}

#[test]
fn subscribers_share_the_shortest_interval_and_floors_apply() {
    let d = start("merge");
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    assert_eq!(
        a.call("subscribe", Some("fake"), json!({"interval": 5000}))
            .unwrap()["interval"],
        5000
    );
    assert_eq!(
        b.call("subscribe", Some("fake"), json!({"interval": 3000}))
            .unwrap()["interval"],
        3000
    );
    assert_eq!(a.interval("fake"), 3000);
    drop(b);
    // The daemon notices the closed connection on its reader thread.
    let deadline = Instant::now() + Duration::from_secs(2);
    while a.interval("fake") != 5000 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        a.interval("fake"),
        5000,
        "the interval returns when the faster subscriber leaves"
    );

    assert_eq!(
        a.call("subscribe", Some("fake"), json!({"interval": 500}))
            .unwrap()["interval"],
        2000,
        "below the floor is raised"
    );
}

#[test]
fn live_goes_to_one_second_only_on_a_live_stream() {
    let d = start("live");
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    assert_eq!(
        a.call(
            "subscribe",
            Some("self"),
            json!({"interval": 1000, "live": true})
        )
        .unwrap()["interval"],
        1000
    );
    assert_eq!(
        a.call(
            "subscribe",
            Some("fake"),
            json!({"interval": 1000, "live": true})
        )
        .unwrap()["interval"],
        2000
    );
    assert_eq!(
        b.call("subscribe", Some("self"), json!({})).unwrap()["interval"],
        2000
    );
    assert_eq!(b.interval("self"), 1000);
    let status = b.call("status", None, Value::Null).unwrap();
    assert_eq!(status["streams"]["self"]["live"], true);
    // Dropping the live flag by subscribing again without it.
    a.call("subscribe", Some("self"), json!({})).unwrap();
    assert_eq!(b.interval("self"), 2000);
}

#[test]
fn unknown_and_unavailable_streams_are_errors() {
    let d = start("unknown");
    let mut c = Client::connect(&d);
    let caps = c.hello();
    assert!(!caps.contains(&"missing".to_string()));
    assert_eq!(
        c.call("subscribe", Some("nope"), Value::Null),
        Err("unknown-stream".into())
    );
    assert_eq!(
        c.call("subscribe", Some("missing"), Value::Null),
        Err("unavailable".into())
    );
    assert_eq!(
        c.call("frobnicate", None, Value::Null),
        Err("unknown-method".into())
    );
    let status = c.call("status", None, Value::Null).unwrap();
    assert_eq!(status["unavailable"]["missing"], "no such hardware");
    assert_eq!(
        c.call("get", Some("fake"), Value::Null).unwrap(),
        json!({"n": 1})
    );
}

#[test]
fn bad_json_is_answered_not_fatal() {
    let d = start("badjson");
    let mut c = Client::connect(&d);
    c.hello();
    c.stream.write_all(b"{not json\n").unwrap();
    match c.frame() {
        Some(ServerFrame::Reply(r)) => assert_eq!(r.error.unwrap().code.as_str(), "bad-request"),
        other => panic!("expected an error reply, got {other:?}"),
    }
    assert!(c.call("status", None, Value::Null).is_ok());
}

#[test]
fn the_interface_hears_actions_and_config_changes() {
    let d = start("ui");
    let mut ui = Client::connect(&d);
    let mut cli = Client::connect(&d);
    ui.hello();
    cli.hello();
    assert_eq!(
        cli.call("action", None, json!({"name": "ui.show", "args": {}})),
        Err("unavailable".into())
    );
    ui.call("subscribe", Some("ui"), json!({"role": "ui", "pid": 4321}))
        .unwrap();
    assert_eq!(
        cli.call("status", None, Value::Null).unwrap()["ui"]["pid"],
        4321
    );

    let r = cli
        .call(
            "action",
            None,
            json!({"name": "ui.show", "args": {"tab": "system", "mode": "window"}, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(
        r,
        json!({"wouldDo": [{"event": "show", "tab": "system", "mode": "window"}]})
    );
    cli.call(
        "action",
        None,
        json!({"name": "ui.show", "args": {"tab": "system"}}),
    )
    .unwrap();
    assert_eq!(
        ui.event("ui").unwrap(),
        json!({"event": "show", "tab": "system", "mode": null})
    );
    cli.call("action", None, json!({"name": "ui.hide"}))
        .unwrap();
    assert_eq!(ui.event("ui").unwrap(), json!({"event": "hide"}));

    assert_eq!(
        cli.call(
            "config.set",
            None,
            json!({"key": "live.intervalMs", "value": 10})
        ),
        Err("bad-request".into())
    );
    cli.call(
        "config.set",
        None,
        json!({"key": "live.intervalMs", "value": 2000}),
    )
    .unwrap();
    assert_eq!(ui.event("ui").unwrap(), json!({"event": "config-changed"}));
    assert_eq!(
        cli.call("config.get", None, json!({"keys": ["live.intervalMs"]}))
            .unwrap(),
        json!({"live.intervalMs": 2000})
    );
    // The live floor follows the setting.
    assert_eq!(
        cli.call(
            "subscribe",
            Some("self"),
            json!({"interval": 1000, "live": true})
        )
        .unwrap()["interval"],
        2000
    );
}

#[test]
fn a_client_that_never_reads_is_dropped() {
    let d = start("flood");
    let mut c = Client::connect(&d);
    c.hello();
    // Thousands of replies the client never reads: the kernel buffer fills,
    // then the client's queue of 256 frames, and the daemon cuts it off.
    let sent = 20_000;
    for i in 0..sent {
        let req = Request::new(i + 1, "status");
        match c.stream.write_all(&frame::encode(&req)) {
            Ok(()) => {}
            Err(e) if matches!(e.kind(), ErrorKind::BrokenPipe | ErrorKind::ConnectionReset) => {
                break;
            }
            Err(e) => panic!("unexpected write error: {e}"),
        }
    }
    let mut replies = 0u64;
    loop {
        let mut line = String::new();
        match c.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => replies += 1,
            Err(e) if e.kind() == ErrorKind::ConnectionReset => break,
            Err(e) => panic!("the daemon kept the flooding client: {e} after {replies} replies"),
        }
    }
    assert!(
        replies < sent,
        "dropped before every reply was written ({replies} of {sent})"
    );
    // The daemon itself is fine.
    let mut other = Client::connect(&d);
    other.hello();
    assert!(other.call("status", None, Value::Null).is_ok());
}

#[test]
fn a_client_that_half_closes_still_gets_its_reply() {
    let d = start("halfclose");
    for i in 0..50 {
        let mut c = Client::connect(&d);
        c.hello();
        let id = c.send("status", None, Value::Null);
        c.stream.shutdown(std::net::Shutdown::Write).unwrap();
        match c.frame() {
            Some(ServerFrame::Reply(r)) => assert_eq!(r.id, id),
            other => panic!("try {i}: expected the reply, got {other:?}"),
        }
    }
}

#[test]
fn a_dry_run_flag_that_is_not_a_boolean_is_refused() {
    let d = start("dryrun");
    let mut c = Client::connect(&d);
    c.hello();
    for flag in [json!("true"), json!(1)] {
        assert_eq!(
            c.call(
                "config.set",
                None,
                json!({"key": "units.clock", "value": "24", "dryRun": flag})
            ),
            Err("bad-request".into())
        );
        assert_eq!(
            c.call("action", None, json!({"name": "ui.hide", "dryRun": flag})),
            Err("bad-request".into())
        );
    }
    assert!(
        !d.dir.join("config/mirqab/config.json").exists(),
        "nothing was written"
    );
    assert_eq!(
        c.call("config.get", None, json!({"keys": ["units.clock"]}))
            .unwrap(),
        json!({"units.clock": "auto"})
    );
}
