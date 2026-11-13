//! One module per tab (architecture §7.2). A tab whose plan has not landed
//! shows the placeholder naming that plan. Pinnable cards are built from
//! one table here, by id, both in their tab and on Home (plan 003).

pub mod home;
pub mod placeholder;
pub mod settings;
pub mod system;

use crate::app::Ctx;
use crate::widgets::card::Card;
use serde_json::Value;
use std::rc::Rc;

/// A card that can stand alone: in its tab, or pinned on Home.
pub trait CardWidget {
    fn card(&self) -> &Card;
    /// An event of one of the card's streams.
    fn update(&self, stream: &str, data: &Value);
}

/// Every card id the table below builds. A test checks it against
/// `mirqab_view::cards::all()`.
pub const CARD_IDS: &[&str] = &[
    "system.overview",
    "system.cpu",
    "system.memory",
    "system.pressure",
    "system.gpu",
    "system.temps",
    "system.power",
];

/// Builds the card `id`; `compact` on Home (no range picker, shorter
/// graphs).
pub fn card(id: &str, ctx: &Rc<Ctx>, compact: bool) -> Option<Rc<dyn CardWidget>> {
    use system::*;
    Some(match id {
        "system.overview" => Rc::new(overview_strip::OverviewStrip::new(ctx)),
        "system.cpu" => Rc::new(cpu_card::CpuCard::new(ctx, compact)),
        "system.memory" => Rc::new(memory_card::MemoryCard::new(ctx, compact)),
        "system.pressure" => Rc::new(pressure_card::PressureCard::new(ctx, compact)),
        "system.gpu" => Rc::new(gpu_card::GpuCards::new(ctx, compact)),
        "system.temps" => Rc::new(temps_card::TempsCard::new(ctx)),
        "system.power" => Rc::new(power_card::PowerCard::new(ctx, compact)),
        _ => return None,
    })
}

/// Builds the page for `tab` (any id `pages::show` accepts). A page with a
/// stream calls `ctx.page_ready` on its first data; the others are ready
/// at once.
pub fn build(tab: &str, ctx: &Rc<Ctx>) -> gtk::Widget {
    match tab {
        "home" => home::page::build(ctx),
        "system" => system::page::build(ctx),
        "settings" => {
            let w = settings::page::build(ctx);
            ctx.page_ready(tab);
            w
        }
        other => {
            let w = placeholder::build(other);
            ctx.page_ready(other);
            w
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_constructor_table_matches_the_card_registry() {
        let mut registry: Vec<&str> = mirqab_view::cards::all().map(|c| c.id).collect();
        let mut table = super::CARD_IDS.to_vec();
        registry.sort();
        table.sort();
        assert_eq!(table, registry);
    }
}
