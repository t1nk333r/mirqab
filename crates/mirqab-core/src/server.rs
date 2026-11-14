//! The daemon's socket: JSON lines, per-stream subscriptions, samplers that
//! run only while subscribed (architecture §5.1–§5.3).
//!
//! The shape (hello with capabilities on connect, subscriptions instead of a
//! request per frame, one sampler shared by every subscriber at the shortest
//! interval) is adapted from haseen's Rust sidecar `core/sidecar/src/server.rs`
//! (MIT, Copyright (c) 2026 t1nk33r), itself adapted from DankMaterialShell's
//! `core/internal/server` (MIT, Copyright (c) 2025 Avenge Media LLC).
//!
//! Threads: the caller's, accepting; per client one reader and one writer;
//! per sampler one thread, parked on a condition variable while nobody
//! subscribes. Every line to a client goes through its bounded queue, so a
//! slow client blocks nobody; one whose queue passes `QUEUE` frames is
//! dropped. Shared state sits behind one mutex, never held while sampling or
//! writing to a socket.

use crate::config::{Config, SetError};
use crate::exec;
use crate::history::{self, History};
use crate::proto::stream::{self, Machine, ProcessCost, SelfData, UiCost, UiEvent};
use crate::proto::{Code, Event, Hello, PROTOCOL, Reply, Request, ServerFrame, frame, strings};
use crate::root::Root;
use crate::sampler::{Sampler, View};
use crate::samplers::apps::Apps;
use crate::samplers::apps::actions::{self, Action, Actions};
use crate::samplers::system::System;
use crate::selfcost::{self, Meter};
use crate::sys;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The floor for a subscription without `live` (§5.3).
pub const FLOOR_MS: u64 = 2000;
/// Frames queued for one client before it is dropped (plan 001).
pub const QUEUE: usize = 256;
/// The systemd user unit that runs the interface (architecture §3).
pub const UI_UNIT: &str = "mirqab-ui.service";
/// How long `open` waits for a started interface to attach (plan 001 step 30).
const UI_START: Duration = Duration::from_secs(5);
/// The least time between two samples a subscription forces at once: a
/// client re-subscribing in a loop must not beat the floor (review of phase B).
const MIN_GAP: Duration = Duration::from_secs(1);
/// The longest request line.
const MAX_LINE: usize = 1 << 20;
const STACK: usize = 256 * 1024;

pub struct Options {
    pub socket: PathBuf,
    pub root: Root,
    pub config_path: PathBuf,
    /// Extra samplers beyond the built-in ones (tests register fakes).
    pub samplers: Vec<Box<dyn Sampler>>,
    /// The history file; None runs without history (some tests).
    pub history: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ListenError {
    AlreadyRunning,
    Failed(String),
}

struct Sub {
    /// The interval in force: what was asked, raised to the floor.
    interval_ms: u64,
    /// What the client asked for, so a new floor can be applied later.
    asked_ms: u64,
    live: bool,
    /// The subscription's params, for a stream whose sampler shapes each
    /// subscriber's view (`Sampler::view`).
    params: Value,
}

struct Client {
    tx: SyncSender<Vec<u8>>,
    conn: Arc<UnixStream>,
    subs: BTreeMap<String, Sub>,
    /// The pid this client gave when it subscribed to `ui` as the interface.
    ui_pid: Option<u32>,
    /// The peer's pid at connect time (SO_PEERCRED): the caller whose
    /// ancestry an action refuses (plan 006, decision 7).
    caller: Option<u32>,
}

impl Client {
    /// Queues a line; a client whose queue is full is cut off, which ends its
    /// reader and removes it.
    fn send(&self, line: &[u8]) {
        match self.tx.try_send(line.to_vec()) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                let _ = self.conn.shutdown(Shutdown::Both);
            }
        }
    }
}

#[derive(Default)]
struct SlotRun {
    last: Option<Instant>,
    /// Sample at once: a new subscriber should not wait a whole interval.
    now: bool,
}

struct Slot {
    sampler: Mutex<Box<dyn Sampler>>,
    live: bool,
    default_ms: u64,
    /// The sampler's view, copied out so shaping a sample for each
    /// subscriber does not lock the sampler.
    view: Option<View>,
    wake: Condvar,
}

struct State {
    clients: BTreeMap<u64, Client>,
    next_id: u64,
    runs: BTreeMap<String, SlotRun>,
    /// The interface: the latest client that subscribed to `ui` as one.
    ui: Option<(u64, u32)>,
    /// The interface being started, if one is: `systemctl start` ran and
    /// no interface has attached yet.
    starting_ui: Option<UiStart>,
    /// Numbers the start attempts.
    ui_starts: u64,
}

/// One attempt to start the interface. It owns the deadline and the queued
/// event, so a waiter leaves only when its own attempt ended (review of
/// PR #5: each caller cleared shared state on its own deadline, and one
/// could report success for an event that was never delivered).
struct UiStart {
    id: u64,
    deadline: Instant,
    /// Delivered right after the interface's `ui` subscribe reply; the
    /// latest `open` wins, and a `close` clears it.
    event: Option<UiEvent>,
}

impl State {
    /// The interface after the current one left: the latest other client
    /// that subscribed as one (review of phase B: an earlier interface was
    /// not restored).
    fn next_ui(&self) -> Option<(u64, u32)> {
        self.clients
            .iter()
            .rev()
            .find_map(|(id, c)| c.ui_pid.map(|pid| (*id, pid)))
    }
}

/// What the `self` sampler reports beyond its own process.
#[derive(Default)]
struct SelfShared {
    subscribers: BTreeMap<String, u32>,
    ui_pid: Option<u32>,
}

