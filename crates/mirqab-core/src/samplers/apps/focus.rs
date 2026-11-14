//! "Idle but heavy" (plan 006, part 4, decisions 5 and 6): a group that
//! used at least 20 % of one core on average over a full five minutes in
//! which none of its processes had focus. The focus comes from
//! `crate::desktop`; this file maps focused pids to groups and keeps each
//! group's CPU over the window, while the stream has subscribers.

use super::group::{self, Group};
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

/// The groups (indices into `groups`) a focus on any of `focused` credits:
/// the group that holds the pid, every group with a member in the same
/// host unit (a terminal's scope and the `session:` groups carved from
/// it: the user looking at a terminal has its jobs in view), and, for a
/// pid outside the user manager, every group with a member in its
/// session. A pid the walk does not know credits nothing.
pub fn credited(procs: &[Proc], groups: &[Group], focused: &[u32]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for &pid in focused {
        let Some(f) = procs.iter().position(|p| p.pid() == pid) else {
            continue;
        };
        let unit = unit_of(&procs[f]);
        let sid = procs[f].stat.sid;
        for (gi, g) in groups.iter().enumerate() {
            let hit = g.members.iter().any(|&m| {
                m == f
                    || match unit {
                        Some(u) => unit_of(&procs[m]) == Some(u),
                        None => sid > 0 && procs[m].stat.sid == sid,
                    }
            });
            if hit {
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
        // Outside the user manager: by session. 6201 runs in the login
        // session's scope, in session 6200 with its shell and worker.
        let dev = ids(&groups, &credited(&procs, &groups, &[6202]));
        assert!(!dev.is_empty());
        let shell_group = groups
            .iter()
            .position(|g| g.members.iter().any(|&m| procs[m].pid() == 6200))
            .unwrap();
        assert!(dev.contains(&groups[shell_group].id), "{dev:?}");
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
