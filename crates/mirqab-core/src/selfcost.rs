//! What a process costs, from `proc/<pid>/stat` and `proc/<pid>/status`:
//! CPU percent of one core between two readings, resident memory and
//! threads (architecture §9.1). The daemon reads `proc/self`; the interface's
//! cost is read the same way from the pid its `ui` hello gave.

use crate::root::Root;
use std::time::Instant;

/// Clock ticks per second when the auxiliary vector does not say. Linux has
/// used 100 on every architecture mirqab targets.
const DEFAULT_CLK_TCK: u64 = 100;
/// AT_CLKTCK in the auxiliary vector (getauxval(3)).
const AT_CLKTCK: u64 = 17;

/// The page size when the auxiliary vector does not say (plan 006 reads
/// `statm`, which counts pages).
const DEFAULT_PAGE_SIZE: u64 = 4096;
/// AT_PAGESZ in the auxiliary vector.
const AT_PAGESZ: u64 = 6;

/// One entry of this process's auxiliary vector, read through Root.
fn auxv(root: &Root, key: u64) -> Option<u64> {
    let auxv = root.read_bytes("proc/self/auxv").ok()?;
    auxv.as_chunks::<16>()
        .0
        .iter()
        .map(|pair| {
            let (key, val) = pair.split_at(8);
            (
                u64::from_ne_bytes(key.try_into().unwrap_or_default()),
                u64::from_ne_bytes(val.try_into().unwrap_or_default()),
            )
        })
        .find(|&(k, _)| k == key)
        .map(|(_, v)| v)
        .filter(|&v| v > 0)
}

/// `sysconf(_SC_CLK_TCK)` without libc: glibc reads it from the auxiliary
/// vector, and so do we, through Root.
pub fn clk_tck(root: &Root) -> u64 {
    auxv(root, AT_CLKTCK).unwrap_or(DEFAULT_CLK_TCK)
}

/// `sysconf(_SC_PAGESIZE)`, the same way.
pub fn page_size(root: &Root) -> u64 {
    auxv(root, AT_PAGESZ).unwrap_or(DEFAULT_PAGE_SIZE)
}

/// utime + stime in ticks, fields 14 and 15 of `stat`. The command name in
/// field 2 may hold spaces and parentheses, so fields count from the last `)`.
pub fn cpu_ticks(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After ")": field 3 (state) is fields[0], so field N is fields[N - 3].
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

/// The first `model name` in `/proc/cpuinfo`, trimmed.
pub fn cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim() == "model name")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Whole seconds of uptime, the first field of `/proc/uptime`.
pub fn uptime_sec(uptime: &str) -> Option<u64> {
    let secs: f64 = uptime.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs as u64)
}

/// (VmRSS in bytes, Threads) from `status`.
pub fn rss_and_threads(status: &str) -> (u64, u32) {
    let mut rss = 0;
    let mut threads = 0;
    for line in status.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            rss = v
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024;
        } else if let Some(v) = line.strip_prefix("Threads:") {
            threads = v.trim().parse().unwrap_or(0);
        }
    }
    (rss, threads)
}

/// One process's cost over time. `proc_dir` is "proc/self" or "proc/<pid>".
pub struct Meter {
    proc_dir: String,
    tck: u64,
    last: Option<(u64, Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub cpu: f64,
    pub rss_bytes: u64,
    pub threads: u32,
}

impl Meter {
    pub fn new(root: &Root, proc_dir: impl Into<String>) -> Self {
        Meter {
            proc_dir: proc_dir.into(),
            tck: clk_tck(root),
            last: None,
        }
    }

    /// None when the process is gone. The first reading's CPU is 0: there is
    /// nothing to take a difference from.
    pub fn read(&mut self, root: &Root, now: Instant) -> Option<Reading> {
        let stat = root.read(&format!("{}/stat", self.proc_dir)).ok()?;
        let status = root
            .read(&format!("{}/status", self.proc_dir))
            .unwrap_or_default();
        let ticks = cpu_ticks(&stat)?;
        let (rss_bytes, threads) = rss_and_threads(&status);
        let cpu = match self.last {
            Some((t0, at)) if now > at && ticks >= t0 => {
                let secs = now.duration_since(at).as_secs_f64();
                ((ticks - t0) as f64 / self.tck as f64) / secs * 100.0
            }
            _ => 0.0,
        };
        self.last = Some((ticks, now));
        Some(Reading {
            cpu: (cpu * 100.0).round() / 100.0,
            rss_bytes,
            threads,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const STAT: &str = "4321 (mir qab) (x)) S 1 4321 4321 0 -1 4194560 300 0 0 0 150 50 0 0 20 0 4 0 100 1000 200 18446744073709551615";

    #[test]
    fn reads_ticks_after_the_last_parenthesis() {
        assert_eq!(cpu_ticks(STAT), Some(200));
        assert_eq!(cpu_ticks("garbage"), None);
    }

    #[test]
    fn reads_rss_and_threads() {
        let (rss, threads) = rss_and_threads("Name:\tmirqab\nVmRSS:\t   3072 kB\nThreads:\t4\n");
        assert_eq!(rss, 3072 * 1024);
        assert_eq!(threads, 4);
    }

    #[test]
    fn cpu_is_ticks_over_wall_time() {
        let d = std::env::temp_dir().join(format!("mirqab-selfcost-{}", std::process::id()));
        std::fs::create_dir_all(d.join("proc/self")).unwrap();
        std::fs::write(d.join("proc/self/stat"), STAT).unwrap();
        std::fs::write(d.join("proc/self/status"), "VmRSS:\t100 kB\nThreads:\t2\n").unwrap();
        let root = Root::new(&d);
        let mut m = Meter::new(&root, "proc/self");
        let t0 = Instant::now();
        assert_eq!(m.read(&root, t0).unwrap().cpu, 0.0);
        // 50 more ticks (0.5 s of CPU at 100 Hz) over 10 s of wall time: 5 %.
        std::fs::write(
            d.join("proc/self/stat"),
            STAT.replace(" 150 50 ", " 175 75 "),
        )
        .unwrap();
        let r = m.read(&root, t0 + Duration::from_secs(10)).unwrap();
        assert_eq!(r.cpu, 5.0);
        assert_eq!(r.rss_bytes, 100 * 1024);
        assert_eq!(r.threads, 2);
        crate::act::remove_path(&d, false).unwrap();
    }
}