struct Inner {
    /// Held, never read: the locks are released when the process ends.
    _lock: std::fs::File,
    /// `<history>.lock`: one daemon per history file, whatever its socket.
    _history_lock: Option<std::fs::File>,
    root: Root,
    socket: PathBuf,
    started: Instant,
    own_uid: u32,
    /// Socket activation: systemd owns the socket file, so it is not removed.
    activated: bool,
    state: Mutex<State>,
    /// Signalled when a client subscribes to `ui` as the interface.
    ui_attached: Condvar,
    slots: BTreeMap<String, Arc<Slot>>,
    unavailable: BTreeMap<String, String>,
    config: Mutex<Config>,
    shared: Arc<Mutex<SelfShared>>,
    history: Option<Mutex<History>>,
    /// Why history is off, when a file was given but could not be opened.
    history_error: Option<String>,
    /// Quit and Force quit: the plans, and a sender that signals for real
    /// only when the root is `/` (plan 006, decision 2).
    actions: Actions,
}

pub struct Server {
    inner: Arc<Inner>,
    listener: UnixListener,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn log(msg: &str) {
    eprintln!("mirqab: {msg}");
}

/// The `self` stream (plan 001): the daemon's and the interface's cost.
struct SelfSampler {
    shared: Arc<Mutex<SelfShared>>,
    started: Instant,
    daemon: Option<Meter>,
    ui: Option<(u32, Meter)>,
    /// Read once: the CPU does not change while the daemon runs.
    cpu_model: Option<Option<String>>,
}

impl Sampler for SelfSampler {
    fn stream(&self) -> &'static str {
        stream::SELF
    }
    fn live(&self) -> bool {
        true
    }
    fn probe(&mut self, _root: &Root) -> Result<(), String> {
        Ok(())
    }
    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        let (subscribers, ui_pid) = {
            let s = lock(&self.shared);
            (s.subscribers.clone(), s.ui_pid)
        };
        let daemon = self
            .daemon
            .get_or_insert_with(|| Meter::new(root, "proc/self"))
            .read(root, now)
            .map(|r| ProcessCost {
                cpu: r.cpu,
                rss_bytes: r.rss_bytes,
                threads: r.threads,
            })
            .unwrap_or(ProcessCost {
                cpu: 0.0,
                rss_bytes: 0,
                threads: 0,
            });
        let ui = ui_pid.and_then(|pid| {
            if self.ui.as_ref().is_none_or(|(p, _)| *p != pid) {
                self.ui = Some((pid, Meter::new(root, format!("proc/{pid}"))));
            }
            let (_, meter) = self.ui.as_mut()?;
            meter.read(root, now).map(|r| UiCost {
                cpu: r.cpu,
                rss_bytes: r.rss_bytes,
                pid,
            })
        });
        let cpu_model = self
            .cpu_model
            .get_or_insert_with(|| {
                root.read("proc/cpuinfo")
                    .ok()
                    .and_then(|c| selfcost::cpu_model(&c))
            })
            .clone();
        let data = SelfData {
            daemon,
            ui,
            subscribers,
            uptime_sec: now.duration_since(self.started).as_secs(),
            machine: Machine {
                cpu_model,
                uptime_sec: root
                    .read("proc/uptime")
                    .ok()
                    .and_then(|u| selfcost::uptime_sec(&u)),
            },
        };
        serde_json::to_value(data).unwrap_or(Value::Null)
    }
}

fn mkdir_private(dir: &Path) -> Result<(), String> {
    // Only the leaf mirqab owns is private; parents it has to make (a
    // fresh `~/.local/state`) get the default mode (review of PR #6).
    if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("mkdir {}: {e}", dir.display())),
    }
}

/// The socket path: `--socket`, else `$MIRQAB_SOCKET`, else
/// `$XDG_RUNTIME_DIR/mirqab/daemon.sock`. None without a runtime dir.
pub fn default_socket() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("MIRQAB_SOCKET").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(s));
    }
    let run = std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty())?;
    Some(PathBuf::from(run).join("mirqab/daemon.sock"))
}

/// Binds the socket (or adopts systemd's), probes the samplers and starts
/// their threads. Nothing is accepted until `serve`.
/// Takes `<socket>.lock` for the life of the process. Two daemons started at
/// the same moment both see a dead socket and could both bind; only one gets
/// this lock, so only one serves and writes history (review of plan 001
/// phase B, deferred to phase C).
fn take_lock(socket: &Path) -> Result<std::fs::File, ListenError> {
    // The socket's directory is private (architecture §5.1).
    if let Some(dir) = socket.parent().filter(|d| !d.as_os_str().is_empty()) {
        mkdir_private(dir).map_err(ListenError::Failed)?;
    }
    lock_beside(socket)
}

/// Takes `<path>.lock` with `File::try_lock`; held means AlreadyRunning.
/// The directory is made with the default mode: a history lock must not
/// turn `~/.local/state` private (review of PR #6).
fn lock_beside(target: &Path) -> Result<std::fs::File, ListenError> {
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| ListenError::Failed(format!("mkdir {}: {e}", dir.display())))?;
    }
    let path = PathBuf::from(format!("{}.lock", target.display()));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| ListenError::Failed(format!("{}: {e}", path.display())))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(ListenError::AlreadyRunning),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(ListenError::Failed(format!("{}: {e}", path.display())))
        }
    }
}

