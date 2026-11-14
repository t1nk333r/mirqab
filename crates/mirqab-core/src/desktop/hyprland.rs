//! Hyprland's focus, from its event socket (plan 006, part 4, decisions
//! 3 and 4; https://wiki.hypr.land/IPC/).
//!
//! The instance lives in `<runtime>/hypr/<signature>/`, with `.socket.sock`
//! for requests, `.socket2.sock` for events and `hyprland.lock`, whose
//! first line is the compositor's pid. It is found by
//! `$HYPRLAND_INSTANCE_SIGNATURE` on the live machine when that names a
//! directory with an event socket, otherwise by the directories whose lock
//! names a pid with a `proc/<pid>/` under the root, the newest first. The
//! variable may be missing (a daemon started before Hyprland exported it)
//! or stale (Hyprland restarted), so the lock is the fallback. Everything
//! is read through `Root`: on the live machine the runtime directory is
//! `$XDG_RUNTIME_DIR` under `/`, on a fixture root `run/user/<uid>` inside
//! the tree (`desktop::runtime_rel`), and the environment is not read.
//!
//! `start` connects to the event socket (with a timeout, `sys::connect_timeout`),
//! asks `j/activewindow` once for the focus now, and starts one thread,
//! `focus-hyprland`, that blocks reading event lines. An
//! `activewindowv2>>ADDRESS` line looks the address up; a miss asks
//! `j/activewindow` again (the window just focused is the active one) and
//! keeps `address -> pid`. `closewindow>>ADDRESS` forgets it. Every other
//! line is dropped unparsed: `activewindow` carries the class and the
//! title, and no window title is ever kept or logged. At most ten
//! requests a second are made; five failed requests in a row, an end of
//! file or a read error end the thread, and the source counts as stopped
//! until `start` runs again. `stop` shuts the event socket down, which
//! returns the blocked read, and joins the thread.

use super::{Focus, FocusedSince, focused_since, runtime_rel};
use crate::proto::strings;
use crate::root::Root;
use crate::sys;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The source's name, in the frame's `focus` and in `status.focus`.
pub const SOURCE: &str = "hyprland";
/// How long a connect to either socket may wait.
const CONNECT: Duration = Duration::from_millis(500);
/// How long a request may take to write and to answer.
const REQUEST: Duration = Duration::from_millis(500);
/// The longest answer read; `j/activewindow` is about 1 KiB.
const ANSWER_CAP: u64 = 64 * 1024;
/// The longest event line kept; a longer one is skipped whole.
const LINE_CAP: u64 = 64 * 1024;
/// How long a gain of focus is remembered: the tag's window is 5 minutes.
const KEEP: Duration = Duration::from_secs(6 * 60);
/// Failed requests in a row that end the reader.
const MAX_ERRORS: u32 = 5;
/// Requests a second at most; the excess is dropped, the focus unchanged.
const MAX_REQUESTS: usize = 10;
/// The reader's stack: it reads lines and parses one small object.
const STACK: usize = 64 * 1024;
/// The longest lock file read.
const LOCK_CAP: usize = 64;
/// Changes of focus kept at most, however fast they come.
const MAX_HISTORY: usize = 4096;

/// A window address as the events and the requests spell it: with or
/// without `0x`, in any case.
pub fn address(raw: &str) -> String {
    let a = raw.trim();
    let a = a
        .strip_prefix("0x")
        .or_else(|| a.strip_prefix("0X"))
        .unwrap_or(a);
    a.to_ascii_lowercase()
}

/// One event line, as far as focus goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A window gained focus; None: no window has it.
    Active(Option<String>),
    /// A window closed.
    Closed(String),
    /// Anything else, dropped unread.
    Other,
}

/// `EVENT>>DATA`, the two events the reader needs.
pub fn event(line: &str) -> Event {
    let line = line.trim_end_matches(['\n', '\r']);
    let Some((name, data)) = line.split_once(">>") else {
        return Event::Other;
    };
    match name {
        "activewindowv2" => {
            // No window: an empty address (a lone comma in older releases).
            let a = address(data.trim_matches(','));
            Event::Active((!a.is_empty()).then_some(a))
        }
        "closewindow" => Event::Closed(address(data)),
        _ => Event::Other,
    }
}

