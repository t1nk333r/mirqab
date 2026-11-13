//! The protocol shared by the daemon and the interface: frames, error codes,
//! stream data shapes and every user-visible string (architecture §5.2, §7.2).
//! It depends on `serde` only, so the interface links no SQLite or D-Bus.

pub mod code;
pub mod frame;
pub mod stream;
pub mod strings;

pub use code::Code;
pub use frame::{ErrorBody, Event, Hello, Reply, Request, ServerFrame};

/// The protocol version in the `hello` frame. It changes only when a frame
/// shape changes (architecture §5.2).
pub const PROTOCOL: u32 = 1;

/// The crate version, reported in `hello` and `status`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn protocol_is_one() {
        assert_eq!(super::PROTOCOL, 1);
    }
}