pub fn listen(opts: Options) -> Result<Server, ListenError> {
    let lock = take_lock(&opts.socket)?;
    // A second daemon on another socket must not share the history file
    // (review of phase C, deferred to phase E).
    let history_lock = match &opts.history {
        None => None,
        Some(h) => Some(lock_beside(h).map_err(|e| match e {
            ListenError::AlreadyRunning => {
                ListenError::Failed(strings::history_in_use(&h.display().to_string()))
            }
            other => other,
        })?),
    };
    let (listener, activated) = match sys::listen_fd() {
        Some(l) => (l, true),
        None => (bind(&opts.socket)?, false),
    };

    let started = Instant::now();
    let shared = Arc::new(Mutex::new(SelfShared::default()));
    let mut samplers: Vec<Box<dyn Sampler>> = vec![Box::new(SelfSampler {
        shared: Arc::clone(&shared),
        started,
        daemon: None,
        ui: None,
        cpu_model: None,
    })];
    samplers.push(Box::new(System::new()));
    samplers.push(Box::new(Apps::new()));
    samplers.extend(opts.samplers);

    let mut slots = BTreeMap::new();
    let mut unavailable = BTreeMap::new();
    for mut s in samplers {
        let name = s.stream().to_string();
        if let Err(why) = s.probe(&opts.root) {
            unavailable.insert(name, why);
            continue;
        }
        slots.insert(
            name,
            Arc::new(Slot {
                live: s.live(),
                default_ms: s.default_interval().as_millis() as u64,
                view: s.view(),
                sampler: Mutex::new(s),
                wake: Condvar::new(),
            }),
        );
    }

    let (history, history_error) = match &opts.history {
        None => (None, None),
        Some(path) => match History::open(path, unix_now()) {
            Ok(h) => {
                if let Some(aside) = &h.moved_aside {
                    log(&format!(
                        "history could not be opened; moved it to {}",
                        aside.display()
                    ));
                }
                (Some(Mutex::new(h)), None)
            }
            Err(e) => {
                log(&format!("history is off: {e}"));
                (None, Some(e.to_string()))
            }
        },
    };
    let actions = Actions::new(crate::act::sender_for(&opts.root));
    let config = Config::load(&opts.config_path);
    if let Some(why) = config.broken() {
        log(&strings::config_broken(why));
    }
    let inner = Arc::new(Inner {
        _lock: lock,
        _history_lock: history_lock,
        root: opts.root,
        socket: opts.socket,
        started,
        own_uid: sys::uid(),
        activated,
        state: Mutex::new(State {
            clients: BTreeMap::new(),
            next_id: 0,
            runs: BTreeMap::new(),
            ui: None,
            starting_ui: None,
            ui_starts: 0,
        }),
        ui_attached: Condvar::new(),
        slots,
        unavailable,
        config: Mutex::new(config),
        shared,
        history,
        history_error,
        actions,
    });
    if inner.history.is_some() {
        let inner = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("history".into())
            .stack_size(STACK)
            .spawn(move || history_loop(inner))
            .map_err(|e| ListenError::Failed(e.to_string()))?;
    }
    for (name, slot) in &inner.slots {
        let (inner, name, slot) = (Arc::clone(&inner), name.clone(), Arc::clone(slot));
        std::thread::Builder::new()
            .name(format!("sampler-{name}"))
            .stack_size(STACK)
            .spawn(move || sampler_loop(inner, name, slot))
            .map_err(|e| ListenError::Failed(e.to_string()))?;
    }
    Ok(Server { inner, listener })
}

fn bind(socket: &Path) -> Result<UnixListener, ListenError> {
    if let Some(dir) = socket.parent().filter(|d| !d.as_os_str().is_empty()) {
        mkdir_private(dir).map_err(ListenError::Failed)?;
    }
    // A socket left by a daemon that died is not an owner: connect first,
    // and remove it only when it is a socket and nothing listens on it
    // (ECONNREFUSED). Anything else at the path, a file, a directory or a
    // symlink, is the user's and is never touched: a typo in --socket must
    // not delete data (review of plan 001 phase B).
    let refused = match UnixStream::connect(socket) {
        Ok(_) => return Err(ListenError::AlreadyRunning),
        Err(e) => e.kind() == ErrorKind::ConnectionRefused,
    };
    if let Ok(meta) = std::fs::symlink_metadata(socket) {
        if !meta.file_type().is_socket() {
            return Err(ListenError::Failed(strings::not_a_socket(
                &socket.display().to_string(),
            )));
        }
        if !refused {
            return Err(ListenError::Failed(strings::socket_busy(
                &socket.display().to_string(),
            )));
        }
        remove_socket(socket).map_err(ListenError::Failed)?;
    }
    let listener = UnixListener::bind(socket)
        .map_err(|e| ListenError::Failed(format!("bind {}: {e}", socket.display())))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| ListenError::Failed(format!("chmod {}: {e}", socket.display())))?;
    Ok(listener)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The minute writer (architecture §6.3): wakes once a minute, aligned to the
/// minute, asks every sampler for its minute values and writes them in one
/// transaction; prunes once an hour and compacts once a day. The only work
/// done with no subscribers.
fn history_loop(inner: Arc<Inner>) {
    let mut last_prune = 0;
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let into_minute = Duration::from_millis((now.as_millis() % 60_000) as u64);
        std::thread::sleep(Duration::from_secs(60) - into_minute);
        let ts = unix_now();
        let mut values = Vec::new();
        for slot in inner.slots.values() {
            values.extend(lock(&slot.sampler).minute(&inner.root));
        }
        let Some(h) = &inner.history else { return };
        let mut h = lock(h);
        // Woken just after a boundary, the values cover the minute that just
        // ended: label them with it (30 s back lands inside it, and a wake a
        // little early or late lands in the same minute). The minute running
        // now is then never half-written in a query (review of phase C).
        if let Err(e) = h.write_minute(ts - 30, &values) {
            log(&format!("history write failed: {e}"));
        }
        if ts - last_prune >= 3600 {
            if let Err(e) = h.prune(ts) {
                log(&format!("history prune failed: {e}"));
            }
            last_prune = ts;
        }
        // Once a day, pack the pages the minute writes split. The time of the
        // last compaction is kept in the file, so a machine that is shut down
        // every night still compacts (review of phase C).
        match h.compaction_due(ts) {
            Ok(true) => {
                if let Err(e) = h.compact(ts) {
                    log(&format!("history compaction failed: {e}"));
                }
            }
            Ok(false) => {}
            Err(e) => log(&format!("history compaction check failed: {e}")),
        }
    }
}

