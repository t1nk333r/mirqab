//! "Idle but heavy" (plan 006, part 4, decisions 5 and 6): a group that
//! used at least 20 % of one core on average over a full five minutes in
//! which none of its processes had focus. The focus comes from
//! `crate::desktop`; this file maps focused pids to groups and keeps each
//! group's CPU over the window, while the stream has subscribers.

use super::group::{self, Group, Kind};
use super::procfs::Proc;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// The window a group must stay busy and unfocused for.
pub const HEAVY_WINDOW: Duration = Duration::from_secs(300);
/// The mean CPU over the window, in percent of one core, as a row prints
/// it: a browser at 25 % of one of sixteen cores is tagged.
pub const HEAVY_CPU: f64 = 20.0;

/// The tag's two numbers; tests shrink the window to seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub window: Duration,
    pub cpu: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            window: HEAVY_WINDOW,
            cpu: HEAVY_CPU,
        }
    }
}

/// The host unit a process is in, when it is a scope or a service under
/// the user manager (never a slice: every app shares `app.slice`).
fn unit_of(p: &Proc) -> Option<&str> {
    p.info
        .cgroup
        .as_deref()
        .and_then(|c| group::host_units(c).last().copied())
        .filter(|u| !u.ends_with(".slice"))
}

/// An app scope in systemd's sense: `app[-<launcher>]-<id>-<random>.scope`
/// (https://systemd.io/DESKTOP_ENVIRONMENTS/), one per launched app.
fn app_scope(unit: &str) -> bool {
    unit.starts_with("app-") && unit.ends_with(".scope")
}

/// The groups (indices into `groups`) a focus on any of `focused` credits
/// (owner, 2026-11-15, narrowing decision 5 after the review of PR #20,
/// S4):
///
/// - the group that holds the pid, always;
/// - when that group is a terminal's `session:` split (a window started
///   from a shell), nothing more: the other tabs are not in view;
/// - otherwise, when the pid's unit is an `app-*.scope`, every group with
///   a member in that scope (a terminal's scope and the `session:` groups
///   carved from it: the user looking at a terminal has its jobs in view);
/// - otherwise, when the pid is a terminal (a terminal server in a
///   service), the `session:` groups carved from its unit;
/// - nothing else. Apps that merely share a service (the compositor's or
///   another), the manager's `init.scope` or the login session's scope
///   get no credit from each other's windows: one focused window there
///   would otherwise keep every group in it from ever being tagged.
///
/// A pid the walk does not know credits nothing.
pub fn credited(procs: &[Proc], groups: &[Group], focused: &[u32]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for &pid in focused {
        let Some(f) = procs.iter().position(|p| p.pid() == pid) else {
            continue;
        };
        let Some(own) = groups.iter().position(|g| g.members.contains(&f)) else {
            continue;
        };
        out.insert(own);
        if groups[own].kind == Kind::Terminal {
            continue;
        }
        let Some(unit) = unit_of(&procs[f]) else {
            continue;
        };
        let terminal = group::is_terminal(&procs[f].stat.comm);
        let wide = app_scope(unit);
        if !wide && !terminal {
            continue;
        }
        for (gi, g) in groups.iter().enumerate() {
            let in_unit = g.members.iter().any(|&m| unit_of(&procs[m]) == Some(unit));
            if in_unit && (wide || g.kind == Kind::Terminal) {
                out.insert(gi);
            }
        }
    }
    out
}

/// Each group's CPU per sample over the window, by group id.
#[derive(Debug, Default)]
pub struct Windows {
    by_id: HashMap<String, VecDeque<(Instant, f64)>>,
}