/// The address and pid of `j/activewindow`'s answer; None for a field
/// that is missing (no window) or not a positive pid.
pub fn active_answer(text: &[u8]) -> Result<(Option<String>, Option<u32>), String> {
    let v: Value = serde_json::from_slice(text).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err(strings::FOCUS_BAD_ANSWER.to_string());
    }
    let addr = v.get("address").and_then(Value::as_str).map(address);
    let pid = v
        .get("pid")
        .and_then(Value::as_i64)
        .filter(|p| *p > 0)
        .and_then(|p| u32::try_from(p).ok());
    Ok((addr.filter(|a| !a.is_empty()), pid))
}

/// The time left before `deadline`, or ETIMEDOUT when there is none.
fn left(deadline: Instant) -> Result<Duration, String> {
    let d = deadline.saturating_duration_since(Instant::now());
    if d.is_zero() {
        Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT).to_string())
    } else {
        Ok(d)
    }
}

/// Asks `j/activewindow` on the request socket at `path`. The write and
/// every read share one deadline, `REQUEST` after the connect, so an
/// answer that trickles in a byte at a time still ends in time (review
/// of PR #20, S2): with a timeout per read it could take minutes.
fn ask_active(path: &Path) -> Result<(Option<String>, Option<u32>), String> {
    let mut s = sys::connect_timeout(path, CONNECT).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + REQUEST;
    s.set_write_timeout(Some(left(deadline)?))
        .map_err(|e| e.to_string())?;
    s.write_all(b"j/activewindow").map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        s.set_read_timeout(Some(left(deadline)?))
            .map_err(|e| e.to_string())?;
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        }
        if buf.len() as u64 > ANSWER_CAP {
            return Err(strings::FOCUS_BAD_ANSWER.to_string());
        }
    }
    active_answer(&buf)
}

/// Whether the instance directory `dir` is a running Hyprland's: its
/// `hyprland.lock` names on its first line a pid with a `proc/<pid>/`
/// under the root, and its event socket is there. An old instance's
/// directory can outlive it.
fn live(root: &Root, dir: &str) -> bool {
    let Ok(lock) = root.read_prefix(&format!("{dir}/hyprland.lock"), LOCK_CAP) else {
        return false;
    };
    let pid = String::from_utf8_lossy(&lock)
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .filter(|p| *p > 0);
    pid.is_some_and(|p| root.exists(&format!("proc/{p}")))
        && root.exists(&format!("{dir}/.socket2.sock"))
}

/// The instance directory, as a path under the root: by `signature` (the
/// environment's, on the live machine only) when that instance is live,
/// else the newest live directory. A daemon keeps the signature of the
/// session it started in, so after a Hyprland restart the signature
/// names a dead instance and must not win (review of PR #20, S3).
pub fn instance(root: &Root, runtime: &str, signature: Option<&str>) -> Result<String, String> {
    let base = format!("{runtime}/hypr");
    let plain = |s: &str| !s.is_empty() && s != "." && s != ".." && !s.contains('/');
    if let Some(sig) = signature.filter(|s| plain(s)) {
        let dir = format!("{base}/{sig}");
        if live(root, &dir) {
            return Ok(dir);
        }
    }
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for name in root.read_dir(&base).unwrap_or_default() {
        if !plain(&name) {
            continue;
        }
        let dir = format!("{base}/{name}");
        if !live(root, &dir) {
            continue;
        }
        let mtime = root
            .modified(&dir)
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, dir));
        }
    }
    best.map(|(_, d)| d)
        .ok_or_else(|| strings::FOCUS_NO_HYPRLAND.to_string())
}

/// What the reader knows, shared with the sampler.
#[derive(Debug, Default)]
struct State {
    /// Every change of focus, oldest first: `Some(pid)` a window of that
    /// client, `None` no window. An entry holds from its time until the
    /// next one's (the last until now), so the oldest is kept while it
    /// still reaches into the last `KEEP` (`desktop::focused_since`).
    history: VecDeque<(Instant, Option<u32>)>,
    by_address: HashMap<String, u32>,
    /// False once the reader ended.
    alive: bool,
    /// Requests made in the last second.
    requests: VecDeque<Instant>,
    /// Requests asked of the compositor since start (tests read it).
    asked: u64,
}

