//! The `system` stream (plan 002): CPU, load, memory, pressure,
//! temperatures, GPUs and power, every part optional. Written new against
//! `Root`, after haseen's sysusage sampler (`core/internal/sysusage`, MIT,
//! Copyright (c) 2026 t1nk33r; NOTICE.md), whose lesson about suspended
//! GPUs it keeps.
//!
//! Three baselines, so no reader disturbs another: the live one (events),
//! the minute one (history, and `get`, which reads against it without
//! moving it), and the means of live samples taken during the current
//! minute, which a gauge's minute stores instead of a point sample
//! (architecture §6.3).

pub mod cpu;
pub mod gpu;
pub mod mem;
pub mod power;
pub mod pressure;
pub mod temps;

use crate::history::SERIES;
use crate::proto::stream::{self, Advice, Cpu, Mem, Power, SystemData};
use crate::root::Root;
use crate::sampler::Sampler;
use cpu::Stat;
use gpu::NvidiaRow;
use mem::VmStat;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A minute baseline older than this means the machine slept or the daemon
/// stalled: that minute stores no rate (architecture §6.3).
const MAX_MINUTE_GAP_SECS: u64 = 90;
/// A live baseline older than this is dropped: the first sample after a
/// quiet spell would otherwise report the average of the whole spell
/// (review of plan 001 phase C).
const LIVE_STALE: Duration = Duration::from_secs(10);
/// `nvidia-smi` is a process spawn; it runs at most this often, so a 1 s
/// live stream repeats the GPU fields every other event (plan 002).
const NVIDIA_EVERY: Duration = Duration::from_secs(2);
/// After a failed or empty `nvidia-smi` (no driver: nouveau, vfio), wait
/// this long before the next try (review of PR #7).
const NVIDIA_RETRY: Duration = Duration::from_secs(60);
/// A baseline younger than this gives no useful rate; older than the
/// minute gap, a stale one (review of PR #7).
const BASELINE_MIN: Duration = Duration::from_secs(1);
/// How long `get` waits for a second reading when no baseline fits.
const SNAPSHOT_GAP: Duration = Duration::from_millis(200);
/// Restart advice (plan 002): uptime over 30 days, or over 7 with swap more
/// than half full or memory pressure above 10 % for the last hour.
const ADVICE_UPTIME: u64 = 30 * 86_400;
const ADVICE_UPTIME_STRAINED: u64 = 7 * 86_400;
const ADVICE_PSI: f64 = 10.0;

/// The counters a rate is computed from, read at one moment.
#[derive(Debug, Clone)]
struct Reading {
    at: Instant,
    stat: Stat,
    vm: Option<VmStat>,
    energy: Option<(u64, u64)>,
    throttle: Option<u64>,
}

impl Reading {
    fn take(root: &Root, at: Instant, cpu_ids: &[u32]) -> Reading {
        Reading {
            at,
            stat: root
                .read("proc/stat")
                .map(|t| cpu::stat(&t))
                .unwrap_or_default(),
            vm: root
                .read("proc/vmstat")
                .ok()
                .as_deref()
                .and_then(mem::vmstat),
            energy: power::energy(root),
            throttle: temps::throttle_count(root, cpu_ids),
        }
    }
}

#[derive(Default)]
pub struct System {
    live: Option<Reading>,
    /// The previous minute's reading and its wall time.
    minute_prev: Option<(Reading, u64)>,
    /// Gauge sums and counts from live samples during this minute.
    acc: BTreeMap<&'static str, (f64, u32)>,
    /// Memory `some60` of the last 60 minutes, for the advice.
    psi_hour: VecDeque<f64>,
    nvidia: Option<(Instant, Vec<NvidiaRow>)>,
    has_nvidia: bool,
    cpu_ids: Vec<u32>,
    model: Option<String>,
    physical: Option<u32>,
    mhz_max: Option<u32>,
}