/// One sampler's thread: sleeps while nobody subscribes, otherwise samples at
/// the shortest interval any subscriber holds, at once for a new subscriber.
/// It holds the daemon's state for the life of the process.
fn sampler_loop(inner: Arc<Inner>, name: String, slot: Arc<Slot>) {
    let mut state = lock(&inner.state);
    loop {
        let interval = state
            .clients
            .values()
            .filter_map(|c| c.subs.get(&name).map(|s| s.interval_ms))
            .min();
        let run = state.runs.entry(name.clone()).or_default();
        let Some(ms) = interval else {
            run.now = false;
            state = slot.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            continue;
        };
        let now = Instant::now();
        let due = match run.last {
            None => now,
            Some(last) if !run.now => last + Duration::from_millis(ms),
            // At once for a new or changed subscription, but never twice
            // within MIN_GAP.
            Some(last) => (last + MIN_GAP).max(now),
        };
        if due > now {
            state = slot
                .wake
                .wait_timeout(state, due - now)
                .map(|(g, _)| g)
                .unwrap_or_else(|p| p.into_inner().0);
            continue;
        }
        run.now = false;
        run.last = Some(now);
        // The params each subscriber shapes its view with, taken while the
        // lock is held anyway; the views are made after it is released.
        let mut wanted: Vec<Value> = Vec::new();
        if slot.view.is_some() {
            for s in state.clients.values().filter_map(|c| c.subs.get(&name)) {
                if !wanted.contains(&s.params) {
                    wanted.push(s.params.clone());
                }
            }
        }
        drop(state);

        // `now` keeps the schedule; the sample is stamped once the sampler
        // is ours, as a `get` may have held it (apps sleeps 500 ms in one),
        // and a rate over a stale stamp reads high (review of PR #16).
        let data = {
            let mut sampler = lock(&slot.sampler);
            sampler.sample(&inner.root, Instant::now())
        };
        let event = |data: Value| {
            frame::encode(&ServerFrame::Event(Event {
                stream: name.clone(),
                data,
            }))
        };
        // Without a view every subscriber gets one line; with one, a line
        // per distinct params, and the sample is kept for a subscriber
        // whose params changed while it was taken.
        let (plain, views, kept) = match slot.view {
            None => (event(data), Vec::new(), None),
            Some(view) => {
                let views: Vec<(Value, Vec<u8>)> = wanted
                    .into_iter()
                    .map(|p| {
                        let line = event(view(&data, &p));
                        (p, line)
                    })
                    .collect();
                (Vec::new(), views, Some((view, data)))
            }
        };
        state = lock(&inner.state);
        for c in state.clients.values() {
            let Some(sub) = c.subs.get(&name) else {
                continue;
            };
            match &kept {
                None => c.send(&plain),
                Some((view, data)) => match views.iter().find(|(p, _)| *p == sub.params) {
                    Some((_, line)) => c.send(line),
                    None => c.send(&event(view(data, &sub.params))),
                },
            }
        }
    }
}

impl Server {
    pub fn socket(&self) -> &Path {
        &self.inner.socket
    }

    pub fn activated(&self) -> bool {
        self.inner.activated
    }

    /// Accepts clients until the process exits (`shutdown` or a signal).
    /// Returns only when accepting fails.
    pub fn serve(self) -> std::io::Error {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    match sys::peer_uid(&stream) {
                        Ok(uid) if uid == self.inner.own_uid => {}
                        _ => {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                    }
                    let caller = sys::peer_pid(&stream).ok();
                    let inner = Arc::clone(&self.inner);
                    let spawned = std::thread::Builder::new()
                        .name("client".into())
                        .stack_size(STACK)
                        .spawn(move || inner.handle(stream, caller));
                    if let Err(e) = spawned {
                        log(&format!("cannot serve a client: {e}"));
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                // Out of descriptors or memory, or a client that gave up
                // before we took it: transient. Any same-uid process can
                // cause these, so they must not end the daemon.
                Err(e)
                    if matches!(
                        e.raw_os_error(),
                        Some(
                            libc::EMFILE
                                | libc::ENFILE
                                | libc::ENOBUFS
                                | libc::ENOMEM
                                | libc::ECONNABORTED
                        )
                    ) =>
                {
                    log(&format!("accept: {e}; retrying"));
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return e,
            }
        }
    }
}

impl Inner {
    fn capabilities(&self) -> Vec<String> {
        let mut caps: Vec<String> = self.slots.keys().cloned().collect();
        caps.push(stream::UI.to_string());
        if self.history.is_some() {
            caps.push("history".to_string());
        }
        caps.sort();
        caps
    }

    fn handle(self: Arc<Self>, stream: UnixStream, caller: Option<u32>) {
        let conn = Arc::new(stream);
        let (tx, rx) = sync_channel::<Vec<u8>>(QUEUE);
        let writer_conn = Arc::clone(&conn);
        let writer = std::thread::Builder::new()
            .name("writer".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                for line in rx {
                    if (&*writer_conn).write_all(&line).is_err() {
                        let _ = writer_conn.shutdown(Shutdown::Both);
                        break;
                    }
                }
            });
        let Ok(writer) = writer else {
            return;
        };
        // A client that stops reading is cut off after this long, so joining
        // its writer below cannot hang.
        let _ = conn.set_write_timeout(Some(Duration::from_secs(10)));
        let hello = frame::encode(&ServerFrame::Hello(Hello {
            protocol: PROTOCOL,
            version: crate::proto::VERSION.to_string(),
            pid: std::process::id(),
            capabilities: self.capabilities(),
        }));
        let id = {
            let mut state = lock(&self.state);
            state.next_id += 1;
            let id = state.next_id;
            let client = Client {
                tx,
                conn: Arc::clone(&conn),
                subs: BTreeMap::new(),
                ui_pid: None,
                caller,
            };
            client.send(&hello);
            state.clients.insert(id, client);
            id
        };

