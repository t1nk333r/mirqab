//! The `apps` stream (plan 006): every process of `proc/`, grouped into
//! apps, with CPU from the difference between two readings, an estimate of
//! memory, and storage rates. Every 2 s while subscribed, also while the
//! window is open: the list is a table, not a live graph, and this walk is
//! the costliest sampler, so the 1 s rule does not apply (architecture
//! §5.3). `get apps.inspect {pid, start?}` explains one process
//! (`inspect.rs`); its `pacman` question runs after the server released
//! this sampler's lock (`Answer::Later`).
//!
//! One sample serves every subscriber; `view` gives each its own order,
//! length and expanded groups from its `params` (`sort`, `limit`,
//! `expand`). `actions.rs` answers `apps.quit` and `apps.kill`.
//!
//! Part 4: while the stream has subscribers a focus source runs
//! (`crate::desktop`, started with the first sample, stopped in `idle`),
//! and each group's CPU is kept over five minutes for the "idle but
//! heavy" tag (`focus.rs`). Once a minute, subscribers or not, the
//! `top-apps` event names the heaviest groups (`minute_events`).

pub mod actions;
pub mod desktop;
pub mod focus;
pub mod group;
pub mod inspect;
pub mod procfs;

use crate::desktop::Focus;
use crate::desktop::hyprland::Hyprland;
use crate::proto::stream::{
    APPS, AppGroup, AppProcess, AppTop, AppsData, AppsTotals, GroupRef, Inspect,
};
use crate::proto::{Code, strings};
use crate::root::Root;
use crate::sampler::{Answer, Sampler, View};
use crate::selfcost;
use desktop::Desktop;
use focus::{Thresholds, Windows};
use group::{Group, Kind};
use inspect::Owners;
use procfs::{Cache, Proc, User};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A baseline older than this is dropped: the first sample after a quiet
/// spell would otherwise report the spell's average (as in `system`).
const LIVE_STALE: Duration = Duration::from_secs(10);
/// A baseline younger than this gives no useful rate.
const BASELINE_MIN: Duration = Duration::from_secs(1);
/// How long `get` waits for its own second reading when no subscriber's
/// baseline fits: half a second is 50 ticks, so 2 % of a core is the
/// smallest step it can tell.
const SNAPSHOT_GAP: Duration = Duration::from_millis(500);
/// Groups a view returns when its params name no `limit` (plan 006).
pub const DEFAULT_LIMIT: usize = 50;
/// The `event` kind of the minute's heaviest groups (plan 006, part 4).
pub const TOP_APPS: &str = "top-apps";
/// Groups named in a `top-apps` row, by CPU and by memory.
const TOP_N: usize = 3;
/// A `top-apps` row is written only when the CPU leader used at least this
/// much of one core (owner, 2026-11-14, part 4 Q4).
pub const TOP_APPS_MIN_CPU: f64 = 10.0;
/// The minute walk's baseline, older than this, gives no row: a missed
/// minute or a suspend lies between.
const MINUTE_STALE: Duration = Duration::from_secs(150);
/// How often a focus source that is not running is asked to start while
/// the stream has subscribers.
pub const FOCUS_RETRY: Duration = Duration::from_secs(30);
/// Primed readings queued at most (one per pending `get`).
pub const MAX_PRIMED: usize = 16;
/// The youngest primed reading a `snapshot` uses: at 100 ticks a second
/// one tick is 4 % of a core over 250 ms.
pub const PRIMED_MIN: Duration = Duration::from_millis(250);

/// One process's counters: CPU ticks, and storage bytes read and written
/// when the daemon may read them.
type Counters = (u64, Option<(u64, u64)>);

/// Counters of one walk, by pid and start time.
#[derive(Clone)]
struct Tick {
    at: Instant,
    counters: HashMap<(u32, u64), Counters>,
}

impl Tick {
    fn of(procs: &[Proc], at: Instant) -> Tick {
        Tick {
            at,
            counters: procs
                .iter()
                .map(|p| (p.key(), (p.stat.ticks, p.io)))
                .collect(),
        }
    }
}

