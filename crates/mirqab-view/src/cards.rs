//! The card registry: every card a tab offers and Home can pin
//! (architecture §7.1). Empty until plan 003; each tab's plan adds its
//! cards, and Home builds a pinned card from its entry here.

/// A card kind: the id stored in `home.pinned`, the tab that owns it and the
/// stream it subscribes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    pub id: &'static str,
    pub tab: &'static str,
    pub stream: &'static str,
}

const fn system(id: &'static str) -> Card {
    Card {
        id,
        tab: "system",
        stream: "system",
    }
}

/// Every card. Plan 002 adds the System tab's; plan 003 builds them on Home.
pub const CARDS: &[Card] = &[
    system("system.overview"),
    system("system.cpu"),
    system("system.memory"),
    system("system.pressure"),
    system("system.gpu"),
    system("system.temps"),
    system("system.power"),
];

pub fn find(id: &str) -> Option<&'static Card> {
    CARDS.iter().find(|c| c.id == id)
}

/// The cards of `home.pinned` that exist, in the user's order. Unknown ids
/// (a card a later version removed) are skipped, never an error.
pub fn pinned(cards: &'static [Card], ids: &[String]) -> Vec<&'static Card> {
    ids.iter()
        .filter_map(|id| cards.iter().find(|c| c.id == id))
        .collect()
}
