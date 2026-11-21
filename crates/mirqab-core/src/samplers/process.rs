//! The `process` stream (plan 006, part 3): the Apps inspector's live graph
//! of one process. Each tick reads `proc/<pid>/stat` and `proc/<pid>/statm`
//! for each distinct pid its subscribers name, `PROCESS_MAX_PIDS` at most,
//! and nothing else: both files are world-readable, so another user's
//! process can be graphed as the System group already shows its figures.
//! It takes no action.
//!
//! `live()` is true, so a graph on screen may ask for 1 s (architecture
//! §5.3): two small reads a second, where the `apps` walk reads every
//! process. The stream is its own sampler, so its lock is its own and a
//! reading never waits behind the `apps` walk, `get apps`' second reading
//! or the inspector's `pacman`.
//!
//! A process is its pid and start time, as in the `apps` sampler: the CPU
//! baseline is keyed by both, and a subscriber that names `start` (or,
//! without it, the process first seen at that pid) gets `gone` once the pid
//! holds another process or none. The pins and the baseline belong to the
//! subscribers: they are dropped once the stream had none (`idle`), and a
//! `get` reads the process at the pid now.

use crate::proto::stream::{PROCESS, PROCESS_MAX_PIDS, ProcessData};
use crate::proto::strings;
use crate::root::Root;
use crate::sampler::{Accepts, Sampler, View};
use crate::samplers::apps::{MAX_PRIMED, PRIMED_MIN, procfs};
use crate::selfcost;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// A baseline older than this is dropped, as in `apps` and `system`.
const LIVE_STALE: Duration = Duration::from_secs(10);
/// A baseline younger than this gives no useful rate.
const BASELINE_MIN: Duration = Duration::from_secs(1);
/// How long `get` waits for its own second reading when no subscriber's
/// baseline fits.
const SNAPSHOT_GAP: Duration = Duration::from_millis(500);

/// What a subscriber asks for: the pid, and the start time it knows the
/// process by, when it says.
fn asked(params: &Value) -> Option<(u32, Option<u64>)> {
    let pid = params
        .get("pid")?
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p > 0)?;
    let start = match params.get("start") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64()?),
    };
    Some((pid, start))
}

/// The params the stream takes: `pid` (a process id) and an optional
/// `start`, nothing else. A subscribe or `get` without them is
/// `bad-request`.
pub fn accepts(params: &Value) -> Result<(), String> {
    let only_known = params
        .as_object()
        .is_some_and(|o| o.keys().all(|k| k == "pid" || k == "start"));
    if only_known && asked(params).is_some() {
        Ok(())
    } else {
        Err(strings::PROCESS_PARAMS.to_string())
    }
}

/// The process first seen at a watched pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    Start(u64),
    /// There was none: whatever takes the pid later is not the one asked
    /// for.
    Missing,
}

/// One process now: `stat` and `statm`.
struct Reading {
    stat: procfs::Stat,
    resident: Option<u64>,
}

fn read(root: &Root, pid: u32) -> Option<Reading> {
    let stat = procfs::read_text(root, &format!("proc/{pid}/stat"))
        .as_deref()
        .and_then(procfs::stat)
        .filter(|s| s.pid == pid)?;
    let resident = root
        .read(&format!("proc/{pid}/statm"))
        .ok()
        .as_deref()
        .and_then(procfs::statm)
        .map(|m| m.resident);
    Some(Reading { stat, resident })
}

/// CPU ticks at one moment, by pid and start time.
struct Base {
    at: Instant,
    ticks: HashMap<(u32, u64), u64>,
}

impl Base {
    fn of(readings: &[(u32, Reading)], at: Instant) -> Base {
        Base {
            at,
            ticks: readings
                .iter()
                .map(|(pid, r)| ((*pid, r.stat.start), r.stat.ticks))
                .collect(),
        }
    }
}

/// Whether the subscribers' baseline can serve a `get` of `first`: 1–10 s
/// old and holding every process read.
fn usable(base: Option<&Base>, first: &[(u32, Reading)], now: Instant) -> bool {
    base.is_some_and(|b| {
        (BASELINE_MIN..=LIVE_STALE).contains(&now.saturating_duration_since(b.at))
            && first
                .iter()
                .all(|(pid, r)| b.ticks.contains_key(&(*pid, r.stat.start)))
    })
}

