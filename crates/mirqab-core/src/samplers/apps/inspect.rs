//! The process inspector, `get apps.inspect {pid}` (plan 006): where a
//! process came from, whether a package owns it, how it is sandboxed and
//! what it holds, headed by a plain-words verdict from fixed rules. Linux
//! has no code signatures; package ownership, Flatpak and the kernel's
//! sandbox flags answer the same questions. Every field is read on demand
//! and left out when it cannot be read.

use super::group;
use super::procfs::{self, User};
use crate::exec;
use crate::proto::stream::{
    Arch, GpuMemory, GroupRef, Inspect, OpenFiles, Package, ProcMemory, ProcRef, Sandbox, Verdict,
};
use crate::proto::{Code, strings};
use crate::root::Root;
use std::collections::HashMap;
use std::time::Duration;

/// How long one `pacman` call may take.
const PACMAN_TIMEOUT: Duration = Duration::from_secs(5);
/// Package answers kept, by path; cleared when full, as paths are few.
const OWNER_CACHE: usize = 512;
/// Open files listed by path.
const MAX_PATHS: usize = 50;
/// Parents walked up at most, so a loop of reused pids cannot hang it.
const MAX_PARENTS: usize = 64;
/// The suffix the kernel adds to `exe` when the file was replaced.
const DELETED: &str = " (deleted)";
/// The namespaces compared with the daemon's own.
const NAMESPACES: &[&str] = &["cgroup", "ipc", "mnt", "net", "pid", "user", "uts"];
/// Capability names by bit (include/uapi/linux/capability.h).
const CAPS: &[&str] = &[
    "chown",
    "dac_override",
    "dac_read_search",
    "fowner",
    "fsetid",
    "kill",
    "setgid",
    "setuid",
    "setpcap",
    "linux_immutable",
    "net_bind_service",
    "net_broadcast",
    "net_admin",
    "net_raw",
    "ipc_lock",
    "ipc_owner",
    "sys_module",
    "sys_rawio",
    "sys_chroot",
    "sys_ptrace",
    "sys_pacct",
    "sys_admin",
    "sys_boot",
    "sys_nice",
    "sys_resource",
    "sys_time",
    "sys_tty_config",
    "mknod",
    "lease",
    "audit_write",
    "audit_control",
    "setfcap",
    "mac_override",
    "mac_admin",
    "syslog",
    "wake_alarm",
    "block_suspend",
    "audit_read",
    "perfmon",
    "bpf",
    "checkpoint_restore",
];

/// Who owns an executable, as pacman says.
#[derive(Debug, Clone, PartialEq)]
pub enum Owner {
    /// No package database to ask (pacman is missing), or it failed.
    Unknown,
    NotOwned,
    Package(Package),
}

/// `pacman -Qo`, through `exec.rs`, cached per path (plan 006: "on
/// demand, cached per path"). A distribution without pacman gives
/// `Unknown`, and the package row is left out.
pub struct Owners {
    program: String,
    by_path: HashMap<String, Owner>,
    missing: bool,
}

impl Owners {
    pub fn new(program: impl Into<String>) -> Self {
        Owners {
            program: program.into(),
            by_path: HashMap::new(),
            missing: false,
        }
    }

    pub fn owner(&mut self, path: &str) -> Owner {
        if self.missing {
            return Owner::Unknown;
        }
        if let Some(o) = self.by_path.get(path) {
            return o.clone();
        }
        let out = match exec::run(&[&self.program, "-Qqo", path], None, PACMAN_TIMEOUT) {
            Err(exec::ExecError::Spawn(_)) => {
                self.missing = true;
                return Owner::Unknown;
            }
            // A timeout says nothing about the file; asked again next time.
            Err(_) => return Owner::Unknown,
            Ok(o) => o,
        };
        let name = out.stdout.lines().next().map(str::trim).unwrap_or("");
        let owner = if out.success() && !name.is_empty() {
            Owner::Package(Package {
                name: name.to_string(),
                repo: self.repo(name),
            })
        } else {
            Owner::NotOwned
        };
        if self.by_path.len() >= OWNER_CACHE {
            self.by_path.clear();
        }
        self.by_path.insert(path.to_string(), owner.clone());
        owner
    }

    /// The sync repository that provides `pkg`: `pacman -Sddp
    /// --print-format %r` prints it without changing anything; it fails for
    /// a package no repository has (built locally, from the AUR).
    fn repo(&self, pkg: &str) -> Option<String> {
        let out = exec::run(
            &[&self.program, "-Sddp", "--print-format", "%r", pkg],
            None,
            PACMAN_TIMEOUT,
        )
        .ok()
        .filter(exec::Output::success)?;
        let repo = out.stdout.lines().next()?.trim();
        (!repo.is_empty()).then(|| repo.to_string())
    }
}