pub struct Apps {
    cache: Cache,
    desktop: Desktop,
    /// Behind its own lock: the inspector's `pacman` call runs with the
    /// sampler's lock released (plan 006, part 4, decision 10).
    owners: Arc<Mutex<Owners>>,
    own_uid: u32,
    users: Vec<User>,
    own_home: Option<String>,
    tck: u64,
    page: u64,
    cores: Option<u32>,
    live: Option<Tick>,
    /// The first readings `prime` took, one per pending `get`, oldest
    /// first (review of PR #20, S1): each `prime` queues one and each
    /// `snapshot` takes one (plan 023's follow-up).
    get_base: VecDeque<Tick>,
    focus: Box<dyn Focus>,
    /// When the focus source may be asked to start again while none runs:
    /// once per `FOCUS_RETRY` while subscribed, so a Hyprland restarted
    /// with the page open is found again and a machine without one is
    /// not searched every tick (review of PR #20, S3). None after `idle`.
    focus_retry: Option<Instant>,
    windows: Windows,
    thresholds: Thresholds,
    /// The last sample, while subscribed: the minute's row is read from it.
    last: Option<(Instant, AppsData)>,
    /// The minute walk's baseline when nobody subscribes.
    minute_tick: Option<Tick>,
    /// The names of the last `top-apps` row written.
    last_top: Option<Vec<String>>,
    /// Walks of `proc/` so far (tests count them).
    walks: u64,
}