impl Windows {
    /// Adds a sample's CPU for each group (`(id, cpu)`; a group with no
    /// figure yet adds nothing). A group missing from the sample is
    /// forgotten: its processes went, and one that comes back starts
    /// afresh. One point at or before the window's start is kept, so it
    /// can be told whether the window is full.
    pub fn record<'a>(
        &mut self,
        now: Instant,
        groups: impl Iterator<Item = (&'a str, Option<f64>)>,
        window: Duration,
    ) {
        let mut seen = HashSet::new();
        for (id, cpu) in groups {
            seen.insert(id.to_string());
            if let Some(c) = cpu {
                self.by_id
                    .entry(id.to_string())
                    .or_default()
                    .push_back((now, c));
            }
        }
        self.by_id.retain(|id, _| seen.contains(id));
        let Some(start) = now.checked_sub(window) else {
            return;
        };
        for v in self.by_id.values_mut() {
            while v.len() >= 2 && v[1].0 <= start {
                v.pop_front();
            }
        }
    }

    /// Whether `id` was at least `th.cpu` on average over a full window
    /// ending `now`: its first point is at or before the window's start,
    /// and the mean is over the points inside it.
    pub fn heavy(&self, id: &str, now: Instant, th: Thresholds) -> bool {
        let Some(v) = self.by_id.get(id) else {
            return false;
        };
        let Some(start) = now.checked_sub(th.window) else {
            return false;
        };
        if v.len() < 2 || v[0].0 > start {
            return false;
        }
        let inside: Vec<f64> = v.iter().skip(1).map(|(_, c)| *c).collect();
        inside.iter().sum::<f64>() / inside.len() as f64 >= th.cpu
    }

    pub fn clear(&mut self) {
        self.by_id.clear();
    }

    /// How many groups have a window (tests).
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::Root;
    use crate::samplers::apps::desktop::Desktop;
    use crate::samplers::apps::procfs::{self, Cache};

    fn fixture() -> (Vec<Proc>, Vec<Group>) {
        let root = Root::new(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/root-desktop"),
        );
        let procs = procfs::walk(&root, &mut Cache::default(), 1000);
        let groups = group::group(&procs, 1000, &Desktop::default());
        (procs, groups)
    }