/// What the inspector needs to know about the machine and the daemon.
pub struct Ctx<'a> {
    pub own_uid: u32,
    pub tck: u64,
    pub page: u64,
    pub users: &'a [User],
    /// The user's home when the account is not in `etc/passwd` (a
    /// systemd-homed account): the daemon's own `$HOME`.
    pub own_home: Option<&'a str>,
    /// The group the `apps` stream counts the process in.
    pub group: Option<GroupRef>,
}

/// The ELF machine type and word size from the first 20 bytes of a file.
pub fn elf(head: &[u8]) -> Option<(u16, u32)> {
    if head.len() < 20 || &head[..4] != b"\x7fELF" {
        return None;
    }
    let bits = match head[4] {
        1 => 32,
        2 => 64,
        _ => return None,
    };
    let pair = [head[18], head[19]];
    let machine = match head[5] {
        2 => u16::from_be_bytes(pair),
        _ => u16::from_le_bytes(pair),
    };
    Some((machine, bits))
}

/// e_machine values (elf.h) by the name `uname -m` users know.
fn machine_name(m: u16) -> String {
    match m {
        3 => "i386".into(),
        8 => "mips".into(),
        20 => "ppc".into(),
        21 => "ppc64".into(),
        22 => "s390x".into(),
        40 => "arm".into(),
        62 => "x86-64".into(),
        183 => "aarch64".into(),
        243 => "riscv".into(),
        258 => "loongarch".into(),
        other => format!("elf-{other}"),
    }
}

fn arch(root: &Root, pid: u32) -> Option<Arch> {
    let (machine, bits) = root
        .read_prefix(&format!("proc/{pid}/exe"), 20)
        .ok()
        .as_deref()
        .and_then(elf)?;
    let own = root
        .read_prefix("proc/self/exe", 20)
        .ok()
        .as_deref()
        .and_then(elf);
    Some(Arch {
        machine: machine_name(machine),
        bits,
        // Unknown when the daemon's own header cannot be read.
        foreign: own.is_some_and(|(m, b)| m != machine || b != bits),
    })
}

/// Capability names for the set bits of `CapEff`.
pub fn caps(mask: u64) -> Vec<String> {
    (0..64)
        .filter(|bit| mask & (1u64 << bit) != 0)
        .map(|bit| {
            CAPS.get(bit as usize)
                .map_or_else(|| bit.to_string(), |n| n.to_string())
        })
        .collect()
}

fn namespaces(root: &Root, pid: u32) -> Vec<String> {
    NAMESPACES
        .iter()
        .filter(|n| {
            let theirs = root.readlink(&format!("proc/{pid}/ns/{n}"));
            let ours = root.readlink(&format!("proc/self/ns/{n}"));
            matches!((theirs, ours), (Ok(a), Ok(b)) if a != b)
        })
        .map(|n| n.to_string())
        .collect()
}

fn open_files(root: &Root, pid: u32) -> Option<OpenFiles> {
    let mut fds: Vec<u32> = root
        .read_dir(&format!("proc/{pid}/fd"))
        .ok()?
        .iter()
        .filter_map(|n| n.parse().ok())
        .collect();
    fds.sort_unstable();
    let mut files = OpenFiles::default();
    for fd in fds {
        let Ok(target) = root.readlink(&format!("proc/{pid}/fd/{fd}")) else {
            continue;
        };
        let target = target.to_string_lossy();
        files.count += 1;
        if target.starts_with("socket:") {
            files.sockets += 1;
        } else if target.starts_with('/') && files.paths.len() < MAX_PATHS {
            files.paths.push(target.into_owned());
        }
    }
    Some(files)
}

/// A size in an fdinfo line: `512 KiB`, `2 MiB` or plain bytes.
fn fdinfo_bytes(v: &str) -> Option<u64> {
    let mut it = v.split_whitespace();
    let n: u64 = it.next()?.parse().ok()?;
    Some(match it.next() {
        Some("KiB") => n * 1024,
        Some("MiB") => n * 1024 * 1024,
        Some("GiB") => n * 1024 * 1024 * 1024,
        _ => n,
    })
}

