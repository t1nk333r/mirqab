//! What an action must never touch beyond architecture §5.4's fixed
//! refusals (plan 005, rule 4; plan 006, decisions 6 and 7): the caller's
//! own ancestry, so an agent or a shell cannot kill itself, its terminal or
//! its editor by accident, and the compositor's. Read through `Root`, so
//! the walk is tested on fixture trees.

use crate::proto::strings;
use crate::root::Root;

/// The deepest walk: a loop in a broken tree must end.
const MAX_DEPTH: usize = 64;

/// Why a walk could not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkError {
    /// A `stat` on the way could not be read or parsed.
    Unreadable(u32),
    /// A pid came back: the parent links form a loop.
    Loop(u32),
    /// Deeper than MAX_DEPTH.
    TooDeep,
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            WalkError::Unreadable(pid) => strings::walk_unreadable(*pid),
            WalkError::Loop(pid) => strings::walk_loop(*pid),
            WalkError::TooDeep => strings::walk_too_deep(MAX_DEPTH),
        })
    }
}

/// `pid` and its ancestors up to (not including) pid 1, nearest first.
/// It fails closed: an unreadable `stat` on the way, a loop or a depth
/// past 64 is an error, and an action that needed the walk is refused
/// (plan 006: a quiet stop would leave the rest of the chain signallable).
pub fn ancestry_strict(root: &Root, pid: u32) -> Result<Vec<u32>, WalkError> {
    let mut out = Vec::new();
    let mut cur = pid;
    while cur > 1 {
        if out.contains(&cur) {
            return Err(WalkError::Loop(cur));
        }
        if out.len() >= MAX_DEPTH {
            return Err(WalkError::TooDeep);
        }
        out.push(cur);
        cur = parent(root, cur).ok_or(WalkError::Unreadable(cur))?;
    }
    Ok(out)
}

/// The session bus's units: never stopped or signalled, whoever asks.
/// Like the compositor, ending it ends the session (owner, 2026-11-14,
/// plan 006 Q2). `dbus-daemon.service` is the reference bus's own name
/// where `dbus.service` is only its alias (review of PR #18).
pub const SESSION_BUS: &[&str] = &["dbus.service", "dbus-broker.service", "dbus-daemon.service"];

/// The session bus by command name (owner, 2026-11-14, part 4 Q1): a
/// bus started by `dbus-run-session` or `dbus-launch` is in no bus unit,
/// and ending it ends whatever runs on it, so these processes of the user
/// are never signalled whatever unit they are in.
pub const SESSION_BUS_COMMS: &[&str] = &["dbus-daemon", "dbus-broker", "dbus-broker-launch"];

/// Whether a process named `comm` is a session bus. The kernel cuts a
/// command name to 15 bytes, so `dbus-broker-launch` runs as
/// `dbus-broker-lau`.
pub fn is_session_bus_comm(comm: &str) -> bool {
    SESSION_BUS_COMMS
        .iter()
        .any(|n| comm == *n || (n.len() > COMM_MAX && n.get(..COMM_MAX) == Some(comm)))
}

/// TASK_COMM_LEN − 1.
const COMM_MAX: usize = 15;

/// Services Quit may stop, with a warning in the confirmation: the sound
/// server and the desktop portals come back on next use, and stopping a
/// stuck one is a real use (owner, 2026-11-14, plan 006 Q2).
const SOUND: &[&str] = &[
    "pipewire.service",
    "pipewire-pulse.service",
    "wireplumber.service",
];
const PORTAL_PREFIX: &str = "xdg-desktop-portal";

/// Whether `unit` is the session bus.
pub fn is_session_bus(unit: &str) -> bool {
    SESSION_BUS.contains(&unit)
}

/// The warning a stop of `unit` carries, if any.
pub fn session_warning(unit: &str) -> Option<String> {
    if SOUND.contains(&unit) {
        Some(strings::warn_sound(unit))
    } else if unit.starts_with(PORTAL_PREFIX) && unit.ends_with(".service") {
        Some(strings::warn_portal(unit))
    } else {
        None
    }
}

