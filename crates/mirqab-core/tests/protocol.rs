//! The socket protocol against a real server on a fixture root (plan 001,
//! "Tests"): hello first; subscribe, events, unsubscribe; merged intervals;
//! the 2 s floor and the live floor; unknown and unavailable streams; the
//! interface's `ui` stream; config.set; and a flooding client is dropped.
//! Plan 006 (D6): a re-subscribe replaces a view and leaves no old one
//! behind, an unsubscribe removes it, two subscribers that differ only in
//! interval share one line; and the `process` stream. Part 4: the minute
//! writer writes a sampler's events; a `get apps` and an `apps.inspect`
//! whose `pacman` takes 2 s no longer hold a subscriber's frame back
//! (D1); `status.focus` is null without a focus source. Plan 007: the
//! `ports` stream takes only its own params, served at 5 s. Audit fixes: a
//! subscriber that joins a narrowed stream mid-sample waits for a sample
//! read for it; `history.query` is clamped and its series capped; the
//! history file, its directory and the locks are private; the minute
//! writer replaces a file found corrupt while open.

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

/// A stream with a view that shows which sample and which params made
/// each line, and how often the stream was idle:
/// `{"n": <sample number>, "idle": <idle calls>, "params": <the params>}`.
#[derive(Default)]
struct Counted {
    n: u64,
    idle: u64,
}
impl Sampler for Counted {
    fn stream(&self) -> &'static str {
        "counted"
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn sample(&mut self, _root: &Root, _now: Instant) -> Value {
        self.n += 1;
        json!({"n": self.n, "idle": self.idle})
    }
    fn idle(&mut self) {
        self.idle += 1;
    }
    fn status(&self) -> Vec<(&'static str, Value)> {
        vec![("counted_idle", json!(self.idle))]
    }
    fn view(&self) -> Option<mirqab_core::sampler::View> {
        Some(|data, params| json!({"n": data["n"], "idle": data["idle"], "params": params}))
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
        samplers: vec![Box::new(Fake), Box::<Counted>::default(), Box::new(Missing)],
        history: None,
        minute_period: None,
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
    let all = [
        "apps", "counted", "fake", "ports", "process", "self", "system", "ui",
    ];
    assert_eq!(caps, all);
    assert_eq!(
        c.call("capabilities", None, Value::Null).unwrap(),
        json!(all)
    );
}

#[test]
fn each_subscriber_gets_its_own_view_of_one_sample() {
    // Plan 006: `apps` is measured once and shaped per subscriber by its
    // params; `get` takes params too, and `apps.inspect` is a query.
    let d = start("views");
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    a.call("subscribe", Some("apps"), json!({"limit": 1}))
        .unwrap();
    b.call(
        "subscribe",
        Some("apps"),
        json!({"limit": 2, "sort": "name", "expand": ["system"]}),
    )
    .unwrap();
    let one = a.event("apps").unwrap();
    let two = b.event("apps").unwrap();
    // The System group comes last, outside the limit.
    assert_eq!(one["groups"].as_array().unwrap().len(), 2);
    assert_eq!(one["groups"][1]["id"], "system");
    assert!(one["groups"][1].get("processes").is_none());
    assert_eq!(two["sort"], "name");
    assert_eq!(two["groups"].as_array().unwrap().len(), 3);
    assert_eq!(two["groups"][0]["name"], "Calculator");
    assert_eq!(two["groups"][2]["processes"].as_array().unwrap().len(), 6);

    let got = a
        .call("get", Some("apps"), json!({"sort": "mem", "limit": 3}))
        .unwrap();
    assert_eq!(got["sort"], "mem");
    assert_eq!(got["groups"].as_array().unwrap().len(), 4);
    let other = a
        .call("get", Some("apps.inspect"), json!({"pid": 410}))
        .unwrap();
    assert_eq!(other["verdict"]["code"], "other-user");
    assert_eq!(other["limited"], true);
    assert_eq!(
        a.call("get", Some("apps.inspect"), json!({})),
        Err("bad-request".into())
    );
    assert_eq!(
        a.call("get", Some("apps.inspect"), json!({"pid": 99999})),
        Err("failed".into())
    );
    assert_eq!(
        a.call("get", Some("apps.nope"), Value::Null),
        Err("unknown-stream".into())
    );
    assert_eq!(
        a.call("get", Some("system.inspect"), Value::Null),
        Err("unknown-stream".into())
    );
}

#[test]
fn subscribe_brings_events_and_unsubscribe_stops_them() {
    let d = start("sub");
    let mut c = Client::connect(&d);
    c.hello();
    let t = Instant::now();
    // A long interval, so "the first event does not wait for it" has a wide
    // margin on a busy CI runner (a 1 s bound against 2 s failed there).
    let r = c
        .call("subscribe", Some("self"), json!({"interval": 5000}))
        .unwrap();
    assert_eq!(r, json!({"stream": "self", "interval": 5000}));
    // The reply came first and the first event follows it: `call` skipped
    // nothing, so a lost first event shows as a five-second wait.
    let data = c.event("self").expect("a first event at once");
    assert!(
        t.elapsed() < Duration::from_secs(4),
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
        .set_read_timeout(Some(Duration::from_millis(5600)))
        .unwrap();
    assert!(c.event("self").is_none(), "no event after unsubscribe");
}

#[test]
fn the_system_stream_matches_its_golden_file() {
    let d = start("system");
    let mut c = Client::connect(&d);
    c.hello();
    c.call("subscribe", Some("system"), json!({})).unwrap();
    // The first sample has no previous reading, so no CPU percent yet.
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/system.json")).unwrap();
    let mut got = c.event("system").unwrap();
    // A slow start can make the probe's baseline over 1 s old, and the
    // first event then reads against it: on a fixture nothing moved, so
    // the rates are 0 instead of null.
    for (group, key) in [
        ("mem", "pageInPerSec"),
        ("mem", "pageOutPerSec"),
        ("mem", "swapInPerSec"),
        ("mem", "swapOutPerSec"),
        ("power", "packageWatts"),
    ] {
        if got[group][key] == json!(0.0) {
            got[group][key] = Value::Null;
        }
    }
    assert_eq!(got, golden);
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
fn a_bad_request_with_a_usable_id_is_answered_with_that_id() {
    // Audit 2026-11-21, L16: the reply carried id 0, so the caller waited
    // for its timeout.
    let d = start("badid");
    let mut c = Client::connect(&d);
    c.hello();
    c.stream.write_all(b"{\"id\":7,\"method\":5}\n").unwrap();
    match c.frame() {
        Some(ServerFrame::Reply(r)) => {
            assert_eq!(r.id, 7);
            assert_eq!(r.error.unwrap().code.as_str(), "bad-request");
        }
        other => panic!("expected an error reply, got {other:?}"),
    }
    // An id that is not a whole number is not guessed at.
    c.stream
        .write_all(b"{\"id\":\"x\",\"method\":5}\n")
        .unwrap();
    match c.frame() {
        Some(ServerFrame::Reply(r)) => assert_eq!(r.id, 0),
        other => panic!("expected an error reply, got {other:?}"),
    }
}

#[test]
fn a_request_split_over_two_writes_is_one_request() {
    // Audit 2026-11-21, L17: read_lines joins the halves.
    let d = start("split");
    let mut c = Client::connect(&d);
    c.hello();
    c.stream.write_all(b"{\"id\":3,\"meth").unwrap();
    c.stream.flush().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    c.stream.write_all(b"od\":\"status\"}\r\n").unwrap();
    match c.frame() {
        Some(ServerFrame::Reply(r)) => {
            assert_eq!(r.id, 3);
            assert!(r.error.is_none(), "{:?}", r.error);
        }
        other => panic!("expected the status reply, got {other:?}"),
    }
}

#[test]
fn a_line_over_a_mebibyte_closes_only_that_connection() {
    // Audit 2026-11-21, L17: MAX_LINE without a newline ends the reader;
    // another client is still served.
    let d = start("longline");
    let mut long = Client::connect(&d);
    let mut other = Client::connect(&d);
    long.hello();
    other.hello();
    let chunk = vec![b'x'; 64 * 1024];
    let mut closed = false;
    for _ in 0..64 {
        if long.stream.write_all(&chunk).is_err() {
            closed = true;
            break;
        }
    }
    // Past 1 MiB the daemon stops reading and shuts the socket: a read
    // sees the end (or a reset), not a timeout and never a reply.
    if !closed {
        let mut line = String::new();
        match long.reader.read_line(&mut line) {
            Ok(0) => {}
            Err(e) if e.kind() == ErrorKind::ConnectionReset => {}
            other => panic!("the connection was still open: {other:?} {line}"),
        }
    }
    assert!(other.call("status", None, Value::Null).is_ok());
}

#[test]
fn the_interface_hears_actions_and_config_changes() {
    let d = start("ui");
    let mut ui = Client::connect(&d);
    let mut cli = Client::connect(&d);
    ui.hello();
    cli.hello();
    // Hiding with no interface is done: nothing is shown. (Showing starts
    // the unit through systemctl; crates/mirqab/tests/cli.rs covers that
    // with a stub, which an in-process server cannot put on PATH.)
    assert_eq!(
        cli.call("action", None, json!({"name": "ui.hide"})),
        Ok(json!({"done": [{"event": "hide"}]}))
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
fn an_interface_pid_out_of_range_is_not_taken() {
    // Audit 2026-11-21, L37: 4294967297 was cut to 1, and 0 was taken.
    let d = start("uipid");
    let mut ui = Client::connect(&d);
    let mut cli = Client::connect(&d);
    ui.hello();
    cli.hello();
    for pid in [json!(4_294_967_297u64), json!(0), json!(2_147_483_648u64)] {
        ui.call("subscribe", Some("ui"), json!({"role": "ui", "pid": pid}))
            .unwrap();
        assert_eq!(
            cli.call("status", None, Value::Null).unwrap()["ui"],
            Value::Null,
            "pid {pid}"
        );
    }
    ui.call("subscribe", Some("ui"), json!({"role": "ui", "pid": 4321}))
        .unwrap();
    assert_eq!(
        cli.call("status", None, Value::Null).unwrap()["ui"]["pid"],
        4321
    );
}

#[test]
fn an_earlier_interface_is_restored_when_the_latest_leaves() {
    let d = start("uirestore");
    let mut first = Client::connect(&d);
    let mut cli = Client::connect(&d);
    first.hello();
    cli.hello();
    first
        .call("subscribe", Some("ui"), json!({"role": "ui", "pid": 1111}))
        .unwrap();
    {
        let mut second = Client::connect(&d);
        second.hello();
        second
            .call("subscribe", Some("ui"), json!({"role": "ui", "pid": 2222}))
            .unwrap();
        assert_eq!(
            cli.call("status", None, Value::Null).unwrap()["ui"]["pid"],
            2222
        );
    }
    // The second interface's connection is closed; the daemon notices on
    // its next read.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let pid = cli.call("status", None, Value::Null).unwrap()["ui"]["pid"].clone();
        if pid == 1111 {
            break;
        }
        assert!(Instant::now() < deadline, "still {pid}");
        std::thread::sleep(Duration::from_millis(20));
    }
    cli.call("action", None, json!({"name": "ui.hide"}))
        .unwrap();
    assert_eq!(first.event("ui").unwrap(), json!({"event": "hide"}));
}

#[test]
fn a_new_live_interval_applies_to_open_live_subscriptions() {
    let d = start("refloor");
    let mut c = Client::connect(&d);
    let mut cli = Client::connect(&d);
    c.hello();
    cli.hello();
    c.call(
        "subscribe",
        Some("self"),
        json!({"interval": 1000, "live": true}),
    )
    .unwrap();
    assert_eq!(cli.interval("self"), 1000);
    cli.call(
        "config.set",
        None,
        json!({"key": "live.intervalMs", "value": 3000}),
    )
    .unwrap();
    assert_eq!(cli.interval("self"), 3000);
    cli.call(
        "config.set",
        None,
        json!({"key": "live.intervalMs", "value": 1000}),
    )
    .unwrap();
    assert_eq!(cli.interval("self"), 1000, "back to what was asked");
}

#[test]
fn re_subscribing_cannot_beat_the_floor() {
    let d = start("resub");
    let mut c = Client::connect(&d);
    c.hello();
    let t = Instant::now();
    let mut events = 0;
    // Re-subscribing asks for a sample at once each time; at most one a
    // second may be taken (review of phase B: 21 events a second).
    while t.elapsed() < Duration::from_millis(2500) {
        c.send("subscribe", Some("fake"), json!({"interval": 2000}));
        std::thread::sleep(Duration::from_millis(50));
    }
    c.stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    while let Some(f) = c.frame() {
        if matches!(f, ServerFrame::Event(_)) {
            events += 1;
        }
    }
    assert!((1..=4).contains(&events), "{events} events in 2.5 s");
}

#[test]
fn re_subscribing_to_a_stream_that_is_not_live_keeps_its_two_second_floor() {
    // Audit 2026-11-21, L14: each subscribe asked for a sample at once,
    // held back only by a 1 s gap, so re-subscribing every second sampled
    // a stream that is not live every second.
    let d = start("resubfloor");
    let mut c = Client::connect(&d);
    c.hello();
    let t = Instant::now();
    for at in [0u64, 600, 1200, 1800] {
        if let Some(wait) = Duration::from_millis(at).checked_sub(t.elapsed()) {
            std::thread::sleep(wait);
        }
        c.send("subscribe", Some("fake"), json!({}));
    }
    std::thread::sleep(Duration::from_millis(2300).saturating_sub(t.elapsed()));
    c.stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut events = 0;
    while let Some(f) = c.frame() {
        if matches!(f, ServerFrame::Event(_)) {
            events += 1;
        }
    }
    assert!((1..=2).contains(&events), "{events} samples in 2.5 s");
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

#[test]
fn quit_takes_strict_arguments_and_fails_closed_without_its_caller() {
    let d = start("quit");
    let mut c = Client::connect(&d);
    c.hello();
    for bad in [
        json!({"name": "apps.quit"}),
        json!({"name": "apps.quit", "args": {"pid": "2001"}}),
        json!({"name": "apps.quit", "args": {"pid": 2001, "group": "system"}}),
        json!({"name": "apps.kill", "args": {"pid": 2001}, "confirm": "true"}),
        json!({"name": "apps.quit", "args": {"pid": 2001}, "confirm": false, "plan": "plan-x"}),
        json!({"name": "apps.quit", "args": {"pid": 2001}, "dryRun": 1}),
    ] {
        assert_eq!(
            c.call("action", None, bad.clone()),
            Err("bad-request".into()),
            "{bad}"
        );
    }
    // The test's own pid has no proc entry in the fixture, so the caller's
    // ancestry cannot be walked and the action is refused, dry run or not
    // (plan 006, decision 7). A pid that happens to be a fixture's is
    // skipped: its walk would succeed.
    let own = fixture("root-desktop").join(format!("proc/{}", std::process::id()));
    if !own.exists() {
        for dry in [false, true] {
            assert_eq!(
                c.call(
                    "action",
                    None,
                    json!({"name": "apps.quit", "args": {"pid": 2001}, "dryRun": dry})
                ),
                Err("refused".into())
            );
        }
    }
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        let kind = e.file_type().unwrap();
        if kind.is_symlink() {
            // The fixtures' links name paths of an imagined machine:
            // copied as links, never followed.
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
        } else if kind.is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[test]
fn an_act_with_no_writable_history_runs_and_says_it_was_not_recorded() {
    let dir = PathBuf::from("/tmp").join(format!("mqp-unrec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tree = dir.join("root");
    copy_tree(&fixture("root-desktop"), &tree);
    // This test is the caller: it sits under the fixture's python3 (3060),
    // so the strict walk of its ancestry succeeds on the copy.
    let me = std::process::id();
    std::fs::create_dir_all(tree.join(format!("proc/{me}"))).unwrap();
    std::fs::write(
        tree.join(format!("proc/{me}/stat")),
        format!("{me} (protocol) S 3060 {me} 3050 0 -1 0 0 0 0 0 1 1 0 0 20 0 1 0 99999 0 0\n"),
    )
    .unwrap();
    // A directory where the history file should be: it cannot be opened.
    let history = dir.join("history.db");
    std::fs::create_dir_all(&history).unwrap();
    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(&tree),
        config_path: dir.join("config.json"),
        samplers: Vec::new(),
        history: Some(history),
        minute_period: None,
    })
    .expect("listens with history off");
    std::thread::spawn(move || server.serve());
    let d = Daemon { socket, dir };
    let mut c = Client::connect(&d);
    c.hello();
    let status = c.call("status", None, Value::Null).unwrap();
    assert_eq!(status["history"]["enabled"], false, "{status}");

    let quit = json!({"name": "apps.quit", "args": {"pid": 7100}});
    let listed = c
        .call(
            "action",
            None,
            json!({"name": "apps.quit", "args": {"pid": 7100}, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(listed["wouldDo"][0]["pid"], 7100);
    let id = c.send("action", None, quit.clone());
    let plan = loop {
        if let Some(ServerFrame::Reply(r)) = c.frame()
            && r.id == id
        {
            let e = r.error.expect("needs-confirm");
            assert_eq!(e.code.as_str(), "needs-confirm");
            break e.data.unwrap()["plan"].as_str().unwrap().to_string();
        }
    };
    let mut redeem = quit;
    redeem["confirm"] = json!(true);
    redeem["plan"] = json!(plan);
    let done = c.call("action", None, redeem).unwrap();
    assert_eq!(done["done"][0]["pid"], 7100, "the act ran: {done}");
    assert_eq!(done["recorded"], false);
}

#[test]
fn the_subscribe_reply_comes_before_the_first_event() {
    let d = start("order");
    for i in 0..100 {
        let mut c = Client::connect(&d);
        c.hello();
        let id = c.send("subscribe", Some("self"), json!({"interval": 5000}));
        match c.frame() {
            Some(ServerFrame::Reply(r)) => assert_eq!(r.id, id, "try {i}"),
            other => panic!("try {i}: an event or nothing before the reply: {other:?}"),
        }
    }
}

/// A live stream whose `snapshot` holds the sampler for 300 ms, and which
/// logs, in the order it was given the sampler, each sample's stamp and the
/// end of each snapshot.
struct Clocked(std::sync::Arc<std::sync::Mutex<Vec<(&'static str, Instant)>>>);
impl Sampler for Clocked {
    fn stream(&self) -> &'static str {
        "clocked"
    }
    fn live(&self) -> bool {
        true
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn sample(&mut self, _root: &Root, now: Instant) -> Value {
        self.0.lock().unwrap().push(("sample", now));
        json!({})
    }
    fn snapshot(&mut self, _root: &Root, _now: Instant) -> Value {
        std::thread::sleep(Duration::from_millis(300));
        self.0
            .lock()
            .unwrap()
            .push(("snapshot-end", Instant::now()));
        json!({})
    }
}

#[test]
fn a_sample_is_stamped_after_it_got_the_sampler() {
    // A tick that waited behind a `get` must not compute its rates over a
    // stamp taken before the wait (review of PR #16: up to 25 % high).
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let dir = PathBuf::from("/tmp").join(format!("mqp-clock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers: vec![Box::new(Clocked(std::sync::Arc::clone(&log)))],
        history: None,
        minute_period: None,
    })
    .expect("listens");
    std::thread::spawn(move || server.serve());
    let d = Daemon { socket, dir };
    let mut sub = Client::connect(&d);
    let mut getter = Client::connect(&d);
    sub.hello();
    getter.hello();
    sub.call(
        "subscribe",
        Some("clocked"),
        json!({"interval": 1000, "live": true}),
    )
    .unwrap();
    // Back-to-back gets keep the sampler busy, so ticks wait for it.
    let until = Instant::now() + Duration::from_millis(3500);
    while Instant::now() < until {
        getter.call("get", Some("clocked"), Value::Null).unwrap();
    }
    let log = log.lock().unwrap().clone();
    assert!(
        log.iter().filter(|(k, _)| *k == "sample").count() >= 3,
        "{log:?}"
    );
    let mut last_end = None;
    for (kind, at) in log {
        match kind {
            "snapshot-end" => last_end = Some(at),
            _ => {
                if let Some(end) = last_end {
                    assert!(at >= end, "a sample stamped before the get it waited for");
                }
            }
        }
    }
}

/// A stream that reads only what its subscribers name (`narrows`): each
/// sample holds the params `wants` handed it, and the first sample waits
/// until the test opens the gate.
struct Gated {
    wanted: Vec<Value>,
    n: u64,
    started: std::sync::mpsc::SyncSender<()>,
    gate: std::sync::mpsc::Receiver<()>,
}
impl Sampler for Gated {
    fn stream(&self) -> &'static str {
        "gated"
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn wants(&mut self, params: &[Value]) {
        self.wanted = params.to_vec();
    }
    fn narrows(&self) -> bool {
        true
    }
    fn sample(&mut self, _root: &Root, _now: Instant) -> Value {
        self.n += 1;
        if self.n == 1 {
            let _ = self.started.send(());
            let _ = self.gate.recv_timeout(Duration::from_secs(5));
        }
        json!({"read": self.wanted})
    }
    fn view(&self) -> Option<mirqab_core::sampler::View> {
        Some(|data, params| json!({"for": params, "read": data["read"]}))
    }
}

#[test]
fn a_subscriber_that_joins_mid_sample_waits_for_a_sample_read_for_it() {
    // Audit: a subscriber whose params arrived while a narrowed sample was
    // being taken got a view cut from a read for other params (`process`
    // said a live pid was gone).
    let (started_tx, started) = std::sync::mpsc::sync_channel(1);
    let (open, gate) = std::sync::mpsc::sync_channel(1);
    let gated = Gated {
        wanted: Vec::new(),
        n: 0,
        started: started_tx,
        gate,
    };
    let d = start_with("gated", vec![Box::new(gated)], false, None);
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    a.call("subscribe", Some("gated"), json!({"k": "a"}))
        .unwrap();
    started
        .recv_timeout(Duration::from_secs(5))
        .expect("the first sample started");
    b.call("subscribe", Some("gated"), json!({"k": "b"}))
        .unwrap();
    open.send(()).unwrap();
    let first_a = a.event("gated").unwrap();
    assert_eq!(first_a["read"], json!([{"k": "a"}]));
    let first_b = b.event("gated").expect("a sample taken for b");
    assert_eq!(first_b["for"], json!({"k": "b"}));
    assert!(
        first_b["read"]
            .as_array()
            .unwrap()
            .contains(&json!({"k": "b"})),
        "b's first view came from a read for a: {first_b}"
    );
}

/// Subscribers of `stream` in `status`.
fn subscribers(c: &mut Client, stream: &str) -> Value {
    c.call("status", None, Value::Null).unwrap()["streams"][stream]["subscribers"].clone()
}

#[test]
fn a_re_subscribe_replaces_the_view_and_leaves_no_old_one() {
    // D6 of the review of PR #16.
    let d = start("resubview");
    let mut c = Client::connect(&d);
    c.hello();
    c.call("subscribe", Some("counted"), json!({"x": 1}))
        .unwrap();
    assert_eq!(c.event("counted").unwrap()["params"], json!({"x": 1}));
    c.call("subscribe", Some("counted"), json!({"x": 2}))
        .unwrap();
    // `call` skipped what came before its reply; everything after it is
    // the new view, as soon as the 2 s floor allows (a changed
    // subscription samples now, audit 2026-11-21, L14) and on.
    let t = Instant::now();
    for _ in 0..2 {
        assert_eq!(c.event("counted").unwrap()["params"], json!({"x": 2}));
    }
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(subscribers(&mut c, "counted"), 1, "replaced, not added");
    c.call("unsubscribe", Some("counted"), Value::Null).unwrap();
    assert_eq!(subscribers(&mut c, "counted"), 0);
    c.stream
        .set_read_timeout(Some(Duration::from_millis(2600)))
        .unwrap();
    assert!(c.event("counted").is_none(), "no view after unsubscribe");
}

#[test]
fn subscribers_that_differ_only_in_interval_share_one_line() {
    let d = start("sharedline");
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    a.call(
        "subscribe",
        Some("counted"),
        json!({"interval": 2000, "x": 1}),
    )
    .unwrap();
    let first = a.event("counted").unwrap();
    b.call(
        "subscribe",
        Some("counted"),
        json!({"interval": 3000, "live": true, "x": 1}),
    )
    .unwrap();
    // b's subscription samples at once; a hears that sample too (a slow
    // runner may give a one of its own scheduled samples first).
    let from_b = b.event("counted").unwrap();
    let mut from_a = a.event("counted").unwrap();
    while from_a["n"].as_u64() < from_b["n"].as_u64() {
        from_a = a.event("counted").unwrap();
    }
    assert_eq!(from_a, from_b, "one sample, one line");
    assert_eq!(
        from_b["params"],
        json!({"x": 1}),
        "interval and live are the server's"
    );
    assert!(from_b["n"].as_u64() > first["n"].as_u64());
}

#[test]
fn the_process_stream_serves_one_pid_live_and_checks_its_params() {
    let d = start("process");
    let mut a = Client::connect(&d);
    let mut b = Client::connect(&d);
    a.hello();
    b.hello();
    for bad in [
        json!({}),
        json!({"pid": "2001"}),
        json!({"pid": 2001, "sort": "cpu"}),
    ] {
        assert_eq!(
            a.call("subscribe", Some("process"), bad.clone()),
            Err("bad-request".into()),
            "{bad}"
        );
        assert_eq!(
            a.call("get", Some("process"), bad.clone()),
            Err("bad-request".into())
        );
    }
    assert_eq!(
        subscribers(&mut a, "process"),
        0,
        "a refused subscribe adds none"
    );
    // 1000 ms only with live.
    assert_eq!(
        a.call(
            "subscribe",
            Some("process"),
            json!({"pid": 2001, "interval": 1000})
        )
        .unwrap()["interval"],
        2000
    );
    assert_eq!(
        a.call(
            "subscribe",
            Some("process"),
            json!({"pid": 2001, "interval": 1000, "live": true})
        )
        .unwrap()["interval"],
        1000
    );
    b.call("subscribe", Some("process"), json!({"pid": 99999}))
        .unwrap();
    let mine = a.event("process").unwrap();
    assert_eq!(mine["pid"], 2001);
    assert_eq!(mine["name"], "browser");
    assert_eq!(mine["start"], 5010);
    assert_eq!(
        b.event("process").unwrap(),
        json!({"pid": 99999, "gone": true})
    );
    let status = a.call("status", None, Value::Null).unwrap();
    assert_eq!(status["streams"]["process"]["subscribers"], 2);
    assert_eq!(status["streams"]["process"]["intervalMs"], 1000);
    assert_eq!(status["streams"]["process"]["live"], true);
    // A get reads its own pid, whoever subscribes.
    let got = b
        .call("get", Some("process"), json!({"pid": 410, "start": 300}))
        .unwrap();
    assert_eq!(got["name"], "systemd-journal");
    assert_eq!(got["cpu"], 0.0, "a second reading of its own");
    // The subscribers still get their own pids after it, with a rate.
    let next = a.event("process").unwrap();
    assert_eq!(next["pid"], 2001);
    assert!(next["cpu"].is_number(), "{next}");
}

#[test]
fn ports_params_are_strict() {
    let d = start("ports");
    let mut c = Client::connect(&d);
    c.hello();
    for bad in [
        json!({"udp": "yes"}),
        json!({"sort": "x"}),
        json!({"lite": 1}),
    ] {
        assert_eq!(
            c.call("subscribe", Some("ports"), bad.clone()),
            Err("bad-request".into()),
            "{bad}"
        );
        assert_eq!(
            c.call("get", Some("ports"), bad.clone()),
            Err("bad-request".into()),
            "{bad}"
        );
    }
    assert_eq!(
        subscribers(&mut c, "ports"),
        0,
        "a refused subscribe adds none"
    );
    let reply = c
        .call("subscribe", Some("ports"), json!({"udp": true}))
        .unwrap();
    assert_eq!(reply["interval"], 5000, "every 5 s unless asked");
    let frame = c.event("ports").unwrap();
    assert_eq!(frame["total"], 13);
    let status = c.call("status", None, Value::Null).unwrap();
    assert_eq!(status["ports"]["rebuilds"], 1, "{status}");
}

#[test]
fn a_stream_left_without_subscribers_is_told_before_its_next_sample() {
    let d = start("idle");
    let mut c = Client::connect(&d);
    c.hello();
    c.call("subscribe", Some("counted"), json!({})).unwrap();
    assert_eq!(c.event("counted").unwrap()["idle"], 0);
    c.call("unsubscribe", Some("counted"), Value::Null).unwrap();
    // The loop wakes on the unsubscribe, finds no subscriber and tells
    // the sampler then, before anyone subscribes again: a focus source
    // must not outlive the page (review of PR #20). Seen through status.
    let until = Instant::now() + Duration::from_secs(1);
    loop {
        let n = c.call("status", None, Value::Null).unwrap()["counted_idle"].clone();
        if n == 1 {
            break;
        }
        assert!(Instant::now() < until, "idle not called yet: {n}");
        std::thread::sleep(Duration::from_millis(20));
    }
    c.call("subscribe", Some("counted"), json!({})).unwrap();
    assert_eq!(c.event("counted").unwrap()["idle"], 1, "one call");
    assert_eq!(c.event("counted").unwrap()["idle"], 1, "and only one");
}

#[test]
fn a_get_with_no_subscriber_leaves_nothing_kept() {
    // Audit 2026-11-21, L20: a `get` left what the sampler kept for it
    // (the procfs cache, the owner map) until a later subscribe came and
    // went. With a subscriber, the sampler keeps what it serves.
    let d = start("getidle");
    let mut c = Client::connect(&d);
    c.hello();
    let idle =
        |c: &mut Client| c.call("status", None, Value::Null).unwrap()["counted_idle"].clone();
    c.call("get", Some("counted"), json!({})).unwrap();
    assert_eq!(idle(&mut c), 1, "told once, after the get");
    let mut sub = Client::connect(&d);
    sub.hello();
    sub.call("subscribe", Some("counted"), json!({})).unwrap();
    sub.event("counted").unwrap();
    c.call("get", Some("counted"), json!({})).unwrap();
    assert_eq!(idle(&mut c), 1, "not told while a subscriber is served");
}

/// A stream whose minute writes one event naming the config it was given.
struct Minuter;
impl Sampler for Minuter {
    fn stream(&self) -> &'static str {
        "minuter"
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn sample(&mut self, _root: &Root, _now: Instant) -> Value {
        Value::Null
    }
    fn minute_events(&mut self, _root: &Root, config: &Value) -> Vec<(&'static str, Value)> {
        vec![("top-apps", json!({"topApps": config["trends"]["topApps"]}))]
    }
}

/// A daemon on root-desktop with these samplers, a history file when
/// `history`, and the minute writer's period.
fn start_with(
    name: &str,
    samplers: Vec<Box<dyn Sampler>>,
    history: bool,
    minute: Option<Duration>,
) -> Daemon {
    let dir = PathBuf::from("/tmp").join(format!("mqp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers,
        history: history.then(|| dir.join("history.db")),
        minute_period: minute,
    })
    .expect("listens");
    std::thread::spawn(move || server.serve());
    Daemon { socket, dir }
}

#[test]
fn the_minute_writer_writes_a_samplers_events() {
    let d = start_with(
        "events",
        vec![Box::new(Minuter)],
        true,
        Some(Duration::from_secs(1)),
    );
    let mut c = Client::connect(&d);
    c.hello();
    let until = Instant::now() + Duration::from_secs(5);
    let rows = loop {
        let rows = c
            .call("history.events", None, json!({"kind": "top-apps"}))
            .unwrap();
        if rows.as_array().is_some_and(|r| !r.is_empty()) || Instant::now() > until {
            break rows;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let first = &rows[0];
    assert_eq!(first["kind"], "top-apps", "{rows}");
    assert_eq!(
        first["data"],
        json!({"topApps": true}),
        "the effective config"
    );
    // The fixture's `apps` writes none: nothing on it reaches 10 %.
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["data"] == json!({"topApps": true})),
        "{rows}"
    );
    // Turned off, the sampler is told so.
    c.call(
        "config.set",
        None,
        json!({"key": "trends.topApps", "value": false}),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = c
            .call("history.events", None, json!({"kind": "top-apps"}))
            .unwrap();
        if rows
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["data"] == json!({"topApps": false}))
        {
            break;
        }
        assert!(Instant::now() < until, "{rows}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The gaps between `n` consecutive `stream` events, with `busy` called
/// between reads from another client.
#[test]
fn a_history_query_is_clamped_and_its_series_capped() {
    // Plan 001, deferred to plan 005.
    let d = start_with("histclamp", Vec::new(), true, None);
    let mut c = Client::connect(&d);
    c.hello();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let all = c
        .call(
            "history.query",
            None,
            json!({"series": ["cpu.total", "cpu.total", "mem.used"], "from": i64::MIN, "to": i64::MAX}),
        )
        .unwrap();
    assert_eq!(all["bucketSec"], 7200);
    assert!(
        all["from"].as_i64().unwrap() >= now - 30 * 86_400 - 5,
        "{all}"
    );
    assert!(all["to"].as_i64().unwrap() <= now + 65, "{all}");
    assert_eq!(all["series"].as_object().unwrap().len(), 2, "{all}");
    let since_1970 = c
        .call(
            "history.query",
            None,
            json!({"series": ["cpu.total"], "from": 0, "to": now}),
        )
        .unwrap();
    assert!(since_1970["bucketSec"].as_i64().unwrap() > 60);
    let many: Vec<String> = (0..65).map(|n| format!("s.{n}")).collect();
    assert_eq!(
        c.call(
            "history.query",
            None,
            json!({"series": many, "range": "1h"})
        ),
        Err("bad-request".into())
    );
    // Repeats count once (50,000 of them stay under the 1 MiB line).
    let repeated = vec!["cpu.total"; 50_000];
    assert!(
        c.call(
            "history.query",
            None,
            json!({"series": repeated, "range": "1h"})
        )
        .is_ok()
    );
    assert_eq!(
        c.call(
            "history.query",
            None,
            json!({"series": ["cpu.total"], "from": 0, "to": 1000})
        ),
        Err("bad-request".into())
    );
    assert!(
        c.call(
            "history.events",
            None,
            json!({"from": i64::MIN, "to": i64::MAX})
        )
        .is_ok()
    );
}

#[test]
fn the_history_file_its_directory_and_the_locks_are_private() {
    // Plan 001, deferred to the first plan that writes events.
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = PathBuf::from("/tmp").join(format!("mqp-modes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("run/daemon.sock");
    let history = dir.join("state/mirqab/history.db");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers: Vec::new(),
        history: Some(history.clone()),
        minute_period: None,
    })
    .expect("listens");
    let _d = Daemon {
        socket: socket.clone(),
        dir: dir.clone(),
    };
    assert_eq!(mode(&dir.join("state/mirqab")), 0o700);
    assert_eq!(mode(&history), 0o600);
    assert_eq!(mode(&dir.join("state/mirqab/history.db.lock")), 0o600);
    assert_eq!(mode(&dir.join("run/daemon.sock.lock")), 0o600);
    drop(server);
}

/// A history file at `path` whose `sample` root page is overwritten: it
/// still opens, and the first use of `sample` finds it corrupt.
fn damaged_history(path: &std::path::Path) {
    use mirqab_core::history::History;
    use std::io::{Seek, SeekFrom};
    {
        let mut h = History::open(path, 1_790_006_400).unwrap();
        h.write_minute(1_790_006_400, &[("cpu.total", 1)]).unwrap();
    }
    let c = rusqlite::Connection::open(path).unwrap();
    let root: i64 = c
        .query_row(
            "SELECT rootpage FROM sqlite_master WHERE name = 'sample'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let page: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
    drop(c);
    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(((root - 1) * page) as u64)).unwrap();
    f.write_all(&vec![0xa5; page as usize]).unwrap();
}

#[test]
fn a_query_that_meets_damage_replaces_the_file_at_once() {
    // Review of PR #26: a query found the damage, answered `failed`, and
    // nothing replaced the file.
    let dir = PathBuf::from("/tmp").join(format!("mqp-qdamaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let history = dir.join("history.db");
    damaged_history(&history);
    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers: Vec::new(),
        history: Some(history.clone()),
        // The minute writer stays asleep: only the query can find it.
        minute_period: Some(Duration::from_secs(86_400)),
    })
    .expect("listens");
    std::thread::spawn(move || server.serve());
    let d = Daemon { socket, dir };
    let mut c = Client::connect(&d);
    c.hello();
    let ask = json!({"series": ["cpu.total"], "range": "1h"});
    assert_eq!(
        c.call("history.query", None, ask.clone()),
        Err("failed".into())
    );
    let status = c.call("status", None, Value::Null).unwrap();
    assert!(
        status["history"]["movedAside"]
            .as_str()
            .is_some_and(|p| p.contains("history.db.broken-")),
        "{status}"
    );
    assert!(c.call("history.query", None, ask).is_ok());
    // A span wholly in the future says so.
    let soon = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;
    let id = c.send(
        "history.query",
        None,
        json!({"series": ["cpu.total"], "from": soon, "to": soon + 60}),
    );
    let error = loop {
        match c.frame() {
            Some(ServerFrame::Reply(r)) if r.id == id => break r.error.expect("an error"),
            Some(_) => continue,
            None => panic!("connection closed"),
        }
    };
    assert_eq!(error.code.as_str(), "bad-request");
    assert_eq!(
        error.message,
        mirqab_core::proto::strings::SPAN_IN_FUTURE,
        "not the message for a span past the kept window"
    );
}

#[test]
fn the_minute_writer_replaces_a_file_found_corrupt_while_open() {
    // Audit: damage past what `open` reads left history "enabled" while
    // every write and query failed.
    use mirqab_core::history::History;
    use std::io::{Seek, SeekFrom};
    let dir = PathBuf::from("/tmp").join(format!("mqp-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let history = dir.join("history.db");
    {
        let mut h = History::open(&history, 1_790_006_400).unwrap();
        h.write_minute(1_790_006_400, &[("cpu.total", 1)]).unwrap();
    }
    // The root page of `sample`, overwritten: the file still opens.
    let c = rusqlite::Connection::open(&history).unwrap();
    let root: i64 = c
        .query_row(
            "SELECT rootpage FROM sqlite_master WHERE name = 'sample'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let page: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
    drop(c);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&history)
        .unwrap();
    f.seek(SeekFrom::Start(((root - 1) * page) as u64)).unwrap();
    f.write_all(&vec![0xa5; page as usize]).unwrap();
    drop(f);

    let socket = dir.join("run/daemon.sock");
    let server = server::listen(Options {
        socket: socket.clone(),
        root: Root::new(fixture("root-desktop")),
        config_path: dir.join("config/mirqab/config.json"),
        samplers: Vec::new(),
        history: Some(history.clone()),
        minute_period: Some(Duration::from_secs(1)),
    })
    .expect("listens");
    std::thread::spawn(move || server.serve());
    let d = Daemon { socket, dir };
    let mut c = Client::connect(&d);
    c.hello();
    let until = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let s = c.call("status", None, Value::Null).unwrap();
        if !s["history"]["movedAside"].is_null() || Instant::now() > until {
            break s;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let aside = status["history"]["movedAside"]
        .as_str()
        .expect("moved aside");
    assert!(aside.contains("history.db.broken-"), "{status}");
    assert!(std::path::Path::new(aside).exists());
    assert_eq!(status["history"]["enabled"], true, "{status}");
    assert_eq!(status["history"]["error"], Value::Null, "{status}");
    assert!(
        c.call(
            "history.query",
            None,
            json!({"series": ["cpu.total"], "range": "1h"})
        )
        .is_ok()
    );
}

fn gaps(sub: &mut Client, stream: &str, n: usize) -> Vec<Duration> {
    let mut last = None;
    let mut out = Vec::new();
    while out.len() < n {
        sub.event(stream).expect("an event");
        let now = Instant::now();
        if let Some(l) = last {
            out.push(now - l);
        }
        last = Some(now);
    }
    out
}

#[test]
fn a_get_of_apps_no_longer_holds_a_subscribers_frame_back() {
    let d = start("lateframe");
    let mut sub = Client::connect(&d);
    let mut getter = Client::connect(&d);
    sub.hello();
    getter.hello();
    sub.call("subscribe", Some("apps"), json!({"interval": 2000}))
        .unwrap();
    sub.event("apps").unwrap();
    // Gets back to back. Each waits half a second between its two
    // readings when the subscriber's baseline is under a second old, and
    // since D1 it waits without the sampler's lock. (At a 2 s interval the
    // old wait under the lock could not reach the next tick either: it
    // ended within 1.5 s of the last one. This guards the new path.)
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = std::sync::Arc::clone(&stop);
    let t = std::thread::spawn(move || {
        let mut n = 0;
        while !s2.load(std::sync::atomic::Ordering::SeqCst) {
            getter.call("get", Some("apps"), json!({})).unwrap();
            n += 1;
        }
        n
    });
    let g = gaps(&mut sub, "apps", 3);
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(t.join().unwrap() >= 4, "the gets ran");
    for gap in &g {
        assert!(
            *gap < Duration::from_millis(2250) && *gap > Duration::from_millis(1750),
            "{g:?}"
        );
    }
}

#[test]
fn an_inspect_whose_pacman_is_slow_does_not_hold_the_stream() {
    use std::os::unix::fs::PermissionsExt;
    let bin = PathBuf::from("/tmp").join(format!("mqp-pacstub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bin);
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("pacman");
    std::fs::write(&stub, "#!/bin/sh\nsleep 2\nexit 1\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let apps = mirqab_core::samplers::apps::Apps::with_pacman(stub.to_string_lossy());
    let d = start_with("slowpac", vec![Box::new(apps)], false, None);
    let mut sub = Client::connect(&d);
    let mut getter = Client::connect(&d);
    sub.hello();
    getter.hello();
    sub.call("subscribe", Some("apps"), json!({"interval": 2000}))
        .unwrap();
    sub.event("apps").unwrap();
    // Asked a second after a frame, the 2 s answer spans the next tick:
    // before D1 that tick waited for it, a whole second late.
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        let t0 = Instant::now();
        let v = getter
            .call("get", Some("apps.inspect"), json!({"pid": 2001}))
            .unwrap();
        (t0.elapsed(), v)
    });
    let first = Instant::now();
    sub.event("apps").expect("the tick in the middle of pacman");
    let mid = first.elapsed();
    let g = gaps(&mut sub, "apps", 1);
    let (took, v) = t.join().unwrap();
    // The old code gave 3010 ms; 2600 leaves room for a loaded runner.
    assert!(mid < Duration::from_millis(2600), "{mid:?}");
    assert!(took >= Duration::from_secs(2), "pacman ran: {took:?}");
    assert_eq!(v["name"], "browser");
    for gap in &g {
        assert!(*gap < Duration::from_millis(2600), "{g:?}");
    }
    let _ = std::fs::remove_dir_all(&bin);
}

#[test]
fn status_names_no_focus_source_on_a_fixture() {
    let d = start("focus");
    let mut c = Client::connect(&d);
    c.hello();
    let s = c.call("status", None, Value::Null).unwrap();
    assert_eq!(s["focus"], Value::Null, "{s}");
    assert!(s.as_object().unwrap().contains_key("focus"));
    c.call("subscribe", Some("apps"), json!({})).unwrap();
    c.event("apps").unwrap();
    assert_eq!(
        c.call("status", None, Value::Null).unwrap()["focus"],
        Value::Null
    );
}