        read_lines(&conn, |line| {
            match serde_json::from_slice::<Request>(line) {
                Ok(req) => self.dispatch(id, req),
                Err(e) => self.reply(
                    id,
                    Reply::err(0, Code::BadRequest, format!("bad json: {e}")),
                ),
            }
        });

        let streams: Vec<String> = {
            let mut state = lock(&self.state);
            let gone = state.clients.remove(&id);
            if state.ui.is_some_and(|(cid, _)| cid == id) {
                state.ui = state.next_ui();
            }
            self.refresh_shared(&state);
            gone.map(|c| c.subs.into_keys().collect())
                .unwrap_or_default()
        };
        for s in streams {
            self.wake(&s);
        }
        // The client and its queue's sender are gone; the writer drains what
        // was queued (the reply to a client that half-closed after asking)
        // and ends. Only then is the socket shut.
        let _ = writer.join();
        let _ = conn.shutdown(Shutdown::Both);
    }

    fn reply(&self, id: u64, reply: Reply) {
        let line = frame::encode(&ServerFrame::Reply(reply));
        if let Some(c) = lock(&self.state).clients.get(&id) {
            c.send(&line);
        }
    }

    fn wake(&self, stream: &str) {
        if let Some(slot) = self.slots.get(stream) {
            slot.wake.notify_all();
        }
    }

    /// Mirrors subscriber counts and the interface's pid for `self`.
    fn refresh_shared(&self, state: &State) {
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for c in state.clients.values() {
            for s in c.subs.keys() {
                *counts.entry(s.clone()).or_default() += 1;
            }
        }
        let mut shared = lock(&self.shared);
        shared.subscribers = counts;
        shared.ui_pid = state.ui.map(|(_, pid)| pid);
    }

    fn dispatch(&self, id: u64, req: Request) {
        let rid = req.id;
        let reply = match req.method.as_str() {
            "capabilities" => Reply::ok(rid, json!(self.capabilities())),
            "subscribe" => match self.subscribe(id, &req) {
                Ok(reply) => reply,
                // Already queued, ahead of the first event.
                Err(Replied) => return,
            },
            "unsubscribe" => self.unsubscribe(id, &req),
            "get" => self.get(&req),
            "status" => Reply::ok(rid, self.status()),
            "config.get" => self.config_get(&req),
            "history.query" => self.history_query(&req),
            "history.events" => self.history_events(&req),
            "history.list" => self.with_history(&req, |h| Ok(h.list())),
            "config.set" => self.config_set(&req),
            "action" => self.action(id, &req),
            "shutdown" => {
                self.reply(id, Reply::ok(rid, Value::Null));
                // Let the writer flush the reply before the process goes.
                std::thread::sleep(Duration::from_millis(50));
                if !self.activated {
                    let _ = remove_socket(&self.socket);
                }
                std::process::exit(0);
            }
            other => Reply::err(rid, Code::UnknownMethod, format!("unknown method: {other}")),
        };
        self.reply(id, reply);
    }

    fn stream_param(req: &Request) -> Result<&str, Reply> {
        req.stream
            .as_deref()
            .or_else(|| req.param("stream").and_then(Value::as_str))
            .ok_or_else(|| Reply::err(req.id, Code::BadRequest, "stream is required"))
    }

    fn check_stream(&self, req: &Request, name: &str) -> Option<Reply> {
        if let Some(why) = self.unavailable.get(name) {
            return Some(Reply::err(req.id, Code::Unavailable, why.clone()));
        }
        if !self.slots.contains_key(name) {
            return Some(Reply::err(
                req.id,
                Code::UnknownStream,
                format!("unknown stream: {name}"),
            ));
        }
        None
    }

    /// `Err(Replied)` when the reply was already queued (a stream
    /// subscription); otherwise the reply to send.
    fn subscribe(&self, id: u64, req: &Request) -> Result<Reply, Replied> {
        let name = match Self::stream_param(req) {
            Ok(n) => n,
            Err(r) => return Ok(r),
        };
        if name == stream::UI {
            let as_ui = req.param("role").and_then(Value::as_str) == Some("ui");
            let pid = req.param("pid").and_then(Value::as_u64).map(|p| p as u32);
            let reply = frame::encode(&ServerFrame::Reply(Reply::ok(
                req.id,
                json!({"stream": name}),
            )));
            let mut state = lock(&self.state);
            let pending = if as_ui && pid.is_some() {
                state.starting_ui.take().and_then(|s| s.event)
            } else {
                None
            };
            if let Some(c) = state.clients.get_mut(&id) {
                c.subs.insert(
                    name.to_string(),
                    Sub {
                        interval_ms: 0,
                        asked_ms: 0,
                        live: false,
                        params: Value::Null,
                    },
                );
                if as_ui {
                    c.ui_pid = pid;
                }
                // The reply, then the event an `open` queued while this
                // interface was starting (plan 001 step 30).
                c.send(&reply);
                if let Some(ev) = &pending {
                    c.send(&ui_line(ev));
                }
            }
            if as_ui && let Some(pid) = pid {
                state.ui = Some((id, pid));
                self.ui_attached.notify_all();
            }
            self.refresh_shared(&state);
            return Err(Replied);
        }
        if let Some(r) = self.check_stream(req, name) {
            return Ok(r);
        }
        let slot = &self.slots[name];
        let live = req.param("live").and_then(Value::as_bool).unwrap_or(false);
        let asked = req
            .param("interval")
            .and_then(Value::as_u64)
            .unwrap_or(slot.default_ms);
        let floor = if live && slot.live {
            let mut c = lock(&self.config);
            c.refresh();
            c.live_interval_ms()
        } else {
            FLOOR_MS
        };
        let interval_ms = asked.max(floor);
        let params = if slot.view.is_some() {
            view_params(&req.params)
        } else {
            Value::Null
        };
        let reply = frame::encode(&ServerFrame::Reply(Reply::ok(
            req.id,
            json!({"stream": name, "interval": interval_ms}),
        )));
        {
            // The reply is queued in the same locked step that adds the
            // subscription, so no event of this stream can reach the client
            // before it (architecture §5.2: reply, then events). A client
            // that skipped events while waiting for its reply lost the first
            // one (CI on PR #3).
            let mut state = lock(&self.state);
            if let Some(c) = state.clients.get_mut(&id) {
                c.subs.insert(
                    name.to_string(),
                    Sub {
                        interval_ms,
                        asked_ms: asked,
                        live: live && slot.live,
                        params,
                    },
                );
                c.send(&reply);
            }
            state.runs.entry(name.to_string()).or_default().now = true;
            self.refresh_shared(&state);
        }
        self.wake(name);
        Err(Replied)
    }