/// GPU memory from `fdinfo/*` (https://docs.kernel.org/gpu/drm-usage-stats.html).
/// A process holds several fds of one DRM client, and each reports the
/// client's memory, so a client is counted once by `drm-client-id`.
fn gpu(root: &Root, pid: u32) -> Option<GpuMemory> {
    let names = root.read_dir(&format!("proc/{pid}/fdinfo")).ok()?;
    let mut clients: HashMap<String, [Option<u64>; 3]> = HashMap::new();
    for n in names {
        let Ok(text) = root.read(&format!("proc/{pid}/fdinfo/{n}")) else {
            continue;
        };
        let mut id = None;
        let mut mem = [None; 3];
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let slot = match k.trim() {
                "drm-client-id" => {
                    id = Some(v.trim().to_string());
                    continue;
                }
                "drm-memory-vram" => 0,
                "drm-memory-gtt" => 1,
                "drm-memory-cpu" => 2,
                _ => continue,
            };
            mem[slot] = fdinfo_bytes(v);
        }
        if let Some(id) = id
            && mem.iter().any(Option::is_some)
        {
            clients.insert(id, mem);
        }
    }
    if clients.is_empty() {
        return None;
    }
    let sum = |slot: usize| {
        clients
            .values()
            .filter_map(|m| m[slot])
            .fold(None, |acc: Option<u64>, v| Some(acc.unwrap_or(0) + v))
    };
    Some(GpuMemory {
        vram_bytes: sum(0),
        gtt_bytes: sum(1),
        cpu_bytes: sum(2),
    })
}

/// `Pss` and `Swap` of `smaps_rollup`, in bytes.
fn smaps(text: &str) -> (Option<u64>, Option<u64>) {
    let kb = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|v| v * 1024)
    };
    (kb("Pss:"), kb("Swap:"))
}

/// The parents up to the user manager (a `systemd`) or pid 1.
fn parents(root: &Root, mut ppid: u32) -> Vec<ProcRef> {
    let mut out = Vec::new();
    while ppid > 0 && out.len() < MAX_PARENTS {
        let Some(st) = procfs::read_text(root, &format!("proc/{ppid}/stat"))
            .as_deref()
            .and_then(procfs::stat)
        else {
            break;
        };
        out.push(ProcRef {
            pid: st.pid,
            name: st.comm.clone(),
        });
        if st.pid == 1 || st.comm == "systemd" {
            break;
        }
        ppid = st.ppid;
    }
    out
}

fn verdict(code: &str, text: String) -> Verdict {
    Verdict {
        code: code.to_string(),
        text,
    }
}

