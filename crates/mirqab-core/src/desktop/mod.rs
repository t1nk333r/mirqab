//! What the compositor tells the daemon (plan 006, part 4): which window
//! has focus, for the Apps page's "idle but heavy" tag. One source today,
//! Hyprland's event socket (`hyprland.rs`), behind the `Focus` trait;
//! sway and niri have event streams of their own shape, and the trait is
//! the seam for them. Not to be confused with `samplers/apps/desktop.rs`,
//! which reads desktop entries.
//!
//! A focus source runs only while the `apps` stream has a subscriber: the
//! sampler starts it with its first sample and stops it when the stream is
//! left without subscribers (`Sampler::idle`). With the window closed
//! there is no reader thread and no open socket.

pub mod hyprland;

use crate::root::Root;
use std::path::PathBuf;
use std::time::Instant;

/// The focus now, and the pids that gained focus since a moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedSince {
    /// The pid of the focused window's client, if any.
    pub now: Option<u32>,
    /// Pids that had focus at any moment at or after the moment asked
    /// about (one that gained it before and held it into the window
    /// counts), oldest first, each once.
    pub since: Vec<u32>,
}

impl FocusedSince {
    /// Every pid in it: the focus now and the ones since.
    pub fn pids(&self) -> Vec<u32> {
        let mut out = self.since.clone();
        if let Some(p) = self.now
            && !out.contains(&p)
        {
            out.push(p);
        }
        out
    }
}

/// The focus over `[since, now]` from a history of changes, oldest first
/// (`Some(pid)`: a window of that client, `None`: no window; an entry
/// holds until the next): every pid whose time with focus overlaps the
/// window, including the last entry before `since` (it held focus into
/// the window), and the focus now. One rule for every source and for
/// the tests' fake, so they cannot drift (review of PR #20, B1): a tag
/// needs the whole window since focus was lost.
pub fn focused_since(history: &[(Instant, Option<u32>)], since: Instant) -> FocusedSince {
    let first_inside = history.partition_point(|(t, _)| *t < since);
    let mut pids: Vec<u32> = Vec::new();
    for (_, p) in &history[first_inside.saturating_sub(1)..] {
        if let Some(p) = p
            && !pids.contains(p)
        {
            pids.push(*p);
        }
    }
    FocusedSince {
        now: history.last().and_then(|(_, p)| *p),
        since: pids,
    }
}

/// A source of window focus.
pub trait Focus: Send {
    /// Starts watching. Ok names the source; Err says why there is none
    /// (no compositor instance found, or it did not answer).
    fn start(&mut self, root: &Root, own_uid: u32) -> Result<&'static str, String>;
    /// The pid focused now and the pids focused since `since`; None when no
    /// source runs (never started, not found, or its reader ended).
    fn focused(&mut self, since: Instant) -> Option<FocusedSince>;
    /// The running source's name, as `start` gave it; None when none runs.
    fn source(&self) -> Option<&'static str>;
    /// Stops watching: the reader ends and its sockets close.
    fn stop(&mut self);
}

/// The runtime directory as a path under `root`: on the live machine
/// `$XDG_RUNTIME_DIR` without its leading `/` (the root is `/`), else
/// `run/user/<uid>`. On a fixture root the environment is never read, so
/// a test daemon finds the fake compositor sockets inside its own tree
/// and never the live session's (plan 006, part 4).
pub fn runtime_rel(root: &Root, own_uid: u32) -> Option<String> {
    if root.is_host()
        && let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty())
    {
        let dir = dir.to_str()?;
        if let Some(rel) = dir.strip_prefix('/') {
            return Some(rel.trim_end_matches('/').to_string());
        }
    }
    Some(format!("run/user/{own_uid}"))
}

/// The runtime directory as a real path (the compositor probe of
/// `samplers/apps/actions.rs` and the Hyprland reader).
pub fn runtime_dir(root: &Root, own_uid: u32) -> Option<PathBuf> {
    root.path(&runtime_rel(root, own_uid)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_root_has_its_own_runtime_dir() {
        let r = Root::new("/tmp/mirqab-fixture");
        assert_eq!(runtime_rel(&r, 1000).as_deref(), Some("run/user/1000"));
        assert_eq!(
            runtime_dir(&r, 1000).unwrap(),
            PathBuf::from("/tmp/mirqab-fixture/run/user/1000")
        );
    }

    #[test]
    fn focus_counts_for_any_overlap_with_the_window() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + std::time::Duration::from_secs(s);
        let h = [
            (at(0), Some(1)),
            (at(10), Some(2)),
            (at(20), None),
            (at(30), Some(3)),
        ];
        assert_eq!(focused_since(&h, at(5)).since, [1, 2, 3], "1 held into it");
        assert_eq!(focused_since(&h, at(15)).since, [2, 3]);
        assert_eq!(
            focused_since(&h, at(25)).since,
            [3],
            "no window credits none"
        );
        let late = focused_since(&h, at(40));
        assert_eq!((late.now, late.pids()), (Some(3), vec![3]));
        let none = focused_since(&[], at(0));
        assert_eq!(none, FocusedSince::default());
        let blank = focused_since(&[(at(0), Some(1)), (at(10), None)], at(20));
        assert_eq!((blank.now, blank.pids()), (None, vec![]));
    }

    #[test]
    fn pids_join_the_focus_now_and_since() {
        let f = FocusedSince {
            now: Some(3),
            since: vec![1, 3],
        };
        assert_eq!(f.pids(), [1, 3]);
        let g = FocusedSince {
            now: Some(4),
            since: vec![1],
        };
        assert_eq!(g.pids(), [1, 4]);
    }
}