    fn unsubscribe(&self, id: u64, req: &Request) -> Reply {
        let name = match Self::stream_param(req) {
            Ok(n) => n,
            Err(r) => return r,
        };
        {
            let mut state = lock(&self.state);
            if let Some(c) = state.clients.get_mut(&id) {
                c.subs.remove(name);
                if name == stream::UI {
                    c.ui_pid = None;
                }
            }
            if name == stream::UI && state.ui.is_some_and(|(cid, _)| cid == id) {
                state.ui = state.next_ui();
            }
            self.refresh_shared(&state);
        }
        self.wake(name);
        Reply::ok(req.id, Value::Null)
    }

    fn get(&self, req: &Request) -> Reply {
        let name = match Self::stream_param(req) {
            Ok(n) => n,
            Err(r) => return r,
        };
        if name == stream::UI {
            return Reply::err(req.id, Code::BadRequest, "ui has events only");
        }
        // `apps.inspect`: a query under the `apps` stream (plan 006). A
        // stream's own name never holds a dot, so the split is unambiguous.
        if let Some((stream_name, query)) = name.split_once('.') {
            if let Some(r) = self.check_stream(req, stream_name) {
                return r;
            }
            let answer =
                lock(&self.slots[stream_name].sampler).query(&self.root, query, &req.params);
            return match answer {
                Some(Ok(v)) => Reply::ok(req.id, v),
                Some(Err((code, message))) => Reply::err(req.id, code, message),
                None => Reply::err(
                    req.id,
                    Code::UnknownStream,
                    format!("unknown stream: {name}"),
                ),
            };
        }
        if let Some(r) = self.check_stream(req, name) {
            return r;
        }
        let slot = &self.slots[name];
        let data = lock(&slot.sampler).snapshot(&self.root, Instant::now());
        match slot.view {
            Some(view) => Reply::ok(req.id, view(&data, &view_params(&req.params))),
            None => Reply::ok(req.id, data),
        }
    }

    fn status(&self) -> Value {
        let state = lock(&self.state);
        let mut streams = serde_json::Map::new();
        for name in self.capabilities().into_iter().filter(|n| n != "history") {
            let subs: Vec<&Sub> = state
                .clients
                .values()
                .filter_map(|c| c.subs.get(&name))
                .collect();
            let interval = if name == stream::UI {
                Value::Null
            } else {
                subs.iter()
                    .map(|s| s.interval_ms)
                    .min()
                    .map_or(Value::Null, |ms| json!(ms))
            };
            streams.insert(
                name,
                json!({
                    "subscribers": subs.len(),
                    "intervalMs": interval,
                    "live": subs.iter().any(|s| s.live),
                }),
            );
        }
        let ui = state.ui.map(|(_, pid)| pid);
        drop(state);
        let mut meter = Meter::new(&self.root, "proc/self");
        let cost = meter.read(&self.root, Instant::now());
        let mut config = lock(&self.config);
        config.refresh();
        json!({
            "version": crate::proto::VERSION,
            "protocol": PROTOCOL,
            "pid": std::process::id(),
            "socket": self.socket.display().to_string(),
            "activated": self.activated,
            "uptimeSec": self.started.elapsed().as_secs(),
            "streams": streams,
            "unavailable": self.unavailable,
            "ui": ui.map(|pid| json!({"pid": pid})),
            "daemon": {
                "rssBytes": cost.map(|c| c.rss_bytes).unwrap_or(0),
                "threads": cost.map(|c| c.threads).unwrap_or(0),
            },
            "config": {
                "broken": config.broken(),
                "problems": config.problems(),
            },
            "history": self.history_status(),
        })
    }

    fn config_get(&self, req: &Request) -> Reply {
        let mut config = lock(&self.config);
        config.refresh();
        match req.param("keys").and_then(Value::as_array) {
            None => Reply::ok(req.id, config.effective().clone()),
            Some(keys) => {
                let mut out = serde_json::Map::new();
                for k in keys.iter().filter_map(Value::as_str) {
                    out.insert(k.to_string(), config.get(k).cloned().unwrap_or(Value::Null));
                }
                Reply::ok(req.id, Value::Object(out))
            }
        }
    }

    fn config_set(&self, req: &Request) -> Reply {
        let Some(key) = req.param("key").and_then(Value::as_str) else {
            return Reply::err(req.id, Code::BadRequest, "key is required");
        };
        let value = req.param("value").cloned().unwrap_or(Value::Null);
        let dry_run = match dry_run_param(req) {
            Ok(d) => d,
            Err(r) => return r,
        };
        let result = lock(&self.config).set(key, value, dry_run);
        match result {
            Ok(entry) if dry_run => Reply::ok(req.id, json!({"wouldDo": [entry]})),
            Ok(entry) => {
                if key == "live.intervalMs" {
                    self.refloor_live();
                }
                self.ui_event(&UiEvent::ConfigChanged);
                Reply::ok(req.id, json!({"done": [entry]}))
            }
            Err(SetError::Invalid(why)) => Reply::err(req.id, Code::BadRequest, why),
            Err(SetError::Broken(why)) => {
                Reply::err(req.id, Code::Refused, strings::config_broken(&why))
            }
            Err(SetError::Write(why)) => Reply::err(req.id, Code::Failed, why),
        }
    }

