//! Every state change goes through here (architecture §5.4, the dry-run
//! contract). Each helper takes `dry_run` and returns what it did, or under
//! `dry_run` what it would do, as one `wouldDo` entry; it changes nothing
//! when `dry_run` is set.

use crate::exec;
use crate::proto::strings;
use crate::root::Root;
use crate::samplers::apps::procfs;
use crate::sys;
use serde_json::{Value, json};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ActError {
    /// Never done, whatever the caller says.
    Refused(String),
    Failed(String),
    /// The process is not there any more: it exited, or its pid now names
    /// another process (plan 006). Not a failure.
    Gone(String),
}

impl std::fmt::Display for ActError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActError::Refused(m) | ActError::Failed(m) | ActError::Gone(m) => f.write_str(m),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
    Int,
    Hup,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
            Signal::Int => "INT",
            Signal::Hup => "HUP",
        }
    }

    fn number(self) -> i32 {
        match self {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Int => libc::SIGINT,
            Signal::Hup => libc::SIGHUP,
        }
    }
}

/// A process as an action names it: its pid and its start time (field 22
/// of `proc/<pid>/stat`, clock ticks after boot), so a pid the kernel has
/// given to another process since is never mistaken for it (plan 006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Target {
    pub pid: u32,
    pub start: u64,
}

/// What delivers a signal or runs a command for an action. `Real` signals
/// through a pidfd and runs commands through exec.rs; `Recording` only
/// writes down what it would have done. The daemon picks `Real` only on
/// the live machine (`sender_for`).
pub trait Sender: Send + Sync {
    /// Whether this sender reaches real processes.
    fn is_real(&self) -> bool;
    /// Step 1 of `signal`: a handle on the process before it is checked.
    fn open(&self, pid: u32) -> std::io::Result<Option<OwnedFd>>;
    /// Step 4: the signal, through the handle `open` gave.
    fn send(&self, handle: Option<&OwnedFd>, target: Target, sig: Signal) -> std::io::Result<()>;
    /// A command for an action (`systemctl --user stop`).
    fn run(&self, argv: &[&str], timeout: Duration) -> Result<exec::Output, String>;
}

/// The live machine: pidfd_open, then pidfd_send_signal (sys.rs).
pub struct Real;

impl Sender for Real {
    fn is_real(&self) -> bool {
        true
    }

    fn open(&self, pid: u32) -> std::io::Result<Option<OwnedFd>> {
        sys::pidfd_open(pid).map(Some)
    }

    fn send(&self, handle: Option<&OwnedFd>, _target: Target, sig: Signal) -> std::io::Result<()> {
        let fd = handle.ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))?;
        sys::pidfd_signal(fd.as_fd(), sig.number())
    }

    fn run(&self, argv: &[&str], timeout: Duration) -> Result<exec::Output, String> {
        exec::run(argv, None, timeout).map_err(|e| e.to_string())
    }
}

/// A fixture root: every signal and command is written down, nothing is
/// sent or run. Lines are `TERM 2001 5010` and `RUN systemctl …`; they are
/// kept in memory, and appended to the file `MIRQAB_ACT_LOG` names when
/// `MIRQAB_TEST=1`, so a test of the binary can read them.
#[derive(Default)]
pub struct Recording {
    lines: Mutex<Vec<String>>,
    log: Option<PathBuf>,
}

impl Recording {
    pub fn new() -> Self {
        let log = (std::env::var("MIRQAB_TEST").as_deref() == Ok("1"))
            .then(|| std::env::var_os("MIRQAB_ACT_LOG"))
            .flatten()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        Recording {
            lines: Mutex::new(Vec::new()),
            log,
        }
    }

    /// What was recorded, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    fn record(&self, line: String) {
        if let Some(path) = &self.log {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| writeln!(f, "{line}"));
        }
        if let Ok(mut l) = self.lines.lock() {
            l.push(line);
        }
    }
}

impl Sender for Recording {
    fn is_real(&self) -> bool {
        false
    }

    fn open(&self, _pid: u32) -> std::io::Result<Option<OwnedFd>> {
        Ok(None)
    }

    fn send(&self, _handle: Option<&OwnedFd>, target: Target, sig: Signal) -> std::io::Result<()> {
        self.record(format!("{} {} {}", sig.name(), target.pid, target.start));
        Ok(())
    }

