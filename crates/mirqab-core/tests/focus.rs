//! The Hyprland focus source against a fake Hyprland in a scratch root
//! (plan 006, part 4, "Tests"): instance discovery by the lock file (a
//! lock naming a pid with no `proc/<pid>` is skipped, the newer of two
//! wins, no `hypr/` is no source); the reader asks `j/activewindow` once
//! per new address and caches it, `closewindow` evicts, an empty address
//! is no focus, `0x` and bare addresses match; a request that hangs times
//! out and the reader goes on; five failures stop it; `stop` and an end of
//! file end its thread; and the `apps` sampler on a root without Hyprland
//! has no `focus`. Every line is synthetic: nothing reads the live
//! session's sockets (a fixture root never reads the environment).

use mirqab_core::Root;
use mirqab_core::desktop::Focus;
use mirqab_core::desktop::hyprland::{self, Hyprland};
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::apps::Apps;
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A scratch root under /tmp (a socket path must stay under 108 bytes)
/// with `proc/<pid>/` for each of `pids`.
fn root(name: &str, pids: &[u32]) -> Scratch {
    let d = PathBuf::from("/tmp").join(format!("mqf-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for p in pids {
        std::fs::create_dir_all(d.join(format!("proc/{p}"))).unwrap();
    }
    std::fs::create_dir_all(d.join("run/user/1000/hypr")).unwrap();
    Scratch(d)
}

/// What the fake answers on `.socket.sock`.
#[derive(Clone)]
enum Mode {
    Answer(String),
    /// Holds the connection two seconds and says nothing.
    Hang,
    /// Closes at once: an empty answer, which is no JSON.
    Close,
    /// Writes one byte every 100 ms, for ever.
    Trickle,
}

/// A fake Hyprland instance: `hyprland.lock`, an event socket the test
/// writes lines to, and a request socket that answers by `mode`.
struct Fake {
    dir: PathBuf,
    events: UnixListener,
    mode: Arc<Mutex<Mode>>,
    asked: Arc<AtomicU64>,
}

fn answer(addr: &str, pid: u32) -> Mode {
    Mode::Answer(format!(
        r#"{{"address":"0x{addr}","pid":{pid},"class":"example","title":"synthetic","focusHistoryID":0}}"#
    ))
}

fn fake(root: &Path, sig: &str, lock_pid: u32, mode: Mode) -> Fake {
    let dir = root.join(format!("run/user/1000/hypr/{sig}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("hyprland.lock"),
        format!("{lock_pid}\nwayland-1\n"),
    )
    .unwrap();
    let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    let requests = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let mode = Arc::new(Mutex::new(mode));
    let asked = Arc::new(AtomicU64::new(0));
    let (m, a) = (Arc::clone(&mode), Arc::clone(&asked));
    // Left running: it blocks in accept until the test process ends.
    std::thread::spawn(move || {
        for conn in requests.incoming() {
            let Ok(mut conn) = conn else { return };
            let mode = m.lock().unwrap().clone();
            let a = Arc::clone(&a);
            std::thread::spawn(move || {
                let mut buf = [0u8; 64];
                let n = conn.read(&mut buf).unwrap_or(0);
                if &buf[..n] != b"j/activewindow" {
                    return;
                }
                a.fetch_add(1, Ordering::SeqCst);
                match mode {
                    Mode::Answer(text) => {
                        let _ = conn.write_all(text.as_bytes());
                    }
                    Mode::Hang => std::thread::sleep(Duration::from_secs(2)),
                    Mode::Close => {}
                    Mode::Trickle => {
                        while conn.write_all(b" ").is_ok() {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            });
        }
    });
    Fake {
        dir,
        events,
        mode,
        asked,
    }
}

impl Fake {
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
    fn asked(&self) -> u64 {
        self.asked.load(Ordering::SeqCst)
    }
}

/// Polls `f` every 20 ms for up to 3 s.
fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("not within 3 s: {what}");
}

fn long_ago() -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap_or_else(Instant::now)
}

fn now_of(h: &mut Hyprland) -> Option<u32> {
    h.focused(long_ago()).and_then(|f| f.now)
}

#[test]
fn the_instance_is_found_by_its_lock_without_the_environment() {
    let s = root("find", &[1050, 1051]);
    let r = Root::new(&s.0);
    assert!(
        hyprland::instance(&r, "run/user/1000", None).is_err(),
        "none yet"
    );
    // A lock naming a pid with no proc/<pid>: an old instance's leftover.
    let _dead = fake(&s.0, "dead_1_1", 4242, answer("1", 1));
    assert!(hyprland::instance(&r, "run/user/1000", None).is_err());
    let _live = fake(&s.0, "live_1_2", 1050, answer("1", 1));
    assert_eq!(
        hyprland::instance(&r, "run/user/1000", None).unwrap(),
        "run/user/1000/hypr/live_1_2"
    );
    // Two live instances: the newer directory.
    let newer = fake(&s.0, "newer_1_3", 1051, answer("1", 1));
    let old = std::fs::File::open(s.0.join("run/user/1000/hypr/live_1_2")).unwrap();
    old.set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))
        .unwrap();
    assert_eq!(
        hyprland::instance(&r, "run/user/1000", None).unwrap(),
        "run/user/1000/hypr/newer_1_3"
    );
    // A signature naming a directory with an event socket wins, and one
    // that names a path is ignored.
    // A signature naming a dead instance (its lock's pid has no proc/
    // entry) does not win: after a Hyprland restart the daemon still has
    // the old one (review of PR #20, S3). A live one does.
    assert_eq!(
        hyprland::instance(&r, "run/user/1000", Some("dead_1_1")).unwrap(),
        "run/user/1000/hypr/newer_1_3"
    );
    assert_eq!(
        hyprland::instance(&r, "run/user/1000", Some("live_1_2")).unwrap(),
        "run/user/1000/hypr/live_1_2"
    );
    assert_eq!(
        hyprland::instance(&r, "run/user/1000", Some("../../../x")).unwrap(),
        "run/user/1000/hypr/newer_1_3"
    );
    drop(newer);
    let none = root("none", &[1050]);
    std::fs::remove_dir_all(none.0.join("run")).unwrap();
    assert!(hyprland::instance(&Root::new(&none.0), "run/user/1000", None).is_err());
}

#[test]
fn the_reader_asks_once_per_address_and_follows_focus() {
    let s = root("read", &[1050]);
    let f = fake(&s.0, "sig_1_1", 1050, answer("aaa", 2001));
    let mut h = Hyprland::new();
    assert_eq!(h.start(&Root::new(&s.0), 1000), Ok("hyprland"));
    assert_eq!(h.source(), Some("hyprland"));
    let (mut ev, _) = f.events.accept().unwrap();
    assert_eq!(now_of(&mut h), Some(2001), "the focus at start");
    assert_eq!(f.asked(), 1);

    let s1 = Instant::now();
    f.set(answer("bbb", 3000));
    ev.write_all(b"activewindowv2>>bbb\n").unwrap();
    eventually("focus on 3000", || now_of(&mut h) == Some(3000));
    assert_eq!(f.asked(), 2, "a new address is asked about once");
    // 2001 held focus into s1: it counts (review of PR #20, B1).
    assert_eq!(h.focused(s1).unwrap().since, [2001, 3000]);

    // The same focus again is no change (S7).
    let n = h.changes().unwrap();
    ev.write_all(b"activewindowv2>>0xaaa\nactivewindowv2>>0xaaa\nactivewindowv2>>aaa\n")
        .unwrap();
    eventually("on 2001", || now_of(&mut h) == Some(2001));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(h.changes(), Some(n + 1), "three lines, one change");
    ev.write_all(b"activewindowv2>>bbb\n").unwrap();
    eventually("on 3000", || now_of(&mut h) == Some(3000));

    // Hostile lines: a title holding `>>` and event names changes nothing
    // and asks nothing; a 200 000-byte line is skipped whole and the next
    // event read.
    let asked = f.asked();
    ev.write_all(b"windowtitlev2>>aaa,x>>y,activewindowv2>>zzz\n")
        .unwrap();
    ev.write_all(b"activewindow>>cls,t>>activewindowv2>>zzz\n")
        .unwrap();
    let mut long = b"activewindow>>cls,".to_vec();
    long.extend(std::iter::repeat_n(b'x', 200_000));
    long.extend(b"\nactivewindowv2>>0xaaa\n");
    ev.write_all(&long).unwrap();
    eventually("the event after the long line", || {
        now_of(&mut h) == Some(2001)
    });
    assert_eq!(f.asked(), asked, "nothing asked for the hostile lines");
    ev.write_all(b"activewindowv2>>bbb\n").unwrap();
    eventually("on 3000 again", || now_of(&mut h) == Some(3000));

    // The cached addresses, with and without 0x: nothing is asked.
    ev.write_all(b"activewindow>>example,synthetic title\nactivewindowv2>>0xaaa\n")
        .unwrap();
    eventually("back on 2001", || now_of(&mut h) == Some(2001));
    ev.write_all(b"activewindowv2>>0xBBB\n").unwrap();
    eventually("back on 3000", || now_of(&mut h) == Some(3000));
    assert_eq!(f.asked(), 2);
    assert_eq!(h.requests(), Some(2));

    // No window: no focus. A closed window is asked about again.
    ev.write_all(b"activewindowv2>>\n").unwrap();
    eventually("no focus", || {
        h.focused(long_ago()).is_some_and(|x| x.now.is_none())
    });
    std::thread::sleep(Duration::from_millis(20));
    let s2 = Instant::now();
    let blank = h.focused(s2).unwrap();
    assert!(blank.since.is_empty() && blank.now.is_none(), "{blank:?}");
    ev.write_all(b"closewindow>>bbb\nactivewindowv2>>bbb\n")
        .unwrap();
    eventually("3000 again", || now_of(&mut h) == Some(3000));
    assert_eq!(f.asked(), 3, "evicted, so asked again");
    let seen = h.focused(long_ago()).unwrap();
    assert_eq!(seen.since, [2001, 3000]);
    assert_eq!(
        h.focused(Instant::now() + Duration::from_secs(1))
            .unwrap()
            .since,
        [3000],
        "only the focus now holds into a moment to come"
    );

    // stop ends the thread, at once.
    let t = Instant::now();
    h.stop();
    assert!(t.elapsed() < Duration::from_secs(1));
    assert!(h.finished());
    assert_eq!(h.source(), None);
    assert!(h.focused(long_ago()).is_none());
}

#[test]
fn a_hung_request_times_out_and_five_failures_stop_the_reader() {
    let s = root("fail", &[1050]);
    let f = fake(&s.0, "sig_1_1", 1050, answer("aaa", 2001));
    let mut h = Hyprland::new();
    h.start(&Root::new(&s.0), 1000).unwrap();
    let (mut ev, _) = f.events.accept().unwrap();
    f.set(Mode::Hang);
    let t = Instant::now();
    ev.write_all(b"activewindowv2>>c1\n").unwrap();
    eventually("the hung request asked", || f.asked() == 2);
    f.set(answer("c2", 3000));
    ev.write_all(b"activewindowv2>>c2\n").unwrap();
    eventually("the reader went on", || now_of(&mut h) == Some(3000));
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());

    f.set(Mode::Close);
    for i in 0..5 {
        ev.write_all(format!("activewindowv2>>d{i}\n").as_bytes())
            .unwrap();
    }
    eventually("five failures end it", || h.finished());
    assert!(h.focused(long_ago()).is_none());
    assert_eq!(h.source(), None);
    h.stop();
}

