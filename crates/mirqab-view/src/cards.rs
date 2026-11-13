//! The card registry (plan 003): every card a tab offers and Home can pin,
//! registered once, as data, in its tab's view model. Home builds a pinned
//! card from its entry; the widget side keeps one constructor per id
//! (`crates/mirqab-ui/src/tabs/mod.rs`), and a test checks the two tables
//! name the same ids.

/// How much of Home's two columns a card takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// Both columns.
    Wide,
    /// One column.
    Half,
}

/// A card kind: the id stored in `home.pinned`, the tab that owns it, the
/// streams it subscribes to, and its size on Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    pub id: &'static str,
    pub tab: &'static str,
    pub streams: &'static [&'static str],
    pub size: Size,
}

/// Every card of every tab, in tab order.
pub fn all() -> impl Iterator<Item = &'static Card> {
    crate::tabs::system::CARDS.iter()
}

pub fn find(id: &str) -> Option<&'static Card> {
    all().find(|c| c.id == id)
}

/// The cards of `home.pinned` that exist, in the user's order. Unknown ids
/// (a card a later version removed) are skipped, never an error; an id
/// listed twice shows once, at its first place.
pub fn pinned(ids: &[String]) -> Vec<&'static Card> {
    let mut out: Vec<&'static Card> = Vec::new();
    for c in ids.iter().filter_map(|id| find(id)) {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(c);
        }
    }
    out
}