/// `key`'s scale from the series list, and the key as a `'static` str.
fn scaled(key: &str, v: f64) -> Option<(&'static str, i64)> {
    let (k, _, scale) = SERIES.iter().find(|s| s.0 == key)?;
    Some((k, (v * *scale as f64).round() as i64))
}

fn wall_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether `nvidia-smi` should run again, given when it last ran and
/// whether that found nothing.
fn nvidia_due(last: Option<(Instant, bool)>, now: Instant) -> bool {
    match last {
        None => true,
        Some((at, empty)) => {
            now.saturating_duration_since(at) >= if empty { NVIDIA_RETRY } else { NVIDIA_EVERY }
        }
    }
}

fn hottest(data: &SystemData, kind: &str) -> Option<f64> {
    data.temps
        .iter()
        .filter(|t| t.kind == kind)
        .map(|t| t.celsius)
        .max_by(f64::total_cmp)
}

impl System {
    pub fn new() -> Self {
        Self::default()
    }

    fn nvidia_rows(&mut self, root: &Root, now: Instant) -> Vec<NvidiaRow> {
        if !self.has_nvidia || !gpu::nvidia_wanted(root) {
            return Vec::new();
        }
        let last = self
            .nvidia
            .as_ref()
            .map(|(at, rows)| (*at, rows.is_empty()));
        match &self.nvidia {
            Some((_, rows)) if !nvidia_due(last, now) => rows.clone(),
            _ => {
                let rows = gpu::query_nvidia();
                self.nvidia = Some((now, rows.clone()));
                rows
            }
        }
    }

    /// One snapshot of everything, with rates against `prev` when given.
    fn build(&mut self, root: &Root, cur: &Reading, prev: Option<&Reading>) -> SystemData {
        let secs = prev.map_or(0.0, |p| cur.at.duration_since(p.at).as_secs_f64());
        let shares = match (prev.and_then(|p| p.stat.all), cur.stat.all) {
            (Some(p), Some(n)) => cpu::shares(&p, &n),
            _ => None,
        };
        let details = cpu::core_details(root, &self.cpu_ids);
        let mhz: Vec<u32> = details.iter().filter_map(|d| d.1).collect();
        let mhz_avg = (!mhz.is_empty())
            .then(|| (mhz.iter().map(|m| u64::from(*m)).sum::<u64>() / mhz.len() as u64) as u32);
        let (governor, epp) = cpu::governor(root);
        let cpu = cur.stat.all.map(|_| Cpu {
            percent: shares.map(|s| cpu::round1(s.busy)),
            user: shares.map(|s| cpu::round1(s.user)),
            system: shares.map(|s| cpu::round1(s.system)),
            iowait: shares.map(|s| cpu::round1(s.iowait)),
            cores: cpu::cores(&cur.stat, prev.map(|p| &p.stat), &details),
            mhz_avg,
            mhz_max: self.mhz_max,
            governor,
            epp,
            model: self.model.clone(),
            physical: self.physical,
            logical: (!self.cpu_ids.is_empty()).then_some(self.cpu_ids.len() as u32),
        });
        let mi = root
            .read("proc/meminfo")
            .map(|t| mem::meminfo(&t))
            .unwrap_or_default();
        let rates = match (prev.and_then(|p| p.vm), cur.vm) {
            (Some(p), Some(n)) => mem::rates(&p, &n, secs),
            _ => None,
        };
        let memv = (mi.total > 0).then(|| Mem {
            total_bytes: mi.total,
            available_bytes: mi.available,
            used_bytes: mi.used(),
            cache_bytes: mi.cache(),
            shmem_bytes: mi.shmem,
            swap_total_bytes: mi.swap_total,
            swap_used_bytes: mi.swap_used(),
            zram: mem::zram(root),
            page_in_per_sec: rates.map(|r| cpu::round1(r[0])),
            page_out_per_sec: rates.map(|r| cpu::round1(r[1])),
            swap_in_per_sec: rates.map(|r| cpu::round1(r[2])),
            swap_out_per_sec: rates.map(|r| cpu::round1(r[3])),
            oom_kills: cur.vm.and_then(|v| v.oom_kill),
        });
        let temps = temps::read(root);
        let throttling = match (prev.and_then(|p| p.throttle), cur.throttle) {
            (Some(p), Some(n)) => n > p,
            (_, Some(_)) => false,
            _ => temps::throttling_by_clock(&temps, mhz_avg, self.mhz_max),
        };
        let thermal = temps::thermal(&temps, throttling);
        let nvidia = self.nvidia_rows(root, cur.at);
        let gpus = gpu::read(root, &nvidia);
        let package = match (prev.and_then(|p| p.energy), cur.energy) {
            (Some((p, _)), Some((n, max))) => power::watts(p, n, max, secs).map(cpu::round1),
            _ => None,
        };
        let battery = power::battery(root).map(cpu::round1);
        let source = match (cur.energy.is_some(), battery.is_some()) {
            (true, true) => "both",
            (true, false) => "rapl",
            (false, true) => "battery",
            (false, false) => "none",
        };
        let uptime = root
            .read("proc/uptime")
            .ok()
            .and_then(|u| crate::selfcost::uptime_sec(&u));
        let mut data = SystemData {
            cpu,
            load: root
                .read("proc/loadavg")
                .ok()
                .as_deref()
                .and_then(cpu::loadavg),
            mem: memv,
            pressure: pressure::read(root),
            temps,
            thermal,
            gpus,
            power: Some(Power {
                package_watts: package,
                battery_watts: battery,
                source: source.to_string(),
                needs_helper: power::needs_helper(root),
            }),
            uptime_sec: uptime,
            boot_time: cur.stat.btime,
            advice: Vec::new(),
        };
        data.advice = self.advice(&data);
        data
    }