impl Default for Apps {
    fn default() -> Self {
        Self::new()
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Apps {
    pub fn new() -> Self {
        Self::with_pacman("pacman")
    }

    /// `program` answers `-Qo` for the inspector: tests pass a stub.
    pub fn with_pacman(program: impl Into<String>) -> Self {
        Apps {
            cache: Cache::default(),
            desktop: Desktop::default(),
            owners: Arc::new(Mutex::new(Owners::new(program))),
            own_uid: crate::sys::uid(),
            users: Vec::new(),
            own_home: None,
            tck: 100,
            page: 4096,
            cores: None,
            live: None,
            get_base: VecDeque::new(),
            focus: Box::new(Hyprland::new()),
            focus_retry: None,
            windows: Windows::default(),
            thresholds: Thresholds::default(),
            last: None,
            minute_tick: None,
            last_top: None,
            walks: 0,
        }
    }

    /// Another focus source (tests pass a fake).
    pub fn set_focus(&mut self, focus: Box<dyn Focus>) {
        self.focus.stop();
        self.focus = focus;
        self.focus_retry = None;
    }

    /// Other thresholds for the tag (tests shrink the window).
    pub fn set_thresholds(&mut self, t: Thresholds) {
        self.thresholds = t;
    }

    /// Walks of `proc/` so far.
    pub fn walks(&self) -> u64 {
        self.walks
    }

    /// Primed readings waiting for their `snapshot` (tests).
    pub fn primed(&self) -> usize {
        self.get_base.len()
    }

    /// Takes a `get`'s primed reading: the oldest queued that is 250 ms to
    /// 10 s old at `now` (review of PR #20, S1: one slot let a second
    /// `get` read rates over a millisecond). Older ones are dropped;
    /// younger ones belong to gets still in their gap and stay queued for
    /// them (plan 023's follow-up: a get with none of its own popped the
    /// next get's, dropped it, and both slept under the lock). Every
    /// reading is a whole walk, so one get's serves another as well.
    fn take_mine(&mut self, now: Instant) -> Option<Tick> {
        self.get_base
            .retain(|t| now.saturating_duration_since(t.at) <= LIVE_STALE);
        let at = self
            .get_base
            .iter()
            .position(|t| now.saturating_duration_since(t.at) >= PRIMED_MIN)?;
        self.get_base.remove(at)
    }

    /// Groups with a CPU window kept (tests).
    pub fn windows(&self) -> usize {
        self.windows.len()
    }

    /// Processes whose command line the cache holds (tests).
    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    fn walk(&mut self, root: &Root) -> Vec<Proc> {
        self.walks += 1;
        self.desktop.refresh(root);
        procfs::walk(root, &mut self.cache, self.own_uid)
    }

    fn groups(&self, procs: &[Proc]) -> Vec<Group> {
        group::group(procs, self.own_uid, &self.desktop)
    }

    /// Every group with every member; `view` trims it per subscriber.
    /// `groups` is `group::group` of `procs`, in its order.
    fn build(
        &self,
        procs: &[Proc],
        groups: &[Group],
        prev: Option<&Tick>,
        at: Instant,
    ) -> AppsData {
        let secs = prev.map(|p| at.saturating_duration_since(p.at).as_secs_f64());
        let rate = |p: &Proc| -> Option<f64> {
            let (prev, secs) = (prev?, secs.filter(|s| *s > 0.0)?);
            let dt = match prev.counters.get(&p.key()) {
                Some((t0, _)) => p.stat.ticks.saturating_sub(*t0),
                // New since the baseline (or a pid reused with another start
                // time): its ticks are not this interval's alone.
                None => 0,
            };
            Some(dt as f64 / self.tck as f64 / secs * 100.0)
        };
        let io_rate = |p: &Proc| -> Option<(f64, f64)> {
            let (prev, secs) = (prev?, secs.filter(|s| *s > 0.0)?);
            let (r1, w1) = p.io?;
            let (r0, w0) = match prev.counters.get(&p.key()) {
                Some((_, Some(io))) => *io,
                _ => (r1, w1),
            };
            Some((
                r1.saturating_sub(r0) as f64 / secs,
                w1.saturating_sub(w0) as f64 / secs,
            ))
        };
        let cpus: Vec<Option<f64>> = procs.iter().map(rate).collect();
        let mut out = Vec::with_capacity(groups.len());
        let (mut all_procs, mut all_threads, mut all_cpu) = (0u32, 0u32, None::<f64>);
        for g in groups {
            let mut cpu = None::<f64>;
            let (mut private, mut shared_max, mut threads) = (0u64, 0u64, 0u32);
            let (mut io, mut top): (Option<(f64, f64)>, Option<usize>) = (None, None);
            let mut processes = Vec::with_capacity(g.members.len());
            for &i in &g.members {
                let p = &procs[i];
                if let Some(c) = cpus[i] {
                    cpu = Some(cpu.unwrap_or(0.0) + c);
                }
                let (res, shr) = p.mem.map_or((0, 0), |m| (m.resident, m.shared));
                private += res.saturating_sub(shr);
                shared_max = shared_max.max(shr);
                threads += p.stat.threads;
                if let Some((r, w)) = io_rate(p) {
                    let (r0, w0) = io.unwrap_or((0.0, 0.0));
                    io = Some((r0 + r, w0 + w));
                }
                let heavier = |t: usize| {
                    let key = |j: usize| {
                        (
                            cpus[j].unwrap_or(-1.0),
                            procs[j].mem.map_or(0, |m| m.resident),
                        )
                    };
                    let (a, b) = (key(i), key(t));
                    a.0 > b.0 || (a.0 == b.0 && a.1 > b.1)
                };
                if top.is_none_or(heavier) {
                    top = Some(i);
                }
                processes.push(AppProcess {
                    pid: p.pid(),
                    ppid: p.stat.ppid,
                    name: p.name(),
                    cpu: cpus[i].map(round1),
                    rss_bytes: p.mem.map_or(0, |m| m.resident * self.page),
                    threads: p.stat.threads,
                    state: p.stat.state.to_string(),
                    start: p.stat.start,
                });
            }
            processes.sort_by_key(|p| p.pid);
            all_procs += g.members.len() as u32;
            all_threads += threads;
            if let Some(c) = cpu {
                all_cpu = Some(all_cpu.unwrap_or(0.0) + c);
            }
            out.push(AppGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                icon: g.icon.clone(),
                kind: g.kind.as_str().to_string(),
                unit: g.unit.clone(),
                cpu: cpu.map(round1),
                rss_bytes: (private + shared_max) * self.page,
                procs: g.members.len() as u32,
                threads,
                io_read_per_sec: io.map(|(r, _)| round1(r)),
                io_write_per_sec: io.map(|(_, w)| round1(w)),
                pids: processes.iter().map(|p| p.pid).collect(),
                top: top.map(|i| AppTop {
                    pid: procs[i].pid(),
                    name: procs[i].name(),
                    cpu: cpus[i].map(round1),
                }),
                processes: Some(processes),
                idle_heavy: false,
            });
        }
        // Counted before any view's limit, so a reader can tell a cut list
        // (review of PR #16).
        let n = out.len() as u32;
        AppsData {
            groups: out,
            totals: AppsTotals {
                groups: n,
                procs: all_procs,
                threads: all_threads,
                cpu: all_cpu.map(round1),
                cores: self.cores,
            },
            sort: "cpu".to_string(),
            focus: None,
        }
    }

    /// "Idle but heavy" on `d`, from the windows kept and the focus now:
    /// nothing, and no `focus`, while no focus source runs.
    fn tag(&mut self, d: &mut AppsData, procs: &[Proc], groups: &[Group], now: Instant) {
        let since = now.checked_sub(self.thresholds.window).unwrap_or(now);
        let Some(seen) = self.focus.focused(since) else {
            return;
        };
        d.focus = self.focus.source().map(str::to_string);
        let credited = focus::credited(procs, groups, &seen.pids());
        for (i, g) in d.groups.iter_mut().enumerate() {
            g.idle_heavy = g.kind != Kind::System.as_str()
                && !credited.contains(&i)
                && self.windows.heavy(&g.id, now, self.thresholds);
        }
    }

    /// A tagged frame of `procs` against `prev`.
    fn data(&mut self, procs: &[Proc], prev: Option<&Tick>, at: Instant) -> Value {
        let groups = self.groups(procs);
        let mut d = self.build(procs, &groups, prev, at);
        self.tag(&mut d, procs, &groups, at);
        serde_json::to_value(d).unwrap_or(Value::Null)
    }

    /// Whether a subscriber's baseline is 1–10 s old.
    fn young(&self, now: Instant) -> bool {
        self.live.as_ref().is_some_and(|t| {
            (BASELINE_MIN..=LIVE_STALE).contains(&now.saturating_duration_since(t.at))
        })
    }

    /// `minute_events` at `now` (tests move the clock).
    pub fn top_apps(
        &mut self,
        root: &Root,
        config: &Value,
        now: Instant,
    ) -> Vec<(&'static str, Value)> {
        if config.pointer("/trends/topApps").and_then(Value::as_bool) == Some(false) {
            self.minute_tick = None;
            return Vec::new();
        }
        let fresh = self
            .last
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) <= LIVE_STALE);
        let (row, names, leader) = if let Some((_, d)) = fresh {
            let top = top_row(d);
            // The walk the subscriber paid for is the next baseline.
            self.minute_tick = self.live.clone();
            top
        } else {
            let procs = self.walk(root);
            let prev = self
                .minute_tick
                .take()
                .filter(|t| now.saturating_duration_since(t.at) <= MINUTE_STALE);
            self.minute_tick = Some(Tick::of(&procs, now));
            let Some(prev) = prev else {
                return Vec::new();
            };
            let groups = self.groups(&procs);
            top_row(&self.build(&procs, &groups, Some(&prev), now))
        };
        if leader < TOP_APPS_MIN_CPU || self.last_top.as_ref() == Some(&names) {
            return Vec::new();
        }
        self.last_top = Some(names);
        vec![(TOP_APPS, row)]
    }

    /// The group `pid` is counted in, from a fresh walk.
    fn group_of(&mut self, root: &Root, pid: u32) -> Option<GroupRef> {
        let procs = self.walk(root);
        let i = procs.iter().position(|p| p.pid() == pid)?;
        self.groups(&procs)
            .into_iter()
            .find(|g| g.members.contains(&i))
            .map(|g| GroupRef {
                id: g.id,
                name: g.name,
            })
    }
}