#[test]
fn an_end_of_file_ends_the_reader() {
    let s = root("eof", &[1050]);
    let f = fake(&s.0, "sig_1_1", 1050, answer("aaa", 2001));
    let mut h = Hyprland::new();
    h.start(&Root::new(&s.0), 1000).unwrap();
    let (ev, _) = f.events.accept().unwrap();
    drop(ev);
    eventually("the end of the stream ends it", || h.finished());
    assert_eq!(h.source(), None);
    // Started again, it finds the instance again.
    assert_eq!(h.start(&Root::new(&s.0), 1000), Ok("hyprland"));
    let _ev = f.events.accept().unwrap();
    assert_eq!(h.source(), Some("hyprland"));
    h.stop();
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn a_root_without_hyprland_has_no_focus_source() {
    let fixture = Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop"),
    );
    let mut h = Hyprland::new();
    assert!(h.start(&fixture, 1000).is_err());
    assert!(h.focused(long_ago()).is_none());
    let mut a = Apps::with_pacman("pacman-not-needed");
    a.probe(&fixture).unwrap();
    let v = a.sample(&fixture, Instant::now());
    assert!(v.get("focus").is_none(), "no source, no focus key");
    assert_eq!(a.status(), vec![("focus", Value::Null)]);
    a.idle();
}

#[test]
fn a_trickled_answer_ends_in_time_and_stop_is_bounded() {
    // Review of PR #20, S2: one deadline across the request's write and
    // reads, so an answer a byte at a time cannot hold the reader.
    let s = root("trickle", &[1050]);
    let f = fake(&s.0, "sig_1_1", 1050, answer("aaa", 2001));
    let mut h = Hyprland::new();
    h.start(&Root::new(&s.0), 1000).unwrap();
    let (mut ev, _) = f.events.accept().unwrap();
    f.set(Mode::Trickle);
    ev.write_all(b"activewindowv2>>t1\n").unwrap();
    eventually("the trickled request asked", || f.asked() == 2);
    let t = Instant::now();
    h.stop();
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    assert!(h.finished());
    // A start whose first request hangs ends within its bounds.
    let s = root("hangstart", &[1050]);
    let _f = fake(&s.0, "sig_1_1", 1050, Mode::Hang);
    let mut h = Hyprland::new();
    let t = Instant::now();
    let _ = h.start(&Root::new(&s.0), 1000);
    assert!(
        t.elapsed() < Duration::from_millis(1200),
        "{:?}",
        t.elapsed()
    );
    h.stop();
}
