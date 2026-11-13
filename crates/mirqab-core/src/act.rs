//! Every state change goes through here (architecture §5.4, the dry-run
//! contract). Each helper takes `dry_run` and returns what it did, or under
//! `dry_run` what it would do, as one `wouldDo` entry; it changes nothing
//! when `dry_run` is set.

use crate::exec;
use crate::root::Root;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ActError {
    /// Never done, whatever the caller says.
    Refused(String),
    Failed(String),
}

impl std::fmt::Display for ActError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActError::Refused(m) | ActError::Failed(m) => f.write_str(m),
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
}

/// The real uid of a process, from `proc/<pid>/status`.
fn process_uid(root: &Root, pid: u32) -> Option<u32> {
    let status = root.read(&format!("proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// Sends `sig` to `pid`. Refused for pid 0 and 1, for the daemon itself and
/// for a process of another user. Later plans add the cgroup and compositor
/// refusals of §5.4 with the actions that need them.
///
/// It runs `kill(1)` through exec.rs rather than calling kill(2), so the
/// daemon needs no `unsafe` here; a signal is a rare, user-started action.
pub fn signal(
    root: &Root,
    own_uid: u32,
    pid: u32,
    sig: Signal,
    dry_run: bool,
) -> Result<Value, ActError> {
    if pid <= 1 {
        return Err(ActError::Refused(format!("pid {pid} is never signalled")));
    }
    if pid == std::process::id() {
        return Err(ActError::Refused(
            "the daemon does not signal itself".into(),
        ));
    }
    match process_uid(root, pid) {
        Some(uid) if uid == own_uid => {}
        Some(_) => {
            return Err(ActError::Refused(format!(
                "pid {pid} belongs to another user"
            )));
        }
        None => return Err(ActError::Failed(format!("pid {pid} is not running"))),
    }
    let entry = json!({"signal": sig.name(), "pid": pid});
    if dry_run {
        return Ok(entry);
    }
    let pid_s = pid.to_string();
    match exec::run(
        &["kill", "-s", sig.name(), "--", &pid_s],
        None,
        Duration::from_secs(5),
    ) {
        Ok(out) if out.success() => Ok(entry),
        Ok(out) => Err(ActError::Failed(out.stderr.trim().to_string())),
        Err(e) => Err(ActError::Failed(e.to_string())),
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

    #[test]
    fn signals_are_refused_for_pid_one_self_and_other_users() {
        let d = scratch("sig");
        for (pid, uid) in [(4242u32, 1000u32), (4343, 0)] {
            std::fs::create_dir_all(d.join(format!("proc/{pid}"))).unwrap();
            std::fs::write(
                d.join(format!("proc/{pid}/status")),
                format!("Name:\tx\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
        }
        let root = Root::new(&d);
        assert!(matches!(
            signal(&root, 1000, 1, Signal::Term, true),
            Err(ActError::Refused(_))
        ));
        assert!(matches!(
            signal(&root, 1000, std::process::id(), Signal::Term, true),
            Err(ActError::Refused(_))
        ));
        assert!(matches!(
            signal(&root, 1000, 4343, Signal::Term, true),
            Err(ActError::Refused(_))
        ));
        assert!(matches!(
            signal(&root, 1000, 9999, Signal::Term, true),
            Err(ActError::Failed(_))
        ));
        let ok = signal(&root, 1000, 4242, Signal::Term, true).unwrap();
        assert_eq!(ok, json!({"signal": "TERM", "pid": 4242}));
        remove_path(&d, false).unwrap();
    }
}
