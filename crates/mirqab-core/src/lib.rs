//! The daemon's library: root, exec, act, server, samplers and history
//! (architecture §5, §6). Phases B and C of plan 001 fill it.

pub use mirqab_proto as proto;

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_the_protocol() {
        assert_eq!(super::proto::PROTOCOL, 1);
    }
}
