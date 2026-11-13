//! CPU from `proc/stat` (per core and in all), clocks and governor from
//! cpufreq, core kinds and topology from sysfs, load from `proc/loadavg`
//! (plan 002).

use crate::proto::stream::{Core, Load};
use crate::root::Root;

/// One `cpu` line of `proc/stat`, in jiffies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Jiffies {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl Jiffies {
    fn parse(fields: &[u64]) -> Option<Jiffies> {
        if fields.len() < 4 {
            return None;
        }
        let f = |i: usize| fields.get(i).copied().unwrap_or(0);
        Some(Jiffies {
            user: f(0),
            nice: f(1),
            system: f(2),
            idle: f(3),
            iowait: f(4),
            irq: f(5),
            softirq: f(6),
            steal: f(7),
        })
    }

    /// Guest time is already inside user and nice, so it is not added.
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }
}

/// The shares of one interval, in percent, each clamped to 0..=100.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shares {
    pub busy: f64,
    pub user: f64,
    pub system: f64,
    pub iowait: f64,
}

/// Busy is everything but idle and iowait: a stuck disk shows as iowait,
/// not as work (plan 002). None when no time passed or a counter went back.
pub fn shares(prev: &Jiffies, now: &Jiffies) -> Option<Shares> {
    let total = now.total().checked_sub(prev.total())?;
    if total == 0 {
        return None;
    }
    let d = |a: u64, b: u64| a.saturating_sub(b) as f64;
    let pct = |x: f64| (x * 100.0 / total as f64).clamp(0.0, 100.0);
    let idle = d(now.idle, prev.idle) + d(now.iowait, prev.iowait);
    Some(Shares {
        busy: pct(total as f64 - idle),
        user: pct(d(now.user, prev.user) + d(now.nice, prev.nice)),
        system: pct(d(now.system, prev.system)
            + d(now.irq, prev.irq)
            + d(now.softirq, prev.softirq)),
        iowait: pct(d(now.iowait, prev.iowait)),
    })
}

/// The aggregate line and every `cpuN` line, and `btime`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stat {
    pub all: Option<Jiffies>,
    pub cores: Vec<(u32, Jiffies)>,
    pub btime: Option<i64>,
}

pub fn stat(text: &str) -> Stat {
    let mut out = Stat::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        if key == "btime" {
            out.btime = it.next().and_then(|v| v.parse().ok());
            continue;
        }
        let Some(rest) = key.strip_prefix("cpu") else {
            continue;
        };
        let fields: Vec<u64> = it.filter_map(|f| f.parse().ok()).collect();
        let Some(j) = Jiffies::parse(&fields) else {
            continue;
        };
        if rest.is_empty() {
            out.all = Some(j);
        } else if let Ok(n) = rest.parse() {
            out.cores.push((n, j));
        }
    }
    out
}

pub fn loadavg(text: &str) -> Option<Load> {
    let f: Vec<&str> = text.split_whitespace().collect();
    let (run, tasks) = f.get(3)?.split_once('/')?;
    Some(Load {
        one: f.first()?.parse().ok()?,
        five: f.get(1)?.parse().ok()?,
        fifteen: f.get(2)?.parse().ok()?,
        runnable: run.parse().ok()?,
        tasks: tasks.parse().ok()?,
    })
}

/// A kernel cpu list (`0-3,8,10-11`) as numbers.
pub fn cpulist(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    out.extend(a..=b.min(a.saturating_add(4095)));
                }
            }
            None => out.extend(part.parse::<u32>().ok()),
        }
    }
    out
}

fn read_u64(root: &Root, rel: &str) -> Option<u64> {
    root.read(rel).ok()?.trim().parse().ok()
}

fn read_word(root: &Root, rel: &str) -> Option<String> {
    let s = root.read(rel).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Per core: its clock in MHz and its kind. Kinds come from Intel's hybrid
/// PMU lists (`cpu_core` is `p`, `cpu_atom` is `e`) or, on ARM, from
/// `cpu_capacity` (the largest is `p`, the rest `e`); one kind of core gets
/// no kind at all.
pub fn core_details(root: &Root, ids: &[u32]) -> Vec<(u32, Option<u32>, Option<String>)> {
    let p_list = root
        .read("sys/devices/cpu_core/cpus")
        .ok()
        .map(|t| cpulist(&t));
    let e_list = root
        .read("sys/devices/cpu_atom/cpus")
        .ok()
        .map(|t| cpulist(&t));
    let capacity: Vec<Option<u64>> = ids
        .iter()
        .map(|n| read_u64(root, &format!("sys/devices/system/cpu/cpu{n}/cpu_capacity")))
        .collect();
    let max_cap = capacity.iter().flatten().max().copied();
    let caps_differ = capacity.iter().flatten().any(|c| Some(*c) != max_cap);
    ids.iter()
        .zip(capacity)
        .map(|(&n, cap)| {
            let mhz = read_u64(
                root,
                &format!("sys/devices/system/cpu/cpu{n}/cpufreq/scaling_cur_freq"),
            )
            .map(|khz| (khz / 1000) as u32);
            let kind = match (&p_list, &e_list) {
                (Some(p), Some(e)) if p.contains(&n) => Some("p"),
                (Some(_), Some(e)) if e.contains(&n) => Some("e"),
                _ if caps_differ => cap.map(|c| if Some(c) == max_cap { "p" } else { "e" }),
                _ => None,
            };
            (n, mhz, kind.map(str::to_string))
        })
        .collect()
}

/// The highest `cpuinfo_max_freq` of any core, in MHz.
pub fn mhz_max(root: &Root, ids: &[u32]) -> Option<u32> {
    ids.iter()
        .filter_map(|n| {
            read_u64(
                root,
                &format!("sys/devices/system/cpu/cpu{n}/cpufreq/cpuinfo_max_freq"),
            )
        })
        .max()
        .map(|khz| (khz / 1000) as u32)
}

pub fn governor(root: &Root) -> (Option<String>, Option<String>) {
    (
        read_word(root, "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        read_word(
            root,
            "sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
        ),
    )
}

/// Physical cores: distinct (package, core) pairs from topology.
pub fn physical(root: &Root, ids: &[u32]) -> Option<u32> {
    let mut seen = std::collections::BTreeSet::new();
    for n in ids {
        let base = format!("sys/devices/system/cpu/cpu{n}/topology");
        let pkg = read_u64(root, &format!("{base}/physical_package_id"))?;
        let core = read_u64(root, &format!("{base}/core_id"))?;
        seen.insert((pkg, core));
    }
    (!seen.is_empty()).then_some(seen.len() as u32)
}

pub fn model(cpuinfo: &str) -> Option<String> {
    crate::selfcost::cpu_model(cpuinfo)
}

/// The cores of a sample: per-core shares when a previous reading exists.
pub fn cores(
    now: &Stat,
    prev: Option<&Stat>,
    details: &[(u32, Option<u32>, Option<String>)],
) -> Vec<Core> {
    now.cores
        .iter()
        .map(|(id, j)| {
            let percent = prev
                .and_then(|p| p.cores.iter().find(|(pid, _)| pid == id))
                .and_then(|(_, pj)| shares(pj, j))
                .map(|s| round1(s.busy));
            let (mhz, kind) = details
                .iter()
                .find(|(n, _, _)| n == id)
                .map(|(_, m, k)| (*m, k.clone()))
                .unwrap_or((None, None));
            Core {
                id: *id,
                percent,
                mhz,
                kind,
            }
        })
        .collect()
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