    fn advice(&self, data: &SystemData) -> Vec<Advice> {
        let Some(up) = data.uptime_sec else {
            return Vec::new();
        };
        let reason = if up > ADVICE_UPTIME {
            Some("uptime-30d")
        } else if up > ADVICE_UPTIME_STRAINED {
            let swap_half = data.mem.as_ref().is_some_and(|m| {
                m.swap_total_bytes > 0 && m.swap_used_bytes * 2 > m.swap_total_bytes
            });
            let pressed =
                self.psi_hour.len() >= 60 && self.psi_hour.iter().all(|p| *p > ADVICE_PSI);
            if swap_half {
                Some("uptime-7d-swap")
            } else if pressed {
                Some("uptime-7d-pressure")
            } else {
                None
            }
        } else {
            None
        };
        reason
            .map(|r| Advice {
                code: "restart-recommended".to_string(),
                reason: r.to_string(),
            })
            .into_iter()
            .collect()
    }

    /// Adds this sample's gauges to the minute's means.
    fn accumulate(&mut self, d: &SystemData) {
        let mut add = |k: &'static str, v: Option<f64>| {
            if let Some(v) = v {
                let e = self.acc.entry(k).or_default();
                e.0 += v;
                e.1 += 1;
            }
        };
        if let Some(m) = &d.mem {
            add("mem.used", Some(m.used_bytes as f64));
            add("mem.available", Some(m.available_bytes as f64));
            add("mem.cache", Some(m.cache_bytes as f64));
            add(
                "swap.used",
                (m.swap_total_bytes > 0).then_some(m.swap_used_bytes as f64),
            );
        }
        add("temp.cpu", hottest(d, "cpu"));
        add("temp.gpu", hottest(d, "gpu"));
        add(
            "cpu.mhz",
            d.cpu.as_ref().and_then(|c| c.mhz_avg).map(f64::from),
        );
        for (n, key) in ["gpu.0.busy", "gpu.1.busy"].into_iter().enumerate() {
            add(key, d.gpus.get(n).and_then(|g| g.busy_percent));
        }
    }

    /// `minute` at an explicit wall time and instant, for tests.
    pub fn minute_at(&mut self, root: &Root, wall: u64, at: Instant) -> Vec<(&'static str, i64)> {
        let cur = Reading::take(root, at, &self.cpu_ids);
        let prev = self
            .minute_prev
            .take()
            .filter(|(_, w)| wall > *w && wall - w <= MAX_MINUTE_GAP_SECS)
            .map(|(r, _)| r);
        let data = self.build(root, &cur, prev.as_ref());
        let acc = std::mem::take(&mut self.acc);
        let mean =
            |k: &str, point: Option<f64>| acc.get(k).map(|(s, n)| s / f64::from(*n)).or(point);
        let mut out = Vec::new();
        let mut put = |k: &str, v: Option<f64>| {
            if let Some(kv) = v.filter(|v| v.is_finite()).and_then(|v| scaled(k, v)) {
                out.push(kv);
            }
        };
        if let Some(c) = &data.cpu {
            put("cpu.total", c.percent);
            put("cpu.user", c.user);
            put("cpu.system", c.system);
            put("cpu.iowait", c.iowait);
            put("cpu.mhz", mean("cpu.mhz", c.mhz_avg.map(f64::from)));
        }
        put("load.one", data.load.as_ref().map(|l| l.one));
        if let Some(m) = &data.mem {
            put("mem.used", mean("mem.used", Some(m.used_bytes as f64)));
            put(
                "mem.available",
                mean("mem.available", Some(m.available_bytes as f64)),
            );
            put("mem.cache", mean("mem.cache", Some(m.cache_bytes as f64)));
            if m.swap_total_bytes > 0 {
                put(
                    "swap.used",
                    mean("swap.used", Some(m.swap_used_bytes as f64)),
                );
            }
        }
        if let (Some(p), Some(n)) = (prev.as_ref().and_then(|p| p.vm), cur.vm) {
            put("page.in", n.pgpgin.checked_sub(p.pgpgin).map(|d| d as f64));
            put(
                "page.out",
                n.pgpgout.checked_sub(p.pgpgout).map(|d| d as f64),
            );
            put("swap.in", n.pswpin.checked_sub(p.pswpin).map(|d| d as f64));
            put(
                "swap.out",
                n.pswpout.checked_sub(p.pswpout).map(|d| d as f64),
            );
        }
        if let Some(p) = &data.pressure {
            put("psi.cpu", p.cpu.as_ref().map(|s| s.some60));
            put("psi.memory", p.memory.as_ref().map(|s| s.some60));
            put("psi.io", p.io.as_ref().map(|s| s.some60));
        }
        put("temp.cpu", mean("temp.cpu", hottest(&data, "cpu")));
        put("temp.gpu", mean("temp.gpu", hottest(&data, "gpu")));
        // Keyed by the card's place among all cards, as `accumulate` and
        // the page are; a suspended card is skipped, not renumbered
        // (review of PR #7).
        for (n, g) in data.gpus.iter().take(2).enumerate() {
            if g.suspended {
                continue;
            }
            let busy = ["gpu.0.busy", "gpu.1.busy"][n];
            put(busy, mean(busy, g.busy_percent));
            put(
                ["gpu.0.vram", "gpu.1.vram"][n],
                g.vram_used_bytes.map(|v| v as f64),
            );
        }
        if let Some(pw) = &data.power {
            put("power.package", pw.package_watts);
            put("power.battery", pw.battery_watts);
        }
        if let Some(m) = data.pressure.as_ref().and_then(|p| p.memory.as_ref()) {
            self.psi_hour.push_back(m.some60);
            while self.psi_hour.len() > 60 {
                self.psi_hour.pop_front();
            }
        }
        self.minute_prev = Some((cur, wall));
        out
    }
}