    /// Applies a changed `live.intervalMs` to the live subscriptions already
    /// open, which got the old floor (review of phase B).
    fn refloor_live(&self) {
        let floor = {
            let mut c = lock(&self.config);
            c.refresh();
            c.live_interval_ms()
        };
        let mut woken = Vec::new();
        {
            let mut state = lock(&self.state);
            for c in state.clients.values_mut() {
                for (name, sub) in c.subs.iter_mut().filter(|(_, s)| s.live) {
                    sub.interval_ms = sub.asked_ms.max(floor);
                    woken.push(name.clone());
                }
            }
        }
        woken.sort();
        woken.dedup();
        for name in woken {
            self.wake(&name);
        }
    }

    fn history_status(&self) -> Value {
        match &self.history {
            None => json!({"enabled": false, "error": self.history_error}),
            Some(h) => {
                let h = lock(h);
                json!({
                    "enabled": true,
                    "path": h.path().display().to_string(),
                    "sizeBytes": h.size_bytes(),
                    "movedAside": h.moved_aside.as_ref().map(|p| p.display().to_string()),
                })
            }
        }
    }

    fn with_history(
        &self,
        req: &Request,
        f: impl FnOnce(&History) -> Result<Value, history::HistoryError>,
    ) -> Reply {
        let Some(h) = &self.history else {
            let why = self
                .history_error
                .clone()
                .unwrap_or_else(|| "history is off".into());
            return Reply::err(req.id, Code::Unavailable, why);
        };
        match f(&lock(h)) {
            Ok(v) => Reply::ok(req.id, v),
            Err(e) => Reply::err(req.id, Code::Failed, e.to_string()),
        }
    }

    /// `history.query`: `series[]` with `range` (1h, 24h, 7d, 30d) or
    /// `from`/`to` in unix seconds (then 1-minute buckets).
    fn history_query(&self, req: &Request) -> Reply {
        let series: Vec<&str> = match req.param("series").and_then(Value::as_array) {
            Some(a) => a.iter().filter_map(Value::as_str).collect(),
            None => return Reply::err(req.id, Code::BadRequest, "series is required"),
        };
        let now = unix_now();
        let (from, to, bucket) = match req.param("range").and_then(Value::as_str) {
            Some(name) => match history::range(name) {
                Some((span, bucket)) => (now - span, now + 1, bucket),
                None => {
                    return Reply::err(req.id, Code::BadRequest, format!("unknown range: {name}"));
                }
            },
            None => match (
                req.param("from").and_then(Value::as_i64),
                req.param("to").and_then(Value::as_i64),
            ) {
                (Some(f), Some(t)) if f < t => (f, t, 60),
                _ => {
                    return Reply::err(
                        req.id,
                        Code::BadRequest,
                        "range, or from and to, are required",
                    );
                }
            },
        };
        self.with_history(req, |h| h.query(&series, from, to, bucket))
    }

    fn history_events(&self, req: &Request) -> Reply {
        let kind = req.param("kind").and_then(Value::as_str);
        let now = unix_now();
        let from = req
            .param("from")
            .and_then(Value::as_i64)
            .unwrap_or(now - history::KEEP_SECS);
        let to = req.param("to").and_then(Value::as_i64).unwrap_or(now + 1);
        self.with_history(req, |h| h.events(kind, from, to))
    }

    /// Sends a `ui` event to every client subscribed to `ui`.
    fn ui_event(&self, event: &UiEvent) {
        let line = ui_line(event);
        let state = lock(&self.state);
        for c in state
            .clients
            .values()
            .filter(|c| c.subs.contains_key(stream::UI))
        {
            c.send(&line);
        }
    }

    /// `action` (§5.4): the window's three, `ui.show`, `ui.toggle` and
    /// `ui.hide` (`show` and `toggle` start the interface when none is
    /// attached, plan 001 step 30), and Quit and Force quit, `apps.quit`
    /// and `apps.kill` (plan 006), which need `confirm`.
    fn action(&self, client: u64, req: &Request) -> Reply {
        let Some(name) = req.param("name").and_then(Value::as_str) else {
            return Reply::err(req.id, Code::BadRequest, "name is required");
        };
        if let Some(action) = Action::from_name(name) {
            return self.apps_action(client, action, req);
        }
        let args = req.param("args").cloned().unwrap_or(Value::Null);
        let dry_run = match dry_run_param(req) {
            Ok(d) => d,
            Err(r) => return r,
        };
        let tab = args.get("tab").and_then(Value::as_str).map(str::to_string);
        let event = match name {
            "ui.show" => UiEvent::Show {
                tab,
                mode: args.get("mode").and_then(Value::as_str).map(str::to_string),
            },
            "ui.toggle" => UiEvent::Toggle { tab },
            "ui.hide" => UiEvent::Hide,
            other => {
                return Reply::err(req.id, Code::BadRequest, format!("unknown action: {other}"));
            }
        };
        let entry = serde_json::to_value(&event).unwrap_or(Value::Null);
        if dry_run {
            return Reply::ok(req.id, json!({"wouldDo": [entry]}));
        }
        match self.deliver_ui(event) {
            Ok(()) => Reply::ok(req.id, json!({"done": [entry]})),
            Err((code, why)) => Reply::err(req.id, code, why),
        }
    }

