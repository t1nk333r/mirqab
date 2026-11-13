//! `mirqab`: the daemon, the CLI and later the MCP server and tray
//! (architecture §3). Phase B of plan 001 adds the daemon and the commands.

fn main() {
    println!("mirqab {}", mirqab_core::proto::VERSION);
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_a_version() {
        assert!(!mirqab_core::proto::VERSION.is_empty());
    }
}