/// The parent pid from `proc/<pid>/stat`: the field after the command,
/// whose parentheses may hold spaces, parentheses and any byte a process
/// set with `prctl`, so the line is read as bytes (review of PR #13: a
/// command name that was not UTF-8 ended the walk early).
fn parent(root: &Root, pid: u32) -> Option<u32> {
    let stat = root.read_bytes(&format!("proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.iter().rposition(|&b| b == b')')? + 1..];
    std::str::from_utf8(rest)
        .ok()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act;

    fn tree(name: &str, procs: &[(u32, &str, u32)]) -> (Root, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("mirqab-guard-{name}-{}", std::process::id()));
        let _ = act::remove_path(&d, false);
        for (pid, comm, ppid) in procs {
            let stat = format!("{pid} ({comm}) S {ppid} 1 1 0 -1\n");
            act::write_file(&d.join(format!("proc/{pid}/stat")), stat.as_bytes(), false).unwrap();
        }
        (Root::new(&d), d)
    }

    #[test]
    fn the_session_bus_is_named_and_sound_and_portals_warn() {
        assert!(is_session_bus("dbus.service"));
        assert!(is_session_bus("dbus-broker.service"));
        assert!(is_session_bus("dbus-daemon.service"));
        assert!(!is_session_bus("dbus.socket"));
        assert!(!is_session_bus("dbus-broker-launch.service"));
        for c in [
            "dbus-daemon",
            "dbus-broker",
            "dbus-broker-launch",
            "dbus-broker-lau",
        ] {
            assert!(is_session_bus_comm(c), "{c}");
        }
        assert!(!is_session_bus_comm("dbus-monitor"));
        assert!(!is_session_bus_comm("dbus"));
        for u in [
            "pipewire.service",
            "pipewire-pulse.service",
            "wireplumber.service",
        ] {
            assert_eq!(session_warning(u), Some(strings::warn_sound(u)));
        }
        for u in [
            "xdg-desktop-portal.service",
            "xdg-desktop-portal-hyprland.service",
        ] {
            assert_eq!(session_warning(u), Some(strings::warn_portal(u)));
        }
        assert_eq!(session_warning("pipewire.socket"), None);
        assert_eq!(session_warning("dbus-broker.service"), None);
        assert_eq!(session_warning("example-sync.service"), None);
    }

    #[test]
    fn the_walk_stops_at_init_and_reads_odd_command_names() {
        let (root, d) = tree(
            "walk",
            &[
                (900, "mirqab", 850),
                (850, "node (agent) x", 400),
                (400, "zsh", 1),
            ],
        );
        assert_eq!(ancestry_strict(&root, 900), Ok(vec![900, 850, 400]));
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_command_name_that_is_not_utf8_does_not_end_the_walk() {
        let (root, d) = tree("bytes", &[(400, "zsh", 1)]);
        act::write_file(
            &d.join("proc/900/stat"),
            b"900 (a\xff) b) S 400 1 1 0 -1\n",
            false,
        )
        .unwrap();
        assert_eq!(ancestry_strict(&root, 900), Ok(vec![900, 400]));
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_loop_a_missing_parent_or_depth_is_an_error() {
        let (root, d) = tree(
            "loop",
            &[(10, "a", 11), (11, "b", 10), (20, "c", 21), (40, "init", 0)],
        );
        assert_eq!(ancestry_strict(&root, 10), Err(WalkError::Loop(10)));
        assert_eq!(
            ancestry_strict(&root, 77),
            Err(WalkError::Unreadable(77)),
            "no stat: an error, not the pid alone"
        );
        assert_eq!(
            ancestry_strict(&root, 20),
            Err(WalkError::Unreadable(21)),
            "a parent's stat missing"
        );
        assert_eq!(ancestry_strict(&root, 40), Ok(vec![40]), "ppid 0 ends it");
        // A chain of 70: 1000 -> 1001 -> … -> 1069 -> 1.
        let chain: Vec<(u32, String, u32)> = (1000..1070)
            .map(|p| (p, "x".to_string(), if p == 1069 { 1 } else { p + 1 }))
            .collect();
        let rows: Vec<(u32, &str, u32)> = chain
            .iter()
            .map(|(p, c, pp)| (*p, c.as_str(), *pp))
            .collect();
        let (deep, d2) = tree("deep", &rows);
        assert_eq!(ancestry_strict(&deep, 1000), Err(WalkError::TooDeep));
        assert_eq!(ancestry_strict(&deep, 1010).map(|v| v.len()), Ok(60));
        act::remove_path(&d, false).unwrap();
        act::remove_path(&d2, false).unwrap();
    }
}
