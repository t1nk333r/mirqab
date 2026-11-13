//! `mirqab-fand`: the opt-in root helper for fans (architecture §8.1).
//! A stub until plan 015.

fn main() {
    println!("mirqab-fand {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_a_version() {
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }
}
