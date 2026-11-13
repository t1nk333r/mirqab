//! View models with no GTK: formatting, theme tokens, graph geometry, the
//! sidebar and one module per tab (architecture §7.2). Phase D of plan 001
//! fills it; it is tested without a display.

pub use mirqab_proto as proto;

#[cfg(test)]
mod tests {
    #[test]
    fn links_the_protocol() {
        assert!(!super::proto::VERSION.is_empty());
    }
}