/// The `top-apps` row of a frame: up to three groups by CPU and three by
/// memory, by name only (never an id, a unit, a pid or a command line: a
/// group id can hold the command a launcher ran), the process count, the
/// six names in order, and the CPU leader's share.
pub fn top_row(d: &AppsData) -> (Value, Vec<String>, f64) {
    let cpu = |g: &AppGroup| g.cpu.unwrap_or(0.0);
    let mut by_cpu: Vec<&AppGroup> = d.groups.iter().collect();
    by_cpu.sort_by(|a, b| cpu(b).total_cmp(&cpu(a)).then(a.name.cmp(&b.name)));
    let mut by_mem: Vec<&AppGroup> = d.groups.iter().collect();
    by_mem.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes).then(a.name.cmp(&b.name)));
    by_cpu.truncate(TOP_N);
    by_mem.truncate(TOP_N);
    let names: Vec<String> = by_cpu
        .iter()
        .chain(by_mem.iter())
        .map(|g| g.name.clone())
        .collect();
    let leader = by_cpu.first().map_or(0.0, |g| cpu(g));
    let row = json!({
        "cpu": by_cpu.iter().map(|g| json!([g.name, round1(cpu(g))])).collect::<Vec<_>>(),
        "mem": by_mem.iter().map(|g| json!([g.name, g.rss_bytes])).collect::<Vec<_>>(),
        "procs": d.totals.procs,
    });
    (row, names, leader)
}

