//! The `apps` stream (plan 006): every process of `proc/`, grouped into
//! apps, with CPU from the difference between two readings, an estimate of
//! memory, and storage rates. Every 2 s while subscribed, also while the
//! window is open: the list is a table, not a live graph, and this walk is
//! the costliest sampler, so the 1 s rule does not apply (architecture
//! §5.3). `get apps.inspect {pid}` explains one process (`inspect.rs`).
//!
//! One sample serves every subscriber; `view` gives each its own order,
//! length and expanded groups from its `params` (`sort`, `limit`,
//! `expand`).

pub mod desktop;
pub mod group;
pub mod inspect;
pub mod procfs;

use crate::proto::stream::{APPS, AppGroup, AppProcess, AppTop, AppsData, AppsTotals, GroupRef};
use crate::proto::{Code, strings};
use crate::root::Root;
use crate::sampler::{QueryResult, Sampler, View};
use crate::selfcost;
use desktop::Desktop;
use group::Kind;
use inspect::Owners;
use procfs::{Cache, Proc, User};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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

/// One process's counters: CPU ticks, and storage bytes read and written
/// when the daemon may read them.
type Counters = (u64, Option<(u64, u64)>);

/// Counters of one walk, by pid and start time.
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
    owners: Owners,
    own_uid: u32,
    users: Vec<User>,
    own_home: Option<String>,
    tck: u64,
    page: u64,
    cores: Option<u32>,
    live: Option<Tick>,
}

impl Default for Apps {
    fn default() -> Self {
        Self::new()
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
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
            owners: Owners::new(program),
            own_uid: crate::sys::uid(),
            users: Vec::new(),
            own_home: None,
            tck: 100,
            page: 4096,
            cores: None,
            live: None,
        }
    }

    fn walk(&mut self, root: &Root) -> Vec<Proc> {
        self.desktop.refresh(root);
        procfs::walk(root, &mut self.cache, self.own_uid)
    }

    /// Every group with every member; `view` trims it per subscriber.
    fn build(&self, procs: &[Proc], prev: Option<&Tick>, at: Instant) -> AppsData {
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
        let groups = group::group(procs, self.own_uid, &self.desktop);
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
                });
            }
            processes.sort_by_key(|p| p.pid);
            all_procs += g.members.len() as u32;
            all_threads += threads;
            if let Some(c) = cpu {
                all_cpu = Some(all_cpu.unwrap_or(0.0) + c);
            }
            out.push(AppGroup {
                id: g.id,
                name: g.name,
                icon: g.icon,
                kind: g.kind.as_str().to_string(),
                unit: g.unit,
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
            });
        }
        // Counted before any view's limit, so a reader can tell a cut list
        // (review of PR #16).
        let groups = out.len() as u32;
        AppsData {
            groups: out,
            totals: AppsTotals {
                groups,
                procs: all_procs,
                threads: all_threads,
                cpu: all_cpu.map(round1),
                cores: self.cores,
            },
            sort: "cpu".to_string(),
        }
    }

    fn data(&self, procs: &[Proc], prev: Option<&Tick>, at: Instant) -> Value {
        serde_json::to_value(self.build(procs, prev, at)).unwrap_or(Value::Null)
    }

    /// The group `pid` is counted in, from a fresh walk.
    fn group_of(&mut self, root: &Root, pid: u32) -> Option<GroupRef> {
        let procs = self.walk(root);
        let i = procs.iter().position(|p| p.pid() == pid)?;
        group::group(&procs, self.own_uid, &self.desktop)
            .into_iter()
            .find(|g| g.members.contains(&i))
            .map(|g| GroupRef {
                id: g.id,
                name: g.name,
            })
    }
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
            .map_err(|e| format!("proc cannot be listed: {e}"))?;
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
        Ok(())
    }

    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        let procs = self.walk(root);
        let prev = self
            .live
            .take()
            .filter(|p| now.saturating_duration_since(p.at) <= LIVE_STALE);
        let data = self.data(&procs, prev.as_ref(), now);
        self.live = Some(Tick::of(&procs, now));
        data
    }

    /// `get`: against a subscriber's baseline when it is 1–10 s old,
    /// without moving it; otherwise against a second reading of its own.
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        let young = self.live.as_ref().is_some_and(|t| {
            (BASELINE_MIN..=LIVE_STALE).contains(&now.saturating_duration_since(t.at))
        });
        if young {
            let procs = self.walk(root);
            return self.data(&procs, self.live.as_ref(), now);
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

    fn query(&mut self, root: &Root, name: &str, params: &Value) -> Option<QueryResult> {
        if name != "inspect" {
            return None;
        }
        let Some(pid) = params
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok())
        else {
            return Some(Err((
                Code::BadRequest,
                strings::APPS_PID_REQUIRED.to_string(),
            )));
        };
        let group = self.group_of(root, pid);
        let ctx = inspect::Ctx {
            own_uid: self.own_uid,
            tck: self.tck,
            page: self.page,
            users: &self.users,
            own_home: self.own_home.as_deref(),
            group,
        };
        Some(
            inspect::inspect(root, pid, &ctx, &mut self.owners)
                .map(|i| serde_json::to_value(i).unwrap_or(Value::Null)),
        )
    }
}