/// A first reading `prime` took for one `get`, with the pids that get
/// asked for: a reading covers only those, so it serves no other get
/// (audit: a leftover answered the next get of another pid with no rate).
struct Primed {
    pids: Vec<u32>,
    base: Base,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub struct Process {
    tck: u64,
    page: u64,
    /// The pids read each tick, in the order subscribers asked.
    want: Vec<u32>,
    /// Asked for beyond `PROCESS_MAX_PIDS`: not read.
    over: Vec<u32>,
    pins: HashMap<u32, Pin>,
    base: Option<Base>,
    /// The first readings `prime` took, one per pending `get`, oldest
    /// first (review of PR #20, S1).
    get_base: VecDeque<Primed>,
}

impl Default for Process {
    fn default() -> Self {
        Self::new()
    }
}

impl Process {
    pub fn new() -> Self {
        Process {
            tck: 100,
            page: 4096,
            want: Vec::new(),
            over: Vec::new(),
            pins: HashMap::new(),
            base: None,
            get_base: VecDeque::new(),
        }
    }

    /// Primed readings queued (tests).
    pub fn primed(&self) -> usize {
        self.get_base.len()
    }

    /// Takes this get's primed reading: the oldest queued for the pids it
    /// asks for. Readings past `LIVE_STALE` are dropped on the way; other
    /// gets' readings stay queued for them.
    fn take_mine(&mut self, now: Instant) -> Option<Base> {
        self.get_base
            .retain(|p| now.saturating_duration_since(p.base.at) <= LIVE_STALE);
        let at = self.get_base.iter().position(|p| p.pids == self.want)?;
        self.get_base.remove(at).map(|p| p.base)
    }

    fn read_all(&self, root: &Root) -> Vec<(u32, Reading)> {
        self.want
            .iter()
            .filter_map(|&pid| read(root, pid).map(|r| (pid, r)))
            .collect()
    }

    /// The sample every view is cut from: one entry per pid read, with the
    /// start time first seen there (`first`, absent when the pid was empty
    /// then), and the pids asked for beyond the cap. With `pinned` false (a
    /// `get`) `first` is the process read now: a `get` never reads the
    /// subscribers' pins (review of PR #18).
    fn data(
        &self,
        readings: &[(u32, Reading)],
        base: Option<&Base>,
        at: Instant,
        pinned: bool,
    ) -> Value {
        let secs = base
            .map(|b| at.saturating_duration_since(b.at).as_secs_f64())
            .filter(|s| *s > 0.0);
        let mut out = serde_json::Map::new();
        for (pid, r) in readings {
            let cpu = match (base, secs) {
                (Some(b), Some(secs)) => b.ticks.get(&(*pid, r.stat.start)).map(|t0| {
                    round1(r.stat.ticks.saturating_sub(*t0) as f64 / self.tck as f64 / secs * 100.0)
                }),
                _ => None,
            };
            let first = match self.pins.get(pid).filter(|_| pinned) {
                Some(Pin::Start(s)) => Some(*s),
                Some(Pin::Missing) => None,
                // A `get`, or a pid no subscriber watches: the process now.
                None => Some(r.stat.start),
            };
            let reading = ProcessData {
                pid: *pid,
                gone: false,
                capped: false,
                start: Some(r.stat.start),
                name: Some(r.stat.comm.clone()),
                cpu,
                rss_bytes: r.resident.map(|p| p * self.page),
                threads: Some(r.stat.threads),
                state: Some(r.stat.state.to_string()),
            };
            out.insert(pid.to_string(), json!({"reading": reading, "first": first}));
        }
        json!({"pids": out, "capped": self.over})
    }
}

/// One subscriber's view: its pid's reading, `gone` when the pid holds no
/// process or another one than asked for, `capped` when it was not read.
pub fn view(data: &Value, params: &Value) -> Value {
    let Some((pid, start)) = asked(params) else {
        return Value::Null;
    };
    let gone = json!({"pid": pid, "gone": true});
    if data["capped"]
        .as_array()
        .is_some_and(|c| c.iter().any(|p| p.as_u64() == Some(u64::from(pid))))
    {
        return json!({"pid": pid, "capped": true});
    }
    let Some(entry) = data["pids"].get(pid.to_string()) else {
        return gone;
    };
    let expected = start.or_else(|| entry["first"].as_u64());
    if expected.is_some() && entry["reading"]["start"].as_u64() == expected {
        entry["reading"].clone()
    } else {
        gone
    }
}

impl Sampler for Process {
    fn stream(&self) -> &'static str {
        PROCESS
    }

    fn live(&self) -> bool {
        true
    }

    fn probe(&mut self, root: &Root) -> Result<(), String> {
        root.read_dir("proc")
            .map_err(|e| format!("proc cannot be listed: {e}"))?;
        self.tck = selfcost::clk_tck(root);
        self.page = selfcost::page_size(root);
        Ok(())
    }