/// The order a view asks for: `cpu` (default), `mem` or `name`.
fn sort_key(params: &Value) -> &'static str {
    match params.get("sort").and_then(Value::as_str) {
        Some("mem") => "mem",
        Some("name") => "name",
        _ => "cpu",
    }
}

/// One subscriber's view of a sample (plan 006, "Stream apps"): groups
/// sorted by `sort`, the first `limit` of them (default 50), and the
/// processes only of the groups named in `expand` (`true` expands every
/// group). The System group is always last, outside the limit, so the
/// heaviest apps are never pushed out by it.
pub fn view(data: &Value, params: &Value) -> Value {
    let Ok(mut d) = serde_json::from_value::<AppsData>(data.clone()) else {
        return data.clone();
    };
    let sort = sort_key(params);
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_LIMIT, |l| l as usize);
    let expand_all = params.get("expand").and_then(Value::as_bool) == Some(true);
    let expand: HashSet<&str> = params
        .get("expand")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let cpu = |c: Option<f64>| c.unwrap_or(0.0);
    let (mut system, mut groups): (Vec<AppGroup>, Vec<AppGroup>) = d
        .groups
        .drain(..)
        .partition(|g| g.kind == Kind::System.as_str());
    let by_name = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b));
    groups.sort_by(|a, b| match sort {
        "mem" => b
            .rss_bytes
            .cmp(&a.rss_bytes)
            .then(by_name(&a.name, &b.name)),
        "name" => by_name(&a.name, &b.name).then(a.id.cmp(&b.id)),
        _ => cpu(b.cpu)
            .total_cmp(&cpu(a.cpu))
            .then(by_name(&a.name, &b.name)),
    });
    groups.truncate(limit);
    groups.append(&mut system);
    for g in &mut groups {
        if !(expand_all || expand.contains(g.id.as_str())) {
            g.processes = None;
        } else if let Some(ps) = &mut g.processes {
            ps.sort_by(|a, b| match sort {
                "mem" => b.rss_bytes.cmp(&a.rss_bytes).then(a.pid.cmp(&b.pid)),
                "name" => by_name(&a.name, &b.name).then(a.pid.cmp(&b.pid)),
                _ => cpu(b.cpu).total_cmp(&cpu(a.cpu)).then(a.pid.cmp(&b.pid)),
            });
        }
    }
    d.groups = groups;
    d.sort = sort.to_string();
    serde_json::to_value(d).unwrap_or(Value::Null)
}

