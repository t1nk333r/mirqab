//! Memory from `proc/meminfo`, compressed swap from zram's `mm_stat`, and
//! paging counters from `proc/vmstat` (plan 002).

use crate::proto::stream::Zram;
use crate::root::Root;

/// `proc/meminfo` in bytes; a missing field is 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfo {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub shmem: u64,
    pub sreclaimable: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemInfo {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    /// The part of available memory that is cache: available − free. Used,
    /// cache and free then add up to the total. Buffers + cached +
    /// reclaimable slab overlaps what the kernel already counts as
    /// available and can exceed it (seen on the development machine: 41 GB
    /// of cache beside 25 GB used of 65 GB).
    pub fn cache(&self) -> u64 {
        self.available.saturating_sub(self.free)
    }

    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }
}

pub fn meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(val)) = (it.next(), it.next()) else {
            continue;
        };
        let bytes = val.parse::<u64>().unwrap_or(0).saturating_mul(1024);
        match key {
            "MemTotal:" => m.total = bytes,
            "MemFree:" => m.free = bytes,
            "MemAvailable:" => m.available = bytes,
            "Buffers:" => m.buffers = bytes,
            "Cached:" => m.cached = bytes,
            "Shmem:" => m.shmem = bytes,
            "SReclaimable:" => m.sreclaimable = bytes,
            "SwapTotal:" => m.swap_total = bytes,
            "SwapFree:" => m.swap_free = bytes,
            _ => {}
        }
    }
    m
}

/// Every zram device's original and compressed sizes, summed; None with
/// no zram device.
pub fn zram(root: &Root) -> Option<Zram> {
    let mut out: Option<Zram> = None;
    for dev in root.read_dir("sys/block").ok()? {
        if !dev.starts_with("zram") {
            continue;
        }
        let Ok(text) = root.read(&format!("sys/block/{dev}/mm_stat")) else {
            continue;
        };
        let f: Vec<u64> = text
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if f.len() < 2 {
            continue;
        }
        let z = out.get_or_insert_with(Zram::default);
        z.orig_bytes += f[0];
        z.compr_bytes += f[1];
    }
    out
}

/// The paging counters of `proc/vmstat` that plan 002 shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmStat {
    pub pgpgin: u64,
    pub pgpgout: u64,
    pub pswpin: u64,
    pub pswpout: u64,
    pub oom_kill: Option<u64>,
}

pub fn vmstat(text: &str) -> Option<VmStat> {
    let mut v = VmStat::default();
    let mut any = false;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(k), Some(n)) = (it.next(), it.next().and_then(|n| n.parse::<u64>().ok())) else {
            continue;
        };
        match k {
            "pgpgin" => v.pgpgin = n,
            "pgpgout" => v.pgpgout = n,
            "pswpin" => v.pswpin = n,
            "pswpout" => v.pswpout = n,
            "oom_kill" => v.oom_kill = Some(n),
            _ => continue,
        }
        any = true;
    }
    any.then_some(v)
}

/// Per-second rates between two readings `secs` apart: page in, page out,
/// swap in, swap out. None when no time passed or a counter went back.
pub fn rates(prev: &VmStat, now: &VmStat, secs: f64) -> Option<[f64; 4]> {
    if secs <= 0.0 {
        return None;
    }
    let d = |a: u64, b: u64| a.checked_sub(b).map(|x| x as f64 / secs);
    Some([
        d(now.pgpgin, prev.pgpgin)?,
        d(now.pgpgout, prev.pgpgout)?,
        d(now.pswpin, prev.pswpin)?,
        d(now.pswpout, prev.pswpout)?,
    ])
}