    fn wants(&mut self, params: &[Value]) {
        let mut pids: Vec<u32> = Vec::new();
        for (pid, _) in params.iter().filter_map(asked) {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
        self.over = pids.split_off(pids.len().min(PROCESS_MAX_PIDS));
        self.want = pids;
    }

    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        let readings = self.read_all(root);
        for &pid in &self.want {
            let found = readings.iter().find(|(p, _)| *p == pid);
            self.pins.entry(pid).or_insert(match found {
                Some((_, r)) => Pin::Start(r.stat.start),
                None => Pin::Missing,
            });
        }
        let want = &self.want;
        self.pins.retain(|pid, _| want.contains(pid));
        let base = self
            .base
            .take()
            .filter(|b| now.saturating_duration_since(b.at) <= LIVE_STALE);
        let data = self.data(&readings, base.as_ref(), now, true);
        self.base = Some(Base::of(&readings, now));
        data
    }

    /// No subscriber since the last sample: the pins and the baseline were
    /// theirs (review of PR #18).
    fn idle(&mut self) {
        self.pins.clear();
        self.base = None;
        self.get_base.clear();
    }

    /// `get`: the first reading, under the lock; half a second without it
    /// before `snapshot` (plan 006, part 4, decision 10). Nothing read, or
    /// a subscriber's baseline that serves, needs no wait (K7 of PR #18: a
    /// missing pid answers at once). A baseline that serves is queued as
    /// this get's reading too: a tick between the get's two locked steps
    /// replaces it, and `snapshot` must not then wait holding the lock
    /// (audit).
    fn prime(&mut self, root: &Root, now: Instant) -> Option<Duration> {
        let first = self.read_all(root);
        if first.is_empty() {
            return None;
        }
        let (base, gap) = match self.base.as_ref() {
            Some(b) if usable(Some(b), &first, now) => {
                let ticks = first
                    .iter()
                    .filter_map(|(pid, r)| {
                        let key = (*pid, r.stat.start);
                        b.ticks.get(&key).map(|t| (key, *t))
                    })
                    .collect();
                (Base { at: b.at, ticks }, None)
            }
            _ => (Base::of(&first, Instant::now()), Some(SNAPSHOT_GAP)),
        };
        self.get_base.push_back(Primed {
            pids: self.want.clone(),
            base,
        });
        while self.get_base.len() > MAX_PRIMED {
            self.get_base.pop_front();
        }
        gap
    }

    /// `get`: against a subscriber's baseline when it is 1–10 s old and
    /// holds the process, without moving it, and never reading the pins;
    /// otherwise against the oldest reading `prime` queued for these pids
    /// when it is 250 ms to 10 s old and holds the process, or a second
    /// reading of its own half a second later. Nothing read is answered at
    /// once. This get's primed reading is taken whichever answers, so none
    /// is left for another get.
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        let mine = self.take_mine(now);
        let first = self.read_all(root);
        if first.is_empty() {
            return self.data(&first, None, now, false);
        }
        if usable(self.base.as_ref(), &first, now) {
            return self.data(&first, self.base.as_ref(), now, false);
        }
        if let Some(base) = mine.filter(|b| {
            now.saturating_duration_since(b.at) >= PRIMED_MIN
                && first
                    .iter()
                    .all(|(pid, r)| b.ticks.contains_key(&(*pid, r.stat.start)))
        }) {
            return self.data(&first, Some(&base), Instant::now(), false);
        }
        let base = Base::of(&first, Instant::now());
        std::thread::sleep(SNAPSHOT_GAP);
        let readings = self.read_all(root);
        self.data(&readings, Some(&base), Instant::now(), false)
    }

    fn narrows(&self) -> bool {
        true
    }

    fn view(&self) -> Option<View> {
        Some(view)
    }

    fn accepts(&self) -> Option<Accepts> {
        Some(accepts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_a_pid_and_an_optional_start() {
        assert!(accepts(&json!({"pid": 2001})).is_ok());
        assert!(accepts(&json!({"pid": 2001, "start": 5010})).is_ok());
        assert!(accepts(&json!({"pid": 2001, "start": null})).is_ok());
        for bad in [
            Value::Null,
            json!({}),
            json!({"pid": 0}),
            json!({"pid": "2001"}),
            json!({"pid": -1}),
            json!({"pid": 4_294_967_296_u64}),
            json!({"pid": 2001, "start": "x"}),
            json!({"pid": 2001, "sort": "cpu"}),
        ] {
            assert!(accepts(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn at_most_eight_pids_are_read() {
        let mut p = Process::new();
        let params: Vec<Value> = (1..=10).map(|n| json!({"pid": n})).collect();
        p.wants(&params);
        assert_eq!(p.want, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(p.over, [9, 10]);
        let data = json!({"pids": {}, "capped": p.over});
        assert_eq!(
            view(&data, &json!({"pid": 9})),
            json!({"pid": 9, "capped": true})
        );
        assert_eq!(
            view(&data, &json!({"pid": 3})),
            json!({"pid": 3, "gone": true})
        );
    }
}
