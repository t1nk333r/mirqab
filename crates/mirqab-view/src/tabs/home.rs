//! Home (plan 003): the pinned cards, the first-run seed, reordering, and
//! the streams Home subscribes to.

use crate::cards::{self, Card, Size};

/// What a first run pins (plan 003). Later plans add their headline card:
/// plan 006 the five heaviest apps.
pub const SEED: &[&str] = &["system.overview", "system.cpu", "system.memory", "apps.top"];

/// The list to pin on a first run: `SEED` when nothing is pinned and Home
/// was never seeded; None otherwise, so an emptied Home stays empty.
pub fn seed(pinned: &[String], seeded: bool) -> Option<Vec<String>> {
    (!seeded && pinned.is_empty()).then(|| SEED.iter().map(|s| s.to_string()).collect())
}

/// Whether Home counts as seeded already: something is pinned (from a tab,
/// before Home first opened) though the flag is unset. Otherwise emptying
/// Home later would refill it (review of PR #9).
pub fn mark_seeded(pinned: &[String], seeded: bool) -> bool {
    !seeded && !pinned.is_empty()
}

/// `id` moved one place up (or down) among the cards Home shows: it swaps
/// with the nearest card on that side that Home puts on screen (in
/// `shown`, and that id's first entry), passing over pinned ids it does
/// not show (unknown, unserved, or a repeat), which would otherwise make
/// the move look like nothing (audit 2026-11-21, L51). Unchanged at an end
/// or when absent.
pub fn moved(pinned: &[String], shown: &[&str], id: &str, up: bool) -> Vec<String> {
    let mut out = pinned.to_vec();
    let on_screen = |k: usize| {
        shown.contains(&pinned[k].as_str())
            && pinned.iter().position(|p| *p == pinned[k]) == Some(k)
    };
    if let Some(i) = pinned.iter().position(|p| p == id) {
        let j = if up {
            (0..i).rev().find(|&k| on_screen(k))
        } else {
            (i + 1..pinned.len()).find(|&k| on_screen(k))
        };
        if let Some(j) = j {
            out.swap(i, j);
        }
    }
    out
}

/// `id` dropped onto `onto`: it takes `onto`'s place, and the cards
/// between shift by one. Repeats of `id`, which Home never shows, fold
/// into the one moved entry; a repeat left behind would become the shown
/// one and the card would seem to move one place (plan 024, L34, as audit
/// 2026-11-21, L51 for `moved`). Unchanged when either is absent.
pub fn dropped(pinned: &[String], id: &str, onto: &str) -> Vec<String> {
    let (Some(i), Some(j)) = (
        pinned.iter().position(|p| p == id),
        pinned.iter().position(|p| p == onto),
    ) else {
        return pinned.to_vec();
    };
    if i == j {
        return pinned.to_vec();
    }
    let mut out: Vec<String> = pinned.iter().filter(|p| *p != id).cloned().collect();
    let k = out.iter().position(|p| p == onto).unwrap_or(out.len());
    // Moving forward it lands after `onto`, which shifted back by one.
    out.insert(if i < j { k + 1 } else { k }, id.to_string());
    out
}

/// `id` pinned or unpinned; a new pin goes last.
pub fn toggled(pinned: &[String], id: &str, on: bool) -> Vec<String> {
    let mut out: Vec<String> = pinned.iter().filter(|p| *p != id).cloned().collect();
    if on {
        out.push(id.to_string());
    }
    out
}

/// The cards Home shows, among those the daemon can serve: a card whose
/// stream is not in `capabilities` is skipped, not unpinned.
pub fn shown(pinned: &[String], capabilities: &[String]) -> Vec<&'static Card> {
    cards::pinned(pinned)
        .into_iter()
        .filter(|c| {
            c.streams
                .iter()
                .all(|s| capabilities.iter().any(|cap| cap == s))
        })
        .collect()
}

/// The cards Home puts on screen: those `shown` lets through, or, before
/// the daemon has said what it serves (`capabilities` empty), every known
/// pinned card.
pub fn visible(pinned: &[String], capabilities: &[String]) -> Vec<&'static Card> {
    if capabilities.is_empty() {
        cards::pinned(pinned)
    } else {
        shown(pinned, capabilities)
    }
}

/// Every stream the shown cards need, once each, in order.
pub fn streams(cards: &[&Card]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for c in cards {
        for s in c.streams {
            if !out.contains(s) {
                out.push(s);
            }
        }
    }
    out
}

/// Where each card goes on Home's two columns: (row, column, span). A wide
/// card takes a row of its own; half cards fill rows two by two.
pub fn layout(cards: &[&Card]) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    let (mut row, mut col) = (0u32, 0u32);
    for c in cards {
        match c.size {
            Size::Wide => {
                if col != 0 {
                    row += 1;
                }
                out.push((row, 0, 2));
                row += 1;
                col = 0;
            }
            Size::Half => {
                out.push((row, col, 1));
                if col == 1 {
                    row += 1;
                    col = 0;
                } else {
                    col = 1;
                }
            }
        }
    }
    out
}