/// One process explained, or `failed` when there is no such process.
pub fn inspect(
    root: &Root,
    pid: u32,
    ctx: &Ctx,
    owners: &mut Owners,
) -> Result<Inspect, (Code, String)> {
    let dir = format!("proc/{pid}");
    let st = procfs::read_text(root, &format!("{dir}/stat"))
        .as_deref()
        .and_then(procfs::stat)
        .ok_or_else(|| (Code::Failed, strings::apps_no_process(pid)))?;
    let status = procfs::read_text(root, &format!("{dir}/status"))
        .map(|t| procfs::status(&t))
        .unwrap_or_default();
    let cmdline = root
        .read_bytes(&format!("{dir}/cmdline"))
        .map(|b| procfs::cmdline(&b))
        .unwrap_or_default();
    let tck = ctx.tck.max(1) as f64;
    let btime = root
        .read("proc/stat")
        .ok()
        .and_then(|t| crate::samplers::system::cpu::stat(&t).btime);
    let uptime: Option<f64> = root
        .read("proc/uptime")
        .ok()
        .and_then(|u| u.split_whitespace().next()?.parse().ok());
    let started_secs = st.start as f64 / tck;
    let user = status
        .uid
        .and_then(|u| ctx.users.iter().find(|x| x.uid == u));
    let rss = root
        .read(&format!("{dir}/statm"))
        .ok()
        .as_deref()
        .and_then(procfs::statm)
        .map(|m| m.resident * ctx.page);
    let kernel = procfs::is_kernel(&st);

    let mut out = Inspect {
        pid,
        name: procfs::display_name(&st.comm, &cmdline),
        ppid: Some(st.ppid),
        uid: status.uid,
        user: user.map(|u| u.name.clone()),
        started_at: btime.map(|b| b + started_secs as i64),
        age_sec: uptime.map(|u| (u - started_secs).max(0.0) as u64),
        cpu_time_sec: Some((st.ticks as f64 / tck * 100.0).round() / 100.0),
        threads: Some(st.threads),
        state: Some(st.state.to_string()),
        memory: rss.map(|r| ProcMemory {
            rss_bytes: r,
            pss_bytes: None,
            swap_bytes: None,
        }),
        group: ctx.group.clone(),
        ..Default::default()
    };
    if kernel {
        out.verdict = verdict("kernel", strings::VERDICT_KERNEL.to_string());
        return Ok(out);
    }
    if status.uid != Some(ctx.own_uid) {
        // Another user's: name, CPU and memory only (plan 006, "Not
        // possible on Linux": other users' details are limited).
        out.limited = true;
        out.ppid = None;
        let who = out
            .user
            .clone()
            .or_else(|| status.uid.map(|u| u.to_string()))
            .unwrap_or_default();
        out.verdict = verdict("other-user", strings::verdict_other_user(&who));
        return Ok(out);
    }

    out.parents = parents(root, st.ppid);
    out.cmdline = cmdline;
    out.cwd = root
        .readlink(&format!("{dir}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let exe_link = root
        .readlink(&format!("{dir}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    if let Some(link) = &exe_link {
        out.exe_deleted = link.ends_with(DELETED);
        out.exe = Some(link.strip_suffix(DELETED).unwrap_or(link).to_string());
    }
    let cg = procfs::read_text(root, &format!("{dir}/cgroup"))
        .as_deref()
        .and_then(procfs::cgroup);
    if let Some(cg) = &cg {
        out.unit = group::any_unit(cg).map(str::to_string);
        out.container = group::container_id(cg);
        out.flatpak = group::user_unit(cg).and_then(group::flatpak_scope);
    }
    out.cgroup = cg;
    if out.flatpak.is_none() {
        out.flatpak = root
            .read(&format!("{dir}/root/.flatpak-info"))
            .ok()
            .as_deref()
            .and_then(procfs::flatpak_info);
    }
    out.arch = arch(root, pid);
    out.sandbox = Some(Sandbox {
        seccomp: status.seccomp.map(|s| {
            match s {
                0 => "none",
                1 => "strict",
                _ => "filter",
            }
            .to_string()
        }),
        no_new_privs: status.no_new_privs.unwrap_or(false),
        capabilities: status.cap_eff.map(caps).unwrap_or_default(),
        namespaces: namespaces(root, pid),
    });
    if let Some(m) = &mut out.memory {
        let (pss, swap) = root
            .read(&format!("{dir}/smaps_rollup"))
            .map(|t| smaps(&t))
            .unwrap_or((None, None));
        m.pss_bytes = pss;
        m.swap_bytes = swap;
    }
    out.files = open_files(root, pid);
    out.gpu = gpu(root, pid);

    // A path inside a sandbox or a container names a file of its own
    // tree, which the host's package database knows nothing about.
    let sandboxed = out.flatpak.is_some() || out.container.is_some();
    let owner = match &out.exe {
        Some(exe) if !sandboxed => owners.owner(exe),
        _ => Owner::Unknown,
    };
    if let Owner::Package(p) = &owner {
        out.package = Some(p.clone());
    }
    let home = user
        .map(|u| u.home.as_str())
        .or(ctx.own_home)
        .filter(|h| h.len() > 1);
    let in_home = match (&out.exe, home) {
        (Some(exe), Some(h)) => exe.starts_with(&format!("{}/", h.trim_end_matches('/'))),
        _ => false,
    };
    out.verdict = if out.exe_deleted {
        verdict("replaced", strings::VERDICT_REPLACED.to_string())
    } else if out.flatpak.is_some() {
        verdict("flatpak", strings::VERDICT_FLATPAK.to_string())
    } else if out.container.is_some() {
        verdict("container", strings::VERDICT_CONTAINER.to_string())
    } else {
        match owner {
            Owner::Package(p) => verdict(
                "packaged",
                strings::verdict_packaged(&p.name, p.repo.as_deref()),
            ),
            Owner::NotOwned if in_home => verdict("home", strings::VERDICT_HOME.to_string()),
            Owner::NotOwned => verdict("unowned", strings::VERDICT_UNOWNED.to_string()),
            Owner::Unknown => verdict("unknown", strings::VERDICT_UNKNOWN.to_string()),
        }
    };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elf_headers() {
        let mut h = vec![0u8; 20];
        h[..4].copy_from_slice(b"\x7fELF");
        h[4] = 2;
        h[5] = 1;
        h[18] = 62;
        assert_eq!(elf(&h), Some((62, 64)));
        h[5] = 2;
        h[18] = 0;
        h[19] = 183;
        assert_eq!(elf(&h), Some((183, 64)));
        assert_eq!(elf(b"#!/bin/sh\n"), None);
        assert_eq!(machine_name(62), "x86-64");
    }

    #[test]
    fn capabilities_by_name() {
        assert_eq!(caps(0x3000), ["net_admin", "net_raw"]);
        assert!(caps(0).is_empty());
        assert_eq!(caps(1 << 50), ["50"]);
    }

    #[test]
    fn fdinfo_sizes() {
        assert_eq!(fdinfo_bytes("512 KiB"), Some(512 * 1024));
        assert_eq!(fdinfo_bytes("2 MiB"), Some(2 << 20));
        assert_eq!(fdinfo_bytes("100"), Some(100));
        assert_eq!(fdinfo_bytes("x"), None);
    }
}