impl System {
    /// The minute baseline when it is 1–90 s old.
    fn minute_baseline(&self, now: Instant) -> Option<Reading> {
        let (r, _) = self.minute_prev.as_ref()?;
        let age = now.saturating_duration_since(r.at);
        (BASELINE_MIN..=Duration::from_secs(MAX_MINUTE_GAP_SECS))
            .contains(&age)
            .then(|| r.clone())
    }
}

impl Sampler for System {
    fn stream(&self) -> &'static str {
        stream::SYSTEM
    }

    fn live(&self) -> bool {
        true
    }

    fn probe(&mut self, root: &Root) -> Result<(), String> {
        let stat = root
            .read("proc/stat")
            .map(|t| cpu::stat(&t))
            .unwrap_or_default();
        if stat.all.is_none() {
            return Err("proc/stat has no cpu line".to_string());
        }
        self.cpu_ids = stat.cores.iter().map(|(n, _)| *n).collect();
        self.model = root
            .read("proc/cpuinfo")
            .ok()
            .as_deref()
            .and_then(cpu::model);
        self.physical = cpu::physical(root, &self.cpu_ids);
        self.mhz_max = cpu::mhz_max(root, &self.cpu_ids);
        self.has_nvidia = gpu::cards(root).iter().any(|c| {
            root.read(&format!("sys/class/drm/{c}/device/vendor"))
                .is_ok_and(|v| v.trim() == "0x10de")
        });
        // The first minute has a baseline, so it stores a CPU value
        // (review of plan 001 phase C).
        self.minute_prev = Some((
            Reading::take(root, Instant::now(), &self.cpu_ids),
            wall_now(),
        ));
        Ok(())
    }

    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        let cur = Reading::take(root, now, &self.cpu_ids);
        // Without a recent live baseline (the first event of a
        // subscription), the minute one when it is fresh enough.
        let prev = self
            .live
            .take()
            .filter(|p| now.duration_since(p.at) <= LIVE_STALE)
            .or_else(|| self.minute_baseline(now));
        let data = self.build(root, &cur, prev.as_ref());
        self.accumulate(&data);
        self.live = Some(cur);
        serde_json::to_value(data).unwrap_or(Value::Null)
    }

    /// `get`: rates against the youngest of the live and minute baselines,
    /// neither of which it moves, so a `get` cannot shorten a subscriber's
    /// window (review of plan 001 phase B).
    /// With no baseline 1–90 s old (history off, or after a sleep), it
    /// takes its own second reading 200 ms later (review of PR #7: it
    /// averaged since the daemon started).
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        let young = |r: &Reading| {
            let age = now.saturating_duration_since(r.at);
            (BASELINE_MIN..=Duration::from_secs(MAX_MINUTE_GAP_SECS)).contains(&age)
        };
        let prev = self
            .live
            .iter()
            .chain(self.minute_prev.iter().map(|(r, _)| r))
            .filter(|r| young(r))
            .min_by_key(|r| now.saturating_duration_since(r.at))
            .cloned();
        let (prev, cur) = match prev {
            Some(p) => (p, Reading::take(root, now, &self.cpu_ids)),
            None => {
                let first = Reading::take(root, now, &self.cpu_ids);
                std::thread::sleep(SNAPSHOT_GAP);
                (
                    first,
                    Reading::take(root, now + SNAPSHOT_GAP, &self.cpu_ids),
                )
            }
        };
        let data = self.build(root, &cur, Some(&prev));
        serde_json::to_value(data).unwrap_or(Value::Null)
    }

    fn minute(&mut self, root: &Root) -> Vec<(&'static str, i64)> {
        self.minute_at(root, wall_now(), Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_nvidia_smi_waits_a_minute() {
        let t = Instant::now();
        assert!(nvidia_due(None, t));
        assert!(!nvidia_due(Some((t, false)), t + Duration::from_secs(1)));
        assert!(nvidia_due(Some((t, false)), t + Duration::from_secs(2)));
        assert!(!nvidia_due(Some((t, true)), t + Duration::from_secs(30)));
        assert!(nvidia_due(Some((t, true)), t + Duration::from_secs(60)));
    }
}
