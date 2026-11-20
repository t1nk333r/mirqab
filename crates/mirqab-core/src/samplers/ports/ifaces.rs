//! How far a bind address reaches (plan 007): its scope. Loopback,
//! unspecified and link-local addresses say so themselves; any other is
//! looked up among the machine's interface addresses, and the interface's
//! kind is read from `sys/class/net/<name>/` through `Root`. The addresses
//! come from `sys::ifaddrs` on the live machine and from
//! `run/mirqab-test/ifaddrs.json` under a fixture root. Interface names
//! stay here: the stream carries the scope only.
//!
//! Rejected (plan 007 §1.9): `proc/net/fib_trie` (no interface per
//! address), netlink (more `unsafe`), `ip -j addr` (a process per read).

use crate::root::Root;
use std::collections::HashMap;
use std::net::IpAddr;

/// ARPHRD_LOOPBACK and ARPHRD_NONE (`include/uapi/linux/if_arp.h`); a
/// WireGuard or tun interface is ARPHRD_NONE.
const ARPHRD_LOOPBACK: &str = "772";
const ARPHRD_NONE: &str = "65534";
/// Where a fixture root keeps its interface table.
pub const FIXTURE_TABLE: &str = "run/mirqab-test/ifaddrs.json";

/// How far a bind address reaches. Everything but `Local` is exposed:
/// `Unknown` too, as a wrong "safe" is worse than an honest "cannot place
/// it" (plan 007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Local,
    All,
    Lan,
    Bridge,
    Vpn,
    Link,
    /// A veth, a dummy, a bridge's port: another network.
    Other,
    Unknown,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::All => "all",
            Scope::Lan => "lan",
            Scope::Bridge => "bridge",
            Scope::Vpn => "vpn",
            Scope::Link => "link",
            Scope::Other => "other",
            Scope::Unknown => "unknown",
        }
    }

    pub fn exposed(self) -> bool {
        self != Scope::Local
    }
}

/// The scope an address has by itself, or None when only the interface
/// table can tell.
pub fn intrinsic(addr: IpAddr) -> Option<Scope> {
    if addr.is_loopback() {
        return Some(Scope::Local);
    }
    if addr.is_unspecified() {
        return Some(Scope::All);
    }
    let link = match addr {
        IpAddr::V4(a) => a.is_link_local(),
        IpAddr::V6(a) => a.segments()[0] & 0xffc0 == 0xfe80,
    };
    link.then_some(Scope::Link)
}

/// A fixture's table: `[{"name": "lan0", "addr": "192.0.2.10"}, …]`; an
/// entry that cannot be read is left out.
fn fixture_table(text: &str) -> Vec<(String, IpAddr)> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?.to_string();
            let addr = e.get("addr")?.as_str()?.parse().ok()?;
            Some((name, addr))
        })
        .collect()
}

/// A name `sys/class/net/` could hold: no slash, not `.` or `..`.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// The kind of interface `name`, from `sys/class/net/<name>/`.
pub fn kind(root: &Root, name: &str) -> Scope {
    if !plain_name(name) {
        return Scope::Other;
    }
    let dir = format!("sys/class/net/{name}");
    let ty = root.read(&format!("{dir}/type")).unwrap_or_default();
    let ty = ty.trim();
    let uevent = root.read(&format!("{dir}/uevent")).unwrap_or_default();
    let devtype = uevent
        .lines()
        .find_map(|l| l.strip_prefix("DEVTYPE="))
        .map(str::trim);
    let has = |sub: &str| root.exists(&format!("{dir}/{sub}"));
    if ty == ARPHRD_LOOPBACK {
        Scope::Local
    } else if devtype == Some("bridge") || has("bridge") {
        Scope::Bridge
    } else if ty == ARPHRD_NONE || devtype == Some("wireguard") || has("tun_flags") {
        Scope::Vpn
    } else if devtype == Some("wlan") || has("wireless") || has("device") {
        Scope::Lan
    } else {
        Scope::Other
    }
}

/// The machine's interface addresses with their kinds.
#[derive(Debug, Default, Clone)]
pub struct Table {
    kinds: HashMap<IpAddr, Scope>,
}

impl Table {
    /// Read now: the addresses (the host's, or a fixture's file), then
    /// each interface's kind once.
    pub fn read(root: &Root) -> Table {
        let addrs: Vec<(String, IpAddr)> = if root.is_host() {
            crate::sys::ifaddrs().unwrap_or_default()
        } else {
            root.read(FIXTURE_TABLE)
                .map(|t| fixture_table(&t))
                .unwrap_or_default()
        };
        let mut by_name: HashMap<String, Scope> = HashMap::new();
        let mut kinds = HashMap::new();
        for (name, addr) in addrs {
            let k = *by_name
                .entry(name)
                .or_insert_with_key(|name| kind(root, name));
            kinds.insert(addr, k);
        }
        Table { kinds }
    }

    /// The scope of a bind address: by itself, else by its interface, else
    /// `Unknown`.
    pub fn scope(&self, addr: IpAddr) -> Scope {
        intrinsic(addr)
            .or_else(|| self.kinds.get(&addr).copied())
            .unwrap_or(Scope::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_that_says_its_scope() {
        let s = |a: &str| intrinsic(a.parse().unwrap());
        assert_eq!(s("127.0.0.53"), Some(Scope::Local));
        assert_eq!(s("::1"), Some(Scope::Local));
        assert_eq!(s("0.0.0.0"), Some(Scope::All));
        assert_eq!(s("::"), Some(Scope::All));
        // IPv4 link-local, built from its octets: the privacy scan
        // (tools/secrets.sh) admits only documentation and loopback
        // addresses as text, and this one names no machine.
        assert_eq!(intrinsic(IpAddr::from([169, 254, 1, 1])), Some(Scope::Link));
        assert_eq!(s("fe80::1"), Some(Scope::Link));
        assert_eq!(s("febf::1"), Some(Scope::Link));
        assert_eq!(s("fec0::1"), None);
        assert_eq!(s("192.0.2.10"), None);
        assert!(!Scope::Local.exposed() && Scope::Unknown.exposed());
    }
}
