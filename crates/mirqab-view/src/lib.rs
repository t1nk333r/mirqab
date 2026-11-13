//! View models with no GTK: formatting, theme tokens, glyphs, graph
//! geometry, the sidebar and the card registry; later one module per tab
//! (architecture §7.2). Everything here is tested without a display
//! (architecture §7.5, layer 1).

pub use mirqab_proto as proto;

pub mod cards;
pub mod config;
pub mod format;
pub mod graph;
pub mod icons;
pub mod shell;
pub mod tabs;
pub mod theme;
