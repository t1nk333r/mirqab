//! Home (plan 003): the pinned cards, the first-run seed, reordering, and
//! the streams Home subscribes to.

use crate::cards::{self, Card, Size};

/// What a first run pins (plan 003). Later plans add their headline card.
pub const SEED: &[&str] = &["system.overview", "system.cpu", "system.memory"];

/// The list to pin on a first run: `SEED` when nothing is pinned and Home
/// was never seeded; None otherwise, so an emptied Home stays empty.
pub fn seed(pinned: &[String], seeded: bool) -> Option<Vec<String>> {
    (!seeded && pinned.is_empty()).then(|| SEED.iter().map(|s| s.to_string()).collect())
}

/// `id` moved one place up (or down); unchanged at an end or when absent.
pub fn moved(pinned: &[String], id: &str, up: bool) -> Vec<String> {
    let mut out = pinned.to_vec();
    if let Some(i) = out.iter().position(|p| p == id) {
        let j = if up {
            i.checked_sub(1)
        } else {
            (i + 1 < out.len()).then_some(i + 1)
        };
        if let Some(j) = j {
            out.swap(i, j);
        }
    }
    out
}

/// `id` dropped onto `onto`: it takes `onto`'s place, and the cards
/// between shift by one. Unchanged when either is absent.
pub fn dropped(pinned: &[String], id: &str, onto: &str) -> Vec<String> {
    let mut out = pinned.to_vec();
    let (Some(i), Some(j)) = (
        out.iter().position(|p| p == id),
        out.iter().position(|p| p == onto),
    ) else {
        return out;
    };
    let item = out.remove(i);
    out.insert(j, item);
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
