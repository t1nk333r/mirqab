//! One process from `proc/<pid>/` (plan 006). `stat` and `statm` are read
//! every tick, and `io` for the user's own processes (the kernel refuses
//! another user's); `status`, `cmdline` and `cgroup` only for a process not
//! seen before, cached by pid and start time, so a reused pid is a new
//! process. A file that cannot be read leaves its field empty: a process
//! may exit between two reads, and another user's files are closed to us.

use super::group;
use crate::root::Root;
use std::collections::HashMap;
use std::sync::Arc;

/// PF_KTHREAD in `stat`'s flags (include/linux/sched.h).
pub const PF_KTHREAD: u64 = 0x0020_0000;
/// The kernel cuts a command name to 15 bytes (TASK_COMM_LEN − 1).
const COMM_MAX: usize = 15;

/// The fields of `stat` the sampler uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub pid: u32,
    pub comm: String,
    pub state: char,
    pub ppid: u32,
    pub pgrp: u32,
    pub sid: u32,
    /// The terminal's foreground process group, or −1 without a terminal.
    pub tpgid: i64,
    pub flags: u64,
    /// utime + stime, in clock ticks.
    pub ticks: u64,
    pub threads: u32,
    /// Clock ticks after boot.
    pub start: u64,
}

/// `proc/<pid>/stat`. The command name (field 2) may hold spaces and
/// parentheses, so the fields after it count from the last `)`.
pub fn stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let pid = text.get(..open)?.trim().parse().ok()?;
    let comm = text.get(open + 1..close)?.to_string();
    let f: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // f[0] is field 3, so field N is f[N - 3].
    let num = |n: usize| -> Option<u64> { f.get(n - 3)?.parse().ok() };
    Some(Stat {
        pid,
        comm,
        state: f.first()?.chars().next()?,
        ppid: num(4)? as u32,
        pgrp: num(5)? as u32,
        sid: num(6)? as u32,
        tpgid: f.get(5)?.parse().ok()?,
        flags: num(9)?,
        ticks: num(14)? + num(15)?,
        threads: num(20)? as u32,
        start: num(22)?,
    })
}

/// Resident and shared pages, the second and third fields of `statm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Statm {
    pub resident: u64,
    pub shared: u64,
}

pub fn statm(text: &str) -> Option<Statm> {
    let mut it = text.split_whitespace().skip(1);
    Some(Statm {
        resident: it.next()?.parse().ok()?,
        shared: it.next()?.parse().ok()?,
    })
}

/// The fields of `status` the sampler and the inspector use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub uid: Option<u32>,
    pub no_new_privs: Option<bool>,
    pub seccomp: Option<u8>,
    pub cap_eff: Option<u64>,
}

pub fn status(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match key {
            // Real, effective, saved, filesystem: the real uid owns it.
            "Uid" => s.uid = val.split_whitespace().next().and_then(|v| v.parse().ok()),
            "NoNewPrivs" => s.no_new_privs = Some(val == "1"),
            "Seccomp" => s.seccomp = val.parse().ok(),
            "CapEff" => s.cap_eff = u64::from_str_radix(val, 16).ok(),
            _ => {}
        }
    }
    s
}

/// `cmdline`: the arguments, NUL-separated.
pub fn cmdline(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_suffix(b"\0").unwrap_or(bytes);
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes
        .split(|b| *b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// The cgroup v2 path: the `0::` line of `cgroup`.
pub fn cgroup(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(|p| p.trim().to_string())
}

/// `read_bytes` and `write_bytes` of `io`: what reached storage.
pub fn io(text: &str) -> Option<(u64, u64)> {
    let mut read = None;
    let mut write = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("read_bytes:") {
            read = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("write_bytes:") {
            write = v.trim().parse().ok();
        }
    }
    Some((read?, write?))
}

/// The app id in a Flatpak sandbox's `/.flatpak-info`.
pub fn flatpak_info(text: &str) -> Option<String> {
    let mut in_app = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_app = line == "[Application]";
        } else if in_app && let Some(v) = line.strip_prefix("name=") {
            return Some(v.trim().to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// One account of `etc/passwd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub home: String,
}

pub fn passwd(text: &str) -> Vec<User> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some(User {
                name: f.first()?.to_string(),
                uid: f.get(2)?.parse().ok()?,
                home: f.get(5)?.to_string(),
            })
        })
        .collect()
}

