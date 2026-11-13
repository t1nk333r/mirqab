//! `mirqab-ui`: the GTK4/libadwaita window (architecture §7). A stub until
//! phase D of plan 001 adds the toolkit.

fn main() {
    println!("mirqab-ui {}", mirqab_view::proto::VERSION);
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_a_version() {
        assert!(!mirqab_view::proto::VERSION.is_empty());
    }
}
