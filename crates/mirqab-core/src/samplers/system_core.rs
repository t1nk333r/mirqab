//! The core of the `system` stream (plan 001 phase C): CPU percent from
//! `proc/stat` differences and memory from `proc/meminfo`. Plan 002 grows
//! it into the System tab's full stream.

use crate::root::Root;
use crate::sampler::Sampler;
use serde_json::{Value, json};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Jiffies of the aggregate `cpu` line: (busy, total). Busy excludes idle
/// and iowait; guest time is already counted in user and nice.
pub fn cpu_jiffies(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|f| f.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// Busy percent between two readings, or None when no time passed.
pub fn cpu_percent(prev: (u64, u64), now: (u64, u64)) -> Option<f64> {
    let total = now.1.checked_sub(prev.1)?;
    let busy = now.0.checked_sub(prev.0)?;
    (total > 0).then(|| busy as f64 * 100.0 / total as f64)
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mem {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

/// `proc/meminfo`, in bytes. Missing fields are 0.
pub fn meminfo(text: &str) -> Mem {
    let mut m = Mem::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(val)) = (it.next(), it.next()) else {
            continue;
        };
        let bytes = val.parse::<u64>().unwrap_or(0) * 1024;
        match key {
            "MemTotal:" => m.total = bytes,
            "MemAvailable:" => m.available = bytes,
            "SwapTotal:" => m.swap_total = bytes,
            "SwapFree:" => m.swap_free = bytes,
            _ => {}
        }
    }
    m
}

/// A minute baseline older than this means the machine slept or the daemon
/// stalled: the difference would smear hours into one minute, so that minute
/// stores nothing (architecture §6.3, "a gap stores nothing").
const MAX_MINUTE_GAP_SECS: u64 = 90;

#[derive(Default)]
pub struct SystemCore {
    /// The previous live sample's jiffies.
    live_prev: Option<(u64, u64)>,
    /// The previous minute's jiffies and wall time.
    minute_prev: Option<((u64, u64), u64)>,
    /// Memory seen by live samples since the last minute: (used sum,
    /// available sum, count). A minute with live samples stores their mean.
    mem_acc: (u128, u128, u64),
}

impl SystemCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `minute` at an explicit wall time, for tests.
    pub fn minute_at(&mut self, root: &Root, wall_secs: u64) -> Vec<(&'static str, i64)> {
        let mut out = Vec::new();
        if let Some(j) = root.read("proc/stat").ok().as_deref().and_then(cpu_jiffies) {
            if let Some((prev, at)) = self.minute_prev
                && wall_secs > at
                && wall_secs - at <= MAX_MINUTE_GAP_SECS
                && let Some(p) = cpu_percent(prev, j)
            {
                out.push(("cpu.total", (p * 100.0).round() as i64));
            }
            self.minute_prev = Some((j, wall_secs));
        }
        if let Ok(text) = root.read("proc/meminfo") {
            let m = meminfo(&text);
            let (used_sum, avail_sum, n) = std::mem::take(&mut self.mem_acc);
            let (used, available) = if n > 0 {
                (
                    (used_sum / n as u128) as u64,
                    (avail_sum / n as u128) as u64,
                )
            } else {
                (m.total.saturating_sub(m.available), m.available)
            };
            if m.total > 0 {
                out.push(("mem.used", used as i64));
                out.push(("mem.available", available as i64));
            }
            if m.swap_total > 0 {
                out.push(("swap.used", m.swap_total.saturating_sub(m.swap_free) as i64));
            }
        }
        out
    }
}

impl Sampler for SystemCore {
    fn stream(&self) -> &'static str {
        "system"
    }

    fn live(&self) -> bool {
        true
    }

    fn probe(&mut self, root: &Root) -> Result<(), String> {
        root.read("proc/stat")
            .ok()
            .as_deref()
            .and_then(cpu_jiffies)
            .map(|_| ())
            .ok_or_else(|| "proc/stat has no cpu line".to_string())
    }

    fn sample(&mut self, root: &Root, _now: Instant) -> Value {
        let j = root.read("proc/stat").ok().as_deref().and_then(cpu_jiffies);
        let percent = match (self.live_prev, j) {
            (Some(p), Some(n)) => cpu_percent(p, n),
            _ => None,
        };
        if j.is_some() {
            self.live_prev = j;
        }
        let m = root
            .read("proc/meminfo")
            .map(|t| meminfo(&t))
            .unwrap_or_default();
        if m.total > 0 {
            let used = m.total.saturating_sub(m.available);
            self.mem_acc.0 += used as u128;
            self.mem_acc.1 += m.available as u128;
            self.mem_acc.2 += 1;
        }
        json!({
            "cpu": {"percent": percent.map(|p| (p * 10.0).round() / 10.0)},
            "mem": {"totalBytes": m.total, "availableBytes": m.available},
        })
    }

    fn minute(&mut self, root: &Root) -> Vec<(&'static str, i64)> {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.minute_at(root, wall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_excludes_idle_and_iowait() {
        let (busy, total) = cpu_jiffies("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        assert_eq!((busy, total), (150, 1000));
        assert_eq!(cpu_percent((150, 1000), (350, 2000)), Some(20.0));
        assert_eq!(cpu_percent((150, 1000), (150, 1000)), None);
        assert_eq!(cpu_jiffies("intr 0\n"), None);
    }

    #[test]
    fn meminfo_is_in_bytes() {
        let m = meminfo("MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 0 kB\n");
        assert_eq!(m.total, 1_024_000);
        assert_eq!(m.available, 409_600);
        assert_eq!(m.swap_total, 0);
    }
}