    fn run(&self, argv: &[&str], _timeout: Duration) -> Result<exec::Output, String> {
        self.record(format!("RUN {}", argv.join(" ")));
        Ok(exec::Output {
            status: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// The sender for a daemon on `root`: `Real` only when the root is the
/// live machine, `Recording` on any other root whatever the environment
/// says. A fixture's pid 2001 may be a real process of whoever runs the
/// tests (plan 006, decision 2).
pub fn sender_for(root: &Root) -> Arc<dyn Sender> {
    if root.is_host() {
        Arc::new(Real)
    } else {
        Arc::new(Recording::new())
    }
}

/// What a signal may never reach beyond the fixed rules: pids with the
/// reason each is refused (the compositor and its parents, the caller and
/// its parents), and who "the user" and "the daemon" are.
#[derive(Debug, Clone, Default)]
pub struct Guard {
    pub own_uid: u32,
    /// This process and the pid `proc/self/stat` names under the root.
    pub daemon: Vec<u32>,
    pub refused: Vec<(u32, String)>,
}

/// What `signal` read about a process, fresh, just before deciding.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub comm: String,
    pub start: u64,
    pub flags: u64,
    pub sid: u32,
    pub pgrp: u32,
}

/// `proc/<pid>/stat` now; None when the process is gone.
pub fn facts(root: &Root, pid: u32) -> Option<Facts> {
    let text = procfs::read_text(root, &format!("proc/{pid}/stat"))?;
    let st = procfs::stat(&text)?;
    Some(Facts {
        comm: st.comm,
        start: st.start,
        flags: st.flags,
        sid: st.sid,
        pgrp: st.pgrp,
    })
}

/// Why `pid` must not be signalled, or None. Everything is read again
/// through `root` (a cached uid must not decide a refusal): all four
/// numbers of `Uid:` must be the user's, so a setuid program is refused;
/// the cgroup must be under the user's slice; the user manager, the
/// daemon, kernel threads and the guard's pids are refused. Architecture
/// §5.4, plan 006 decision 5.
pub fn refusal(root: &Root, guard: &Guard, pid: u32, facts: &Facts) -> Option<String> {
    if pid <= 1 {
        return Some(strings::refuse_init(pid));
    }
    if guard.daemon.contains(&pid) {
        return Some(strings::refuse_daemon(pid));
    }
    if pid == 2 || facts.flags & procfs::PF_KTHREAD != 0 {
        return Some(strings::refuse_kernel(pid));
    }
    let Some(uids) = procfs::read_text(root, &format!("proc/{pid}/status")).and_then(|t| uids(&t))
    else {
        return Some(strings::refuse_unreadable(pid));
    };
    if uids.iter().any(|u| *u != guard.own_uid) {
        return Some(strings::refuse_other_user(pid));
    }
    let Some(cgroup) = procfs::read_text(root, &format!("proc/{pid}/cgroup"))
        .as_deref()
        .and_then(procfs::cgroup)
    else {
        return Some(strings::refuse_unreadable(pid));
    };
    let uid = guard.own_uid;
    if !cgroup.starts_with(&format!("/user.slice/user-{uid}.slice/")) {
        return Some(strings::refuse_outside(pid));
    }
    let manager = format!("user@{uid}.service");
    let parts: Vec<&str> = cgroup.split('/').collect();
    if parts
        .windows(2)
        .any(|w| w[0] == manager && w[1] == "init.scope")
    {
        return Some(strings::refuse_user_manager(pid));
    }
    guard
        .refused
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, why)| why.clone())
}

/// The four numbers of `Uid:` (real, effective, saved, filesystem).
fn uids(status: &str) -> Option<[u32; 4]> {
    let rest = status.lines().find_map(|l| l.strip_prefix("Uid:"))?;
    let n: Vec<u32> = rest
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    n.try_into().ok()
}

/// Sends `sig` to `target`, in an order that leaves no window for pid
/// reuse (plan 006, decision 1):
///
/// 1. open a pidfd for the pid;
/// 2. read `stat` and compare its start time with the planned one, so the
///    fd is known to name the planned process (it existed before step 1,
///    and an open pidfd cannot be re-pointed);
/// 3. run every refusal on what is read now;
/// 4. send the signal through the fd.
///
/// A start time that differs, or a process gone at step 1, 2 or 4, is
/// `Gone`. Under `dry_run` nothing is opened or sent; the checks still run.
/// There is no fallback to kill(2) on a kernel without pidfds.
pub fn signal(
    root: &Root,
    sender: &dyn Sender,
    guard: &Guard,
    target: Target,
    sig: Signal,
    dry_run: bool,
) -> Result<Value, ActError> {
    let pid = target.pid;
    if pid <= 1 {
        return Err(ActError::Refused(strings::refuse_init(pid)));
    }
    let handle = if dry_run {
        None
    } else {
        match sender.open(pid) {
            Ok(h) => h,
            Err(e) => return Err(open_error(pid, &e)),
        }
    };
    let Some(f) = facts(root, pid).filter(|f| f.start == target.start) else {
        return Err(ActError::Gone(strings::act_gone(pid)));
    };
    if let Some(why) = refusal(root, guard, pid, &f) {
        return Err(ActError::Refused(why));
    }
    let entry = json!({"signal": sig.name(), "pid": pid, "start": target.start, "name": f.comm});
    if dry_run {
        return Ok(entry);
    }
    match sender.send(handle.as_ref(), target, sig) {
        Ok(()) => Ok(entry),
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {
            Err(ActError::Gone(strings::act_gone(pid)))
        }
        Err(e) => Err(ActError::Failed(e.to_string())),
    }
}

fn open_error(pid: u32, e: &std::io::Error) -> ActError {
    match e.raw_os_error() {
        Some(libc::ESRCH) => ActError::Gone(strings::act_gone(pid)),
        Some(libc::ENOSYS) => ActError::Failed(strings::ACT_NO_PIDFD.to_string()),
        Some(libc::EINVAL) => ActError::Failed(strings::act_not_leader(pid)),
        _ => ActError::Failed(e.to_string()),
    }
}

/// Removes a file, or a directory and everything in it.
pub fn remove_path(path: &Path, dry_run: bool) -> Result<Value, ActError> {
    let entry = json!({"remove": path.display().to_string()});
    if dry_run {
        return Ok(entry);
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| ActError::Failed(e.to_string()))?;
    let r = if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    r.map(|()| entry)
        .map_err(|e| ActError::Failed(e.to_string()))
}

/// Writes `contents` to `path` atomically: a temporary file beside it, then a
/// rename, so a reader never sees half a file. When `path` is a symlink (a
/// config kept in a dotfiles repository), the file it points to is replaced
/// and the link stays; a dangling link is refused.
pub fn write_file(path: &Path, contents: &[u8], dry_run: bool) -> Result<Value, ActError> {
    let resolved;
    let path = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            resolved = std::fs::canonicalize(path)
                .map_err(|e| ActError::Failed(format!("{}: {e}", path.display())))?;
            resolved.as_path()
        }
        _ => path,
    };
    let entry = json!({"write": path.display().to_string(), "bytes": contents.len()});
    if dry_run {
        return Ok(entry);
    }
    let fail = |e: std::io::Error| ActError::Failed(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mirqab-act-{name}-{}", std::process::id()));
        let _ = remove_path(&d, false);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn dry_runs_change_nothing() {
        let d = scratch("dry");
        let f = d.join("x");
        std::fs::write(&f, "keep").unwrap();
        let w = write_file(&f, b"new", true).unwrap();
        assert_eq!(w["bytes"], 3);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "keep");
        let r = remove_path(&f, true).unwrap();
        assert!(r["remove"].as_str().unwrap().ends_with("/x"));
        assert!(f.exists());
        remove_path(&d, false).unwrap();
    }

    #[test]
    fn writes_atomically_and_removes() {
        let d = scratch("real");
        let f = d.join("sub/config.json");
        write_file(&f, b"{}", false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}");
        assert_eq!(
            std::fs::read_dir(d.join("sub")).unwrap().count(),
            1,
            "no temporary left"
        );
        remove_path(&d.join("sub"), false).unwrap();
        assert!(!d.join("sub").exists());
        remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_symlink_is_written_through_and_kept() {
        let d = scratch("link");
        std::fs::create_dir_all(d.join("dotfiles")).unwrap();
        std::fs::write(d.join("dotfiles/config.json"), "{}").unwrap();
        std::os::unix::fs::symlink("dotfiles/config.json", d.join("config.json")).unwrap();
        let w = write_file(&d.join("config.json"), b"{\"a\":1}", false).unwrap();
        assert!(
            w["write"]
                .as_str()
                .unwrap()
                .ends_with("dotfiles/config.json")
        );
        let meta = std::fs::symlink_metadata(d.join("config.json")).unwrap();
        assert!(meta.file_type().is_symlink(), "the link stays a link");
        assert_eq!(
            std::fs::read_to_string(d.join("dotfiles/config.json")).unwrap(),
            "{\"a\":1}"
        );
        std::os::unix::fs::symlink("nowhere.json", d.join("dangling.json")).unwrap();
        assert!(write_file(&d.join("dangling.json"), b"{}", false).is_err());
        assert!(!d.join("nowhere.json").exists());
        remove_path(&d, false).unwrap();
    }

    /// A fixture of processes: (pid, comm, flags, uids, cgroup).
    fn procs(d: &Path, rows: &[(u32, &str, u64, [u32; 4], &str)]) {
        for (pid, comm, flags, u, cg) in rows {
            let dir = d.join(format!("proc/{pid}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("stat"),
                format!(
                    "{pid} ({comm}) S 1 {pid} {pid} 0 -1 {flags} 0 0 0 0 1 1 0 0 20 0 1 0 500 0 0"
                ),
            )
            .unwrap();
            std::fs::write(
                dir.join("status"),
                format!(
                    "Name:\t{comm}\nUid:\t{}\t{}\t{}\t{}\n",
                    u[0], u[1], u[2], u[3]
                ),
            )
            .unwrap();
            std::fs::write(dir.join("cgroup"), format!("0::{cg}\n")).unwrap();
        }
    }

    #[test]
    fn signals_are_refused_by_every_rule_and_recorded_otherwise() {
        let d = scratch("sig");
        let app = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-x-1.scope";
        procs(
            &d,
            &[
                (4242, "ok", 0, [1000; 4], app),
                (4343, "rootly", 0, [0; 4], app),
                (4444, "suid", 0, [1000, 0, 0, 1000], app),
                (4545, "kworker", 0x0020_0000, [0; 4], "/"),
                (4646, "sshd", 0, [1000; 4], "/system.slice/sshd.service"),
                (
                    4747,
                    "systemd",
                    0,
                    [1000; 4],
                    "/user.slice/user-1000.slice/user@1000.service/init.scope",
                ),
                (4848, "parent", 0, [1000; 4], app),
            ],
        );
        let root = Root::new(&d);
        let guard = Guard {
            own_uid: 1000,
            daemon: vec![std::process::id()],
            refused: vec![(4848, "the caller's parent".into())],
        };
        let rec = Recording::default();
        let t = |pid| Target { pid, start: 500 };
        let refused = |pid: u32| {
            matches!(
                signal(&root, &rec, &guard, t(pid), Signal::Term, false),
                Err(ActError::Refused(_))
            )
        };
        for pid in [1, 4343, 4444, 4545, 4646, 4747, 4848] {
            assert!(refused(pid), "{pid} is refused");
        }
        assert!(matches!(
            signal(
                &root,
                &rec,
                &guard,
                t(std::process::id()),
                Signal::Term,
                false
            ),
            Err(ActError::Refused(_) | ActError::Gone(_))
        ));
        assert!(matches!(
            signal(&root, &rec, &guard, t(9999), Signal::Term, false),
            Err(ActError::Gone(_))
        ));
        assert!(
            matches!(
                signal(
                    &root,
                    &rec,
                    &guard,
                    Target {
                        pid: 4242,
                        start: 1
                    },
                    Signal::Term,
                    false
                ),
                Err(ActError::Gone(_))
            ),
            "another start time is another process"
        );
        assert!(rec.lines().is_empty(), "nothing refused or gone was sent");
        let ok = signal(&root, &rec, &guard, t(4242), Signal::Kill, true).unwrap();
        assert_eq!(
            ok,
            json!({"signal": "KILL", "pid": 4242, "start": 500, "name": "ok"})
        );
        assert!(rec.lines().is_empty(), "a dry run sends nothing");
        signal(&root, &rec, &guard, t(4242), Signal::Term, false).unwrap();
        assert_eq!(rec.lines(), ["TERM 4242 500"]);
        remove_path(&d, false).unwrap();
    }

    #[test]
    fn only_the_live_machine_gets_the_real_sender() {
        assert!(sender_for(&Root::host()).is_real());
        assert!(!sender_for(&Root::new("/tmp/mirqab-fixture")).is_real());
    }
}