    /// `apps.quit` and `apps.kill`: parsed strictly, resolved and acted on by
    /// `actions.rs` with no lock of the daemon's held.
    fn apps_action(&self, client: u64, action: Action, req: &Request) -> Reply {
        let peer = lock(&self.state)
            .clients
            .get(&client)
            .and_then(|c| c.caller);
        let caller = test_caller().or(peer);
        let ask = match actions::parse(action, &req.params, caller) {
            Ok(a) => a,
            Err(f) => return Reply::err_data(req.id, f.code, f.message, f.data),
        };
        match self.actions.handle(&self.root, &ask, self.history.as_ref()) {
            Ok(v) => Reply::ok(req.id, v),
            Err(f) => Reply::err_data(req.id, f.code, f.message, f.data),
        }
    }

    /// Sends a window event to the interface, starting it first when none is
    /// attached: `systemctl --user start mirqab-ui.service` through exec.rs,
    /// the event queued on the start attempt and delivered when the
    /// interface subscribes to `ui`. One deadline covers the start and the
    /// wait. `hide` with no interface is done, and clears an event queued
    /// for one being started. Concurrent opens share one attempt; the latest
    /// event wins.
    fn deliver_ui(&self, event: UiEvent) -> Result<(), (Code, String)> {
        let mut state = lock(&self.state);
        if state.ui.is_some() {
            drop(state);
            self.ui_event(&event);
            return Ok(());
        }
        if event == UiEvent::Hide {
            if let Some(s) = &mut state.starting_ui {
                s.event = None;
            }
            return Ok(());
        }
        let timeout = ui_start_timeout();
        let (id, deadline, starter) = match &mut state.starting_ui {
            Some(s) => {
                s.event = Some(event);
                (s.id, s.deadline, false)
            }
            None => {
                state.ui_starts += 1;
                let (id, deadline) = (state.ui_starts, Instant::now() + timeout);
                state.starting_ui = Some(UiStart {
                    id,
                    deadline,
                    event: Some(event),
                });
                (id, deadline, true)
            }
        };
        drop(state);
        if starter {
            let started = exec::run(&["systemctl", "--user", "start", UI_UNIT], None, timeout);
            let failed = match started {
                Ok(out) if out.success() => None,
                Ok(out) => Some(out.stderr.trim().to_string()),
                Err(e) => Some(e.to_string()),
            };
            if let Some(why) = failed {
                let mut state = lock(&self.state);
                if state.starting_ui.as_ref().is_some_and(|s| s.id == id) {
                    state.starting_ui = None;
                }
                // Other opens waiting on this attempt leave now too.
                self.ui_attached.notify_all();
                return Err((Code::Failed, strings::interface_start_failed(&why)));
            }
        }
        let mut state = lock(&self.state);
        while state.starting_ui.as_ref().is_some_and(|s| s.id == id) {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                state.starting_ui = None;
                self.ui_attached.notify_all();
                break;
            };
            state = self
                .ui_attached
                .wait_timeout(state, left)
                .map(|(g, _)| g)
                .unwrap_or_else(|p| p.into_inner().0);
        }
        if state.ui.is_some() {
            Ok(())
        } else {
            Err((
                Code::Unavailable,
                strings::interface_did_not_attach().to_string(),
            ))
        }
    }
}

/// A `ui` event as a line.
fn ui_line(event: &UiEvent) -> Vec<u8> {
    frame::encode(&ServerFrame::Event(Event {
        stream: stream::UI.to_string(),
        data: serde_json::to_value(event).unwrap_or(Value::Null),
    }))
}

/// UI_START, or `MIRQAB_UI_START_MS` when `MIRQAB_TEST=1` (tests wait less).
fn ui_start_timeout() -> Duration {
    if std::env::var("MIRQAB_TEST").as_deref() == Ok("1")
        && let Some(ms) = std::env::var("MIRQAB_UI_START_MS")
            .ok()
            .and_then(|v| v.parse().ok())
    {
        return Duration::from_millis(ms);
    }
    UI_START
}

/// `MIRQAB_CALLER_PID` when `MIRQAB_TEST=1`: under a fixture root the real
/// caller has no `proc/<pid>/stat`, and the strict walk of its ancestry
/// would refuse every action, so a test names a fixture pid instead (plan
/// 006, decision 7).
fn test_caller() -> Option<u32> {
    if std::env::var("MIRQAB_TEST").as_deref() != Ok("1") {
        return None;
    }
    std::env::var("MIRQAB_CALLER_PID").ok()?.parse().ok()
}

/// A handler already queued its reply.
struct Replied;

/// The params a view reads: the request's, without the ones the server
/// itself takes, so two subscribers that differ only in interval share one
/// shaped line.
fn view_params(params: &Value) -> Value {
    let mut p = params.clone();
    if let Some(o) = p.as_object_mut() {
        for k in ["interval", "live", "stream"] {
            o.remove(k);
        }
    }
    p
}

/// `dryRun`: absent is false, a boolean is itself, and anything else is a
/// bad request. "true" or 1 must never fall through to the real action (the
/// dry-run contract, architecture §5.4).
fn dry_run_param(req: &Request) -> Result<bool, Reply> {
    match req.param("dryRun") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(Reply::err(
            req.id,
            Code::BadRequest,
            "dryRun must be true or false",
        )),
    }
}

/// Removes the socket at `path` only if it is a socket: never a file, a
/// directory or a symlink someone put there (review of plan 001 phase B).
fn remove_socket(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => crate::act::remove_path(path, false)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Ok(_) => Err(strings::not_a_socket(&path.display().to_string())),
        Err(_) => Ok(()),
    }
}

/// Calls `f` for every non-empty line, a trailing `\r` dropped, until the
/// client closes or sends a line longer than MAX_LINE.
fn read_lines(stream: &UnixStream, mut f: impl FnMut(&[u8])) {
    let mut reader = stream;
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        let mut start = 0;
        while let Some(off) = buf[start..].iter().position(|&b| b == b'\n') {
            let mut line = &buf[start..start + off];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if !line.is_empty() {
                f(line);
            }
            start += off + 1;
        }
        buf.drain(..start);
        if buf.len() >= MAX_LINE {
            return;
        }
    }
}