impl Sampler for Apps {
    fn stream(&self) -> &'static str {
        APPS
    }

    fn probe(&mut self, root: &Root) -> Result<(), String> {
        root.read_dir("proc")
            .map_err(|e| strings::proc_unlisted(&e.to_string()))?;
        // The daemon's own uid through Root, so a fixture tree says whose
        // processes are "ours" (its proc/self/status), as the live machine
        // does.
        if let Some(uid) = root
            .read("proc/self/status")
            .ok()
            .and_then(|t| procfs::status(&t).uid)
        {
            self.own_uid = uid;
        }
        self.users = root
            .read("etc/passwd")
            .map(|t| procfs::passwd(&t))
            .unwrap_or_default();
        self.own_home = match self.users.iter().find(|u| u.uid == self.own_uid) {
            Some(u) => Some(u.home.clone()),
            None => std::env::var("HOME").ok(),
        };
        self.tck = selfcost::clk_tck(root);
        self.page = selfcost::page_size(root);
        self.cores = root
            .read("proc/stat")
            .ok()
            .map(|t| crate::samplers::system::cpu::stat(&t).cores.len() as u32)
            .filter(|n| *n > 0);
        self.desktop = Desktop::new(self.own_home.as_deref());
        // A test daemon on a fixture root may shrink the tag's window, the
        // `caller_for` pattern: never on the live machine.
        if !root.is_host()
            && std::env::var("MIRQAB_TEST").as_deref() == Ok("1")
            && let Some(ms) = std::env::var("MIRQAB_HEAVY_WINDOW_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|ms| *ms > 0)
        {
            self.thresholds.window = Duration::from_millis(ms);
        }
        Ok(())
    }

    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        // Only a subscriber's tick samples (a `get` takes a snapshot), so
        // the focus source runs only while the stream has subscribers.
        if self.focus.source().is_none() && self.focus_retry.is_none_or(|t| now >= t) {
            self.focus_retry = Some(now + FOCUS_RETRY);
            // No source is no tag and `focus` absent; nothing is logged
            // (a tty or another compositor is not a fault).
            let _ = self.focus.start(root, self.own_uid);
        }
        let procs = self.walk(root);
        let prev = self
            .live
            .take()
            .filter(|p| now.saturating_duration_since(p.at) <= LIVE_STALE);
        let groups = self.groups(&procs);
        let mut d = self.build(&procs, &groups, prev.as_ref(), now);
        if self.focus.source().is_some() {
            self.windows.record(
                now,
                d.groups.iter().map(|g| (g.id.as_str(), g.cpu)),
                self.thresholds.window,
            );
        } else {
            self.windows.clear();
        }
        self.tag(&mut d, &procs, &groups, now);
        let data = serde_json::to_value(&d).unwrap_or(Value::Null);
        self.live = Some(Tick::of(&procs, now));
        self.last = Some((now, d));
        data
    }

    /// The last subscriber left, or a `get` ended with none: the focus
    /// source stops (no thread, no socket while the window is closed), and
    /// the windows, the last frame, the subscriber baseline and the process
    /// cache (every process's command line) go, so a page opened later
    /// counts afresh (plan 024, L27; the minute walk, while `top-apps` is
    /// on, fills the cache again at its cost of one full read of each
    /// process). The primed readings
    /// are the pending gets', not the subscribers': they stay, or a get in
    /// its gap would find none and sleep under the lock (plan 023's
    /// follow-up). Its `snapshot` takes each, and drops any past 10 s.
    fn idle(&mut self) {
        self.focus.stop();
        self.focus_retry = None;
        self.windows.clear();
        self.last = None;
        self.live = None;
        self.cache = Cache::default();
    }

    /// `get`: the first reading, here under the lock, and half a second
    /// without it before `snapshot`. A young subscriber baseline needs no
    /// wait, and is queued as this get's reading: a tick between the get's
    /// two locked steps replaces it, and `snapshot` must not then wait
    /// holding the lock (plan 023's follow-up, as `process` in PR #26).
    /// Only one young enough to still serve half a second later is; an
    /// older one gets a fresh reading and the wait.
    fn prime(&mut self, root: &Root, now: Instant) -> Option<Duration> {
        let serves = self
            .live
            .as_ref()
            .filter(|t| {
                self.young(now) && now.saturating_duration_since(t.at) <= LIVE_STALE - SNAPSHOT_GAP
            })
            .cloned();
        let (base, gap) = match serves {
            Some(t) => (t, None),
            None => {
                let first = self.walk(root);
                (Tick::of(&first, Instant::now()), Some(SNAPSHOT_GAP))
            }
        };
        self.get_base.push_back(base);
        while self.get_base.len() > MAX_PRIMED {
            self.get_base.pop_front();
        }
        gap
    }

    /// `get`: against a subscriber's baseline when it is 1–10 s old,
    /// without moving it; otherwise against the oldest reading `prime`
    /// queued that is 250 ms to 10 s old (`take_mine`); otherwise against
    /// a second reading of its own, half a second later. A reading is
    /// taken whichever answers, so none is left over.
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        let mine = self.take_mine(now);
        if self.young(now) {
            let procs = self.walk(root);
            let live = self.live.clone();
            return self.data(&procs, live.as_ref(), now);
        }
        if let Some(base) = mine {
            let procs = self.walk(root);
            return self.data(&procs, Some(&base), Instant::now());
        }
        // Both readings are stamped when they were taken, so the rate is
        // over the time that really passed (review of PR #16).
        let first = self.walk(root);
        let base = Tick::of(&first, Instant::now());
        std::thread::sleep(SNAPSHOT_GAP);
        let procs = self.walk(root);
        self.data(&procs, Some(&base), Instant::now())
    }

    fn view(&self) -> Option<View> {
        Some(view)
    }

    fn query(&mut self, root: &Root, name: &str, params: &Value) -> Option<Answer> {
        if name != "inspect" {
            return None;
        }
        let bad = |m: &str| Some(Answer::Ready(Err((Code::BadRequest, m.to_string()))));
        let Some(pid) = params
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok())
        else {
            return bad(strings::APPS_PID_REQUIRED);
        };
        let start = match params.get("start") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64() {
                Some(s) => Some(s),
                None => return bad(strings::APPS_START_NUMBER),
            },
        };
        let group = self.group_of(root, pid);
        let ctx = inspect::Ctx {
            own_uid: self.own_uid,
            tck: self.tck,
            page: self.page,
            users: &self.users,
            own_home: self.own_home.as_deref(),
            group,
            start,
        };
        let to_value = |i: &Inspect| serde_json::to_value(i).unwrap_or(Value::Null);
        Some(match inspect::read(root, pid, &ctx) {
            Err(e) => Answer::Ready(Err(e)),
            Ok(inspect::Read::Done(i)) => Answer::Ready(Ok(to_value(&i))),
            Ok(inspect::Read::Owner(p)) => {
                // pacman, up to 5 s a call, after the server let go of
                // this sampler's lock. The `Owners` lock is taken only for
                // that call, after `finish` has checked the host's file.
                let owners = Arc::clone(&self.owners);
                Answer::Later(Box::new(move || {
                    let i = inspect::finish(*p, |exe| lock(&owners).owner(exe));
                    Ok(to_value(&i))
                }))
            }
        })
    }

    /// The minute's `top-apps` row (plan 006, part 4, decision 7, and
    /// `top_apps`): from the last sample while subscribed, else from a walk differenced
    /// against the previous minute's (the first minute writes nothing).
    /// Written only when the CPU leader used at least 10 % of one core and
    /// the six names differ from the last row's (owner, 2026-11-14, Q4).
    /// `trends.topApps: false` skips the walk too.
    fn minute_events(&mut self, root: &Root, config: &Value) -> Vec<(&'static str, Value)> {
        self.top_apps(root, config, Instant::now())
    }

    fn status(&self) -> Vec<(&'static str, Value)> {
        let focus = match self.focus.source() {
            Some(s) => json!({ "source": s }),
            None => Value::Null,
        };
        vec![("focus", focus)]
    }
}