    fn ids(groups: &[Group], set: &HashSet<usize>) -> Vec<String> {
        let mut v: Vec<String> = set.iter().map(|&i| groups[i].id.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn a_terminal_credits_its_jobs_and_a_browser_only_itself() {
        let (procs, groups) = fixture();
        let term = ids(&groups, &credited(&procs, &groups, &[3000]));
        assert!(term.contains(&"session:3030".to_string()), "{term:?}");
        assert!(term.contains(&"session:3010".to_string()), "{term:?}");
        assert!(
            term.iter()
                .any(|i| i.contains("app-Hyprland-foot-3e4f5a6b")),
            "{term:?}"
        );
        assert!(
            !term.iter().any(|i| i.contains("browser")),
            "the browser is not in view: {term:?}"
        );
        assert_eq!(
            ids(&groups, &credited(&procs, &groups, &[2001])),
            ["scope:app-Hyprland-browser.sh-5650fedc"]
        );
        assert!(credited(&procs, &groups, &[99999]).is_empty());
        // Outside the user manager only the group that holds the pid: 6202
        // runs in the login session's scope, in session 6200, whose shell
        // sits in the compositor's tree; the compositor is not credited.
        assert_eq!(
            ids(&groups, &credited(&procs, &groups, &[6202])),
            ["tree:6201"]
        );
        // A job's own window credits its split, not the terminal or the
        // other tabs.
        assert_eq!(
            ids(&groups, &credited(&procs, &groups, &[3041])),
            ["session:3030"]
        );
    }

    /// A synthetic table, for shapes `root-desktop` does not hold: `(pid,
    /// ppid, comm, sid, cgroup)`, uid 1000, `~` the user manager.
    fn table(rows: &[(u32, u32, &str, u32, &str)]) -> Vec<Proc> {
        let manager = "/user.slice/user-1000.slice/user@1000.service";
        rows.iter()
            .map(|&(pid, ppid, comm, sid, cg)| Proc {
                stat: procfs::Stat {
                    pid,
                    comm: comm.to_string(),
                    state: 'S',
                    ppid,
                    pgrp: pid,
                    sid,
                    tpgid: -1,
                    flags: 0,
                    ticks: 0,
                    threads: 1,
                    start: u64::from(pid),
                },
                mem: None,
                io: None,
                info: std::sync::Arc::new(procfs::Info {
                    uid: Some(1000),
                    cgroup: Some(cg.replacen('~', manager, 1)),
                    cmdline: vec![comm.to_string()],
                    flatpak: None,
                }),
            })
            .collect()
    }

    #[test]
    fn apps_sharing_a_service_or_the_session_scope_credit_only_themselves() {
        let session = "/user.slice/user-1000.slice/session-2.scope";
        let term = "~/app.slice/example-term.service";
        let wm = "~/session.slice/wayland-wm@example.service";
        let procs = table(&[
            (1100, 1, "systemd", 1100, "~/init.scope"),
            // The login session's scope: a compositor and two apps it
            // started through `sh -c`, all in its session.
            (1050, 1, "examplewm", 1050, session),
            (1060, 1050, "sh", 1050, session),
            (1061, 1060, "chat", 1050, session),
            (1070, 1050, "sh", 1050, session),
            (1071, 1070, "player", 1050, session),
            // A non-app service holding a terminal server and two apps run
            // from its shells: `viewer` in one tab, `build` in another.
            (2000, 1100, "foot", 2000, term),
            (2010, 2000, "zsh", 2010, term),
            (2011, 2010, "viewer", 2010, term),
            (2020, 2000, "zsh", 2020, term),
            (2021, 2020, "build", 2020, term),
            // The compositor's own service with the apps it started without
            // `uwsm app`: one group, the unit's.
            (3000, 1100, "examplewm", 3000, wm),
            (3001, 3000, "mail", 3000, wm),
        ]);
        let groups = group::group(&procs, 1000, &Desktop::default());
        let credit = |pid| ids(&groups, &credited(&procs, &groups, &[pid]));
        let holding = |pid: u32| {
            groups
                .iter()
                .find(|g| g.members.iter().any(|&m| procs[m].pid() == pid))
                .unwrap()
                .id
                .clone()
        };
        assert_ne!(holding(1061), holding(1071), "two groups in one scope");
        assert_eq!(credit(1061), [holding(1061)], "not the player");
        assert_eq!(credit(1071), [holding(1071)], "not the chat");
        assert_eq!(credit(1050), [holding(1050)]);
        assert_eq!(holding(2011), "session:2010");
        assert_eq!(credit(2011), ["session:2010"], "not the build");
        assert_eq!(credit(2021), ["session:2020"], "not the viewer");
        // The terminal's own window: its jobs are in view.
        assert_eq!(
            credit(2000),
            ["service:example-term", "session:2010", "session:2020"]
        );
        assert_eq!(holding(3001), holding(3000), "one group: the service's");
        assert_eq!(credit(3001), [holding(3000)]);
    }

    #[test]
    fn a_window_needs_its_full_span_and_its_mean() {
        let th = Thresholds {
            window: Duration::from_secs(3),
            cpu: 20.0,
        };
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut w = Windows::default();
        w.record(
            at(2),
            [("a", Some(50.0)), ("b", Some(5.0))].into_iter(),
            th.window,
        );
        w.record(
            at(4),
            [("a", Some(50.0)), ("b", Some(5.0))].into_iter(),
            th.window,
        );
        assert!(!w.heavy("a", at(4), th), "not a full window yet");
        w.record(
            at(6),
            [("a", Some(30.0)), ("b", Some(5.0))].into_iter(),
            th.window,
        );
        assert!(w.heavy("a", at(6), th));
        assert!(!w.heavy("b", at(6), th), "under the mean");
        w.record(at(8), [("a", Some(0.0))].into_iter(), th.window);
        assert!(!w.heavy("a", at(8), th), "(30 + 0) / 2 is under 20");
        assert_eq!(w.len(), 1, "a group that left is forgotten");
        w.clear();
        assert!(w.is_empty());
    }
}