impl State {
    /// Focus went to `pid` (None: no window). The same focus again is no
    /// change, and adds nothing (S7 of PR #20).
    fn changed(&mut self, pid: Option<u32>) {
        if self.history.back().map(|(_, p)| *p) == Some(pid) {
            return;
        }
        let at = Instant::now();
        self.history.push_back((at, pid));
        // One entry older than KEEP stays: it held focus into the window.
        while self.history.len() >= 2 && at.saturating_duration_since(self.history[1].0) > KEEP {
            self.history.pop_front();
        }
        // A script cycling windows faster than a person: the backstop.
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }

    /// Whether another request fits in this second; counts it if so.
    fn may_ask(&mut self) -> bool {
        let at = Instant::now();
        while self
            .requests
            .front()
            .is_some_and(|t| at.saturating_duration_since(*t) >= Duration::from_secs(1))
        {
            self.requests.pop_front();
        }
        if self.requests.len() >= MAX_REQUESTS {
            return false;
        }
        self.requests.push_back(at);
        self.asked += 1;
        true
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Reads one line of at most `LINE_CAP` bytes into `buf`; a longer line is
/// read to its end and left empty. Ok(0) at the end of the stream.
fn read_line(reader: &mut BufReader<UnixStream>, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    buf.clear();
    let n = reader.by_ref().take(LINE_CAP).read_until(b'\n', buf)?;
    if n == 0 || buf.ends_with(b"\n") || (n as u64) < LINE_CAP {
        return Ok(n);
    }
    // Too long: skip to the newline, keep nothing of it.
    loop {
        buf.clear();
        let m = reader.by_ref().take(LINE_CAP).read_until(b'\n', buf)?;
        if m == 0 || buf.ends_with(b"\n") {
            buf.clear();
            return Ok(n + m.max(1));
        }
    }
}

/// The reader thread's body.
fn read_events(mut reader: BufReader<UnixStream>, requests: PathBuf, state: Arc<Mutex<State>>) {
    let mut buf = Vec::new();
    let mut errors = 0u32;
    loop {
        match read_line(&mut reader, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // Event names and addresses are ASCII; a title that is not UTF-8
        // only reaches the `Other` arm.
        let line = String::from_utf8_lossy(&buf);
        match event(&line) {
            Event::Other => {}
            Event::Closed(a) => {
                lock(&state).by_address.remove(&a);
            }
            Event::Active(None) => lock(&state).changed(None),
            Event::Active(Some(a)) => {
                let mut st = lock(&state);
                if let Some(&pid) = st.by_address.get(&a) {
                    st.changed(Some(pid));
                    continue;
                }
                if !st.may_ask() {
                    continue;
                }
                drop(st);
                match ask_active(&requests) {
                    Ok((addr, pid)) => {
                        errors = 0;
                        let mut st = lock(&state);
                        if let (Some(addr), Some(pid)) = (addr, pid) {
                            st.by_address.insert(addr, pid);
                        }
                        st.changed(pid);
                    }
                    Err(_) => {
                        errors += 1;
                        if errors >= MAX_ERRORS {
                            break;
                        }
                    }
                }
            }
        }
    }
    lock(&state).alive = false;
}

/// A running reader.
struct Running {
    state: Arc<Mutex<State>>,
    /// The event socket, to shut down from `stop`.
    events: UnixStream,
    thread: Option<JoinHandle<()>>,
}

/// Hyprland as a focus source.
#[derive(Default)]
pub struct Hyprland {
    run: Option<Running>,
}

impl Hyprland {
    pub fn new() -> Self {
        Hyprland { run: None }
    }

    /// Requests made to the compositor since `start` (tests check the
    /// cache with it); None when not running.
    pub fn requests(&self) -> Option<u64> {
        self.run.as_ref().map(|r| lock(&r.state).asked)
    }

    /// Changes of focus kept (tests check the dedupe with it); None when
    /// not running.
    pub fn changes(&self) -> Option<usize> {
        self.run.as_ref().map(|r| lock(&r.state).history.len())
    }

    /// Whether the reader thread has ended (tests).
    pub fn finished(&self) -> bool {
        self.run
            .as_ref()
            .and_then(|r| r.thread.as_ref())
            .is_none_or(JoinHandle::is_finished)
    }
}

impl Focus for Hyprland {
    fn start(&mut self, root: &Root, own_uid: u32) -> Result<&'static str, String> {
        if self.source().is_some() {
            return Ok(SOURCE);
        }
        self.stop();
        let runtime =
            runtime_rel(root, own_uid).ok_or_else(|| strings::FOCUS_NO_HYPRLAND.to_string())?;
        let signature = if root.is_host() {
            std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()
        } else {
            None
        };
        let dir = instance(root, &runtime, signature.as_deref())?;
        let path = |name: &str| {
            root.path(&format!("{dir}/{name}"))
                .map_err(|e| e.to_string())
        };
        let (events_path, requests) = (path(".socket2.sock")?, path(".socket.sock")?);
        let events = sys::connect_timeout(&events_path, CONNECT)
            .map_err(|e| format!("{}: {e}", events_path.display()))?;
        let (addr, now) =
            ask_active(&requests).map_err(|e| format!("{}: {e}", requests.display()))?;
        let state = Arc::new(Mutex::new(State {
            alive: true,
            asked: 1,
            ..State::default()
        }));
        {
            let mut st = lock(&state);
            st.changed(now);
            if let (Some(a), Some(pid)) = (addr, now) {
                st.by_address.insert(a, pid);
            }
        }
        let reader = BufReader::new(events.try_clone().map_err(|e| e.to_string())?);
        let shared = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("focus-hyprland".into())
            .stack_size(STACK)
            .spawn(move || read_events(reader, requests, shared))
            .map_err(|e| e.to_string())?;
        self.run = Some(Running {
            state,
            events,
            thread: Some(thread),
        });
        Ok(SOURCE)
    }

    fn focused(&mut self, since: Instant) -> Option<FocusedSince> {
        let run = self.run.as_ref()?;
        let mut st = lock(&run.state);
        if !st.alive {
            return None;
        }
        Some(focused_since(st.history.make_contiguous(), since))
    }

    fn source(&self) -> Option<&'static str> {
        let run = self.run.as_ref()?;
        lock(&run.state).alive.then_some(SOURCE)
    }

    fn stop(&mut self) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        // Returns the blocked read with 0: the thread ends on its own,
        // within a request's deadline (`REQUEST`) if it is asking.
        let _ = run.events.shutdown(Shutdown::Both);
        if let Some(t) = run.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Hyprland {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_read_by_name_and_addresses_either_way() {
        assert_eq!(
            event("activewindowv2>>5633a0b1c2d3\n"),
            Event::Active(Some("5633a0b1c2d3".into()))
        );
        assert_eq!(
            event("activewindowv2>>0x5633A0B1C2D3"),
            Event::Active(Some("5633a0b1c2d3".into()))
        );
        assert_eq!(event("activewindowv2>>"), Event::Active(None));
        assert_eq!(event("activewindowv2>>,"), Event::Active(None));
        assert_eq!(
            event("closewindow>>0x5633a0b1c2d3"),
            Event::Closed("5633a0b1c2d3".into())
        );
        assert_eq!(event("activewindow>>example-term,~"), Event::Other);
        assert_eq!(event("workspace>>2"), Event::Other);
        assert_eq!(event("garbage"), Event::Other);
    }

    #[test]
    fn the_active_window_answer_gives_an_address_and_a_pid() {
        assert_eq!(
            active_answer(br#"{"address":"0x5633a0b1c2d3","pid":2001,"class":"x"}"#).unwrap(),
            (Some("5633a0b1c2d3".into()), Some(2001))
        );
        assert_eq!(active_answer(b"{}").unwrap(), (None, None));
        assert_eq!(
            active_answer(br#"{"address":"0x1","pid":-1}"#).unwrap(),
            (Some("1".into()), None)
        );
        assert!(active_answer(b"[]").is_err());
        assert!(active_answer(b"not json").is_err());
    }
}