/// What is read once per process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub uid: Option<u32>,
    pub cgroup: Option<String>,
    pub cmdline: Vec<String>,
    /// From `root/.flatpak-info`, read only for the user's own processes
    /// that no user unit claims (rule 2 of the grouping).
    pub flatpak: Option<String>,
}

/// One process at one tick.
#[derive(Debug, Clone)]
pub struct Proc {
    pub stat: Stat,
    pub mem: Option<Statm>,
    pub io: Option<(u64, u64)>,
    pub info: Arc<Info>,
}

impl Proc {
    pub fn pid(&self) -> u32 {
        self.stat.pid
    }

    /// Pid and start time: the identity of a process across ticks.
    pub fn key(&self) -> (u32, u64) {
        (self.stat.pid, self.stat.start)
    }

    pub fn kernel(&self) -> bool {
        is_kernel(&self.stat)
    }

    pub fn name(&self) -> String {
        display_name(&self.stat.comm, &self.info.cmdline)
    }
}

/// kthreadd (pid 2), its children, and anything flagged PF_KTHREAD.
pub fn is_kernel(st: &Stat) -> bool {
    st.pid == 2 || st.ppid == 2 || st.flags & PF_KTHREAD != 0
}

/// The command name, or the program's file name when the kernel cut the
/// name short (`devserver-worke` → `devserver-worker`).
pub fn display_name(comm: &str, cmdline: &[String]) -> String {
    if comm.len() >= COMM_MAX
        && let Some(base) = cmdline.first().map(|a| basename(a))
        && base.starts_with(comm)
    {
        return base.to_string();
    }
    comm.to_string()
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A file of `proc/<pid>/` as text. Bytes that are not UTF-8 become U+FFFD:
/// a process may give itself any name (`prctl(PR_SET_NAME)`), and a name
/// that is not UTF-8 must not hide it (review of PR #16).
pub fn read_text(root: &Root, rel: &str) -> Option<String> {
    root.read_bytes(rel)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// What the cache knows of one pid.
struct Known {
    start: u64,
    comm: String,
    info: Arc<Info>,
    /// Read again once, on the second sighting: a launcher may move a
    /// process into its scope, or a wrapper exec a program of the same
    /// name, just after the first read (review of PR #16).
    settled: bool,
}

/// The per-process facts already read, by pid.
#[derive(Default)]
pub struct Cache {
    known: HashMap<u32, Known>,
}

/// Every process now, kernel threads included (the caller hides them).
pub fn walk(root: &Root, cache: &mut Cache, own_uid: u32) -> Vec<Proc> {
    let Ok(names) = root.read_dir("proc") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pid in names.iter().filter_map(|n| n.parse::<u32>().ok()) {
        let dir = format!("proc/{pid}");
        // Gone since the listing: skipped, not an error.
        let Some(st) = read_text(root, &format!("{dir}/stat"))
            .as_deref()
            .and_then(stat)
        else {
            continue;
        };
        // The same pid with another start time is another process; the same
        // one with another name has exec'd, and its cgroup and command line
        // may have changed with it.
        let info = match cache.known.get_mut(&pid) {
            Some(k) if k.start == st.start && k.comm == st.comm => {
                if !k.settled {
                    k.info = Arc::new(read_info(root, &dir, own_uid));
                    k.settled = true;
                }
                Arc::clone(&k.info)
            }
            _ => {
                let info = Arc::new(read_info(root, &dir, own_uid));
                cache.known.insert(
                    pid,
                    Known {
                        start: st.start,
                        comm: st.comm.clone(),
                        info: Arc::clone(&info),
                        settled: false,
                    },
                );
                info
            }
        };
        let mem = root
            .read(&format!("{dir}/statm"))
            .ok()
            .as_deref()
            .and_then(statm);
        let io = if info.uid == Some(own_uid) {
            root.read(&format!("{dir}/io")).ok().as_deref().and_then(io)
        } else {
            None
        };
        out.push(Proc {
            stat: st,
            mem,
            io,
            info,
        });
    }
    let seen: std::collections::HashSet<u32> = out.iter().map(Proc::pid).collect();
    cache.known.retain(|pid, _| seen.contains(pid));
    out
}

fn read_info(root: &Root, dir: &str, own_uid: u32) -> Info {
    let uid = read_text(root, &format!("{dir}/status")).and_then(|t| status(&t).uid);
    let cgroup = read_text(root, &format!("{dir}/cgroup"))
        .as_deref()
        .and_then(cgroup);
    let cmdline = root
        .read_bytes(&format!("{dir}/cmdline"))
        .map(|b| cmdline(&b))
        .unwrap_or_default();
    let unclaimed = cgroup
        .as_deref()
        .is_none_or(|c| group::user_unit(c).is_none());
    let flatpak = if uid == Some(own_uid) && unclaimed {
        root.read(&format!("{dir}/root/.flatpak-info"))
            .ok()
            .as_deref()
            .and_then(flatpak_info)
    } else {
        None
    };
    Info {
        uid,
        cgroup,
        cmdline,
        flatpak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_counts_fields_after_the_last_parenthesis() {
        let s = stat(
            "1101 ((sd-pam)) S 1100 1100 1100 0 -1 4194560 0 0 0 0 7 3 0 0 20 0 1 0 1210 0 0 0",
        )
        .unwrap();
        assert_eq!(s.comm, "(sd-pam)");
        assert_eq!((s.ppid, s.pgrp, s.sid, s.tpgid), (1100, 1100, 1100, -1));
        assert_eq!((s.ticks, s.threads, s.start), (10, 1, 1210));
        let spaced = stat(
            "2010 (Web Content) S 2001 2000 2000 34816 3020 0 0 0 0 0 1 1 0 0 20 0 30 0 5100 0 0",
        )
        .unwrap();
        assert_eq!(spaced.comm, "Web Content");
        assert_eq!(spaced.tpgid, 3020);
        assert!(stat("garbage").is_none());
        assert!(stat("12 (short) S 1").is_none());
    }

    #[test]
    fn status_cmdline_cgroup_io_and_passwd() {
        let s = status(
            "Name:\tx\nUid:\t1000\t1000\t1000\t1000\nNoNewPrivs:\t1\nSeccomp:\t2\nCapEff:\t0000000000003000\n",
        );
        assert_eq!(s.uid, Some(1000));
        assert_eq!(s.no_new_privs, Some(true));
        assert_eq!(s.seccomp, Some(2));
        assert_eq!(s.cap_eff, Some(0x3000));
        assert_eq!(cmdline(b"node\0vite\0"), ["node", "vite"]);
        assert!(cmdline(b"").is_empty());
        assert_eq!(cgroup("0::/init.scope\n").as_deref(), Some("/init.scope"));
        assert_eq!(
            io("rchar: 1\nread_bytes: 10\nwrite_bytes: 20\n"),
            Some((10, 20))
        );
        assert_eq!(
            flatpak_info("[Instance]\nname=x\n[Application]\nname=org.example.Notes\n").as_deref(),
            Some("org.example.Notes")
        );
        let users =
            passwd("root:x:0:0::/root:/bin/sh\nbroken\nuser:x:1000:1000:U:/users/user:/bin/zsh\n");
        assert_eq!(users.len(), 2);
        assert_eq!(users[1].home, "/users/user");
    }
}
