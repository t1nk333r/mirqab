//! The kernel's socket tables, `proc/net/{tcp,tcp6,udp,udp6}` (plan 007):
//! the listening TCP sockets (state `0A`) and the bound UDP ones (`07`),
//! each with its local address, port, owner's uid and inode. Read through
//! `Root`, so a fixture tree stands for the machine.
//!
//! The format (`net/ipv4/tcp_ipv4.c`, `net/ipv6/tcp_ipv6.c`): a header
//! line, then `sl local rem st tx:rx tr:when retrnsmt uid timeout inode …`.
//! The port is big-endian hex. An address is printed as 32-bit words, each
//! with `%08X` of the `u32` as it lies in memory: one word for IPv4, four
//! in address order for IPv6. So each word's bytes, in memory order, are
//! `to_ne_bytes` of the parsed number: right on the live machine whatever
//! its byte order. The fixture rows were written as a little-endian
//! machine prints them, as every machine mirqab targets is (plan 007 §5.1,
//! read on 2026-11-20: `::1` is `00000000000000000000000001000000`).

use crate::root::Root;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A TCP socket in LISTEN (`include/net/tcp_states.h`).
const TCP_LISTEN: &str = "0A";
/// A UDP socket that is bound and not connected: the kernel reports it as
/// TCP_CLOSE.
const UDP_BOUND: &str = "07";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn name(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        }
    }
}

/// One row of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socket {
    pub proto: Proto,
    pub addr: IpAddr,
    pub port: u16,
    pub uid: u32,
    pub inode: u64,
}

/// One address word: eight hex digits, the bytes in memory order.
fn word(hex: &str) -> Option<[u8; 4]> {
    if hex.len() != 8 {
        return None;
    }
    // The kernel prints the u32 as it lies in memory with %08X, so its
    // native-endian bytes are the address bytes in order.
    Some(u32::from_str_radix(hex, 16).ok()?.to_ne_bytes())
}

/// An address of either table. A v4-mapped IPv6 address (`::ffff:a.b.c.d`)
/// is the IPv4 address: such a socket reaches IPv4 peers only.
pub fn address(hex: &str) -> Option<IpAddr> {
    match hex.len() {
        8 => Some(IpAddr::V4(Ipv4Addr::from(word(hex)?))),
        32 => {
            let mut bytes = [0u8; 16];
            for i in 0..4 {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&word(hex.get(i * 8..i * 8 + 8)?)?);
            }
            let v6 = Ipv6Addr::from(bytes);
            Some(match v6.to_ipv4_mapped() {
                Some(v4) => IpAddr::V4(v4),
                None => IpAddr::V6(v6),
            })
        }
        _ => None,
    }
}

/// The rows of one table's text that are in `state`, as `proto`. A line
/// that cannot be read is skipped, never a panic: the table is the
/// kernel's, but a fixture or a future column could surprise.
pub fn parse(text: &str, proto: Proto, state: &str) -> Vec<Socket> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.get(3) != Some(&state) {
                return None;
            }
            let (addr, port) = f.get(1)?.split_once(':')?;
            Some(Socket {
                proto,
                addr: address(addr)?,
                port: u16::from_str_radix(port, 16).ok()?,
                uid: f.get(7)?.parse().ok()?,
                inode: f.get(9)?.parse().ok()?,
            })
        })
        .collect()
}

/// Every listening TCP socket, and with `udp` every bound UDP socket, of
/// both families. A table that cannot be read gives nothing (a kernel
/// without IPv6 has no `tcp6`).
pub fn read(root: &Root, udp: bool) -> Vec<Socket> {
    let mut tables = vec![
        ("proc/net/tcp", Proto::Tcp, TCP_LISTEN),
        ("proc/net/tcp6", Proto::Tcp, TCP_LISTEN),
    ];
    if udp {
        tables.push(("proc/net/udp", Proto::Udp, UDP_BOUND));
        tables.push(("proc/net/udp6", Proto::Udp, UDP_BOUND));
    }
    let mut out = Vec::new();
    for (rel, proto, state) in tables {
        if let Ok(text) = root.read(rel) {
            out.extend(parse(&text, proto, state));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_decode_in_memory_order() {
        assert_eq!(address("0100007F"), Some("127.0.0.1".parse().unwrap()));
        assert_eq!(address("0A0200C0"), Some("192.0.2.10".parse().unwrap()));
        assert_eq!(
            address("00000000000000000000000001000000"),
            Some("::1".parse().unwrap())
        );
        assert_eq!(
            address("0000000000000000FFFF00000100007F"),
            Some("127.0.0.1".parse().unwrap())
        );
        assert_eq!(
            address("B80D0120000000000000000010000000"),
            Some("2001:db8::10".parse().unwrap())
        );
        assert_eq!(address("0100007"), None);
        assert_eq!(address("zz00007F"), None);
    }
}
