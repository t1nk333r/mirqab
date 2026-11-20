//! The `ports` stream (plan 007): what listens on which port, which of the
//! user's processes holds it, and how far the network reaches it.
//!
//! Every 5 s while subscribed, never `live`. Each tick reads the kernel's
//! socket tables (`procnet.rs`), classifies each bind address (`ifaces.rs`)
//! and names the process of each socket of the user's (`owner.rs`). The map
//! from inode to process is rebuilt only when the set of the user's
//! listening inodes changed since the last tick, or a mapped process is
//! gone or is another one (a new start time); between rebuilds a tick reads
//! the tables and `stat` and `statm` of the few mapped processes. A `get`
//! rebuilds it. A row is a port, a protocol, a bind pair and an owner: for
//! the user's sockets, the tree of processes that hold it (a forked
//! server and its workers are one row, `pids` listing them, root first;
//! independent holders are rows of their own). While every subscriber is lite (the counts, for a badge, the
//! tray and a bar item) the tables are all that is read.
//!
//! The sampler is its own slot with its own lock and its own process
//! cache, so a tick never waits behind the `apps` walk. It takes no
//! action; the sender it is given is for part 1b's `docker ps` read.

pub mod ifaces;
pub mod owner;
pub mod procnet;

use crate::act::Sender;
use crate::proto::stream::{PORTS, PortRow, PortsData, PortsLite, SystemPorts};
use crate::proto::strings;
use crate::root::Root;
use crate::sampler::{Accepts, Sampler, View};
use crate::samplers::apps::{group, procfs};
use crate::selfcost;
use ifaces::{Scope, Table};
use procnet::{Proto, Socket};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long an interface table serves while the bind addresses stay the
/// same: addresses come and go (a VPN, a new lease) without a socket
/// changing.
const TABLE_MAX_AGE: Duration = Duration::from_secs(60);
/// The longest label, in characters.
const LABEL_MAX: usize = 40;

/// The stream's params: `udp` and `lite`, each a boolean, nothing else.
pub fn accepts(params: &Value) -> Result<(), String> {
    let ok = match params {
        Value::Null => true,
        Value::Object(o) => o
            .iter()
            .all(|(k, v)| matches!(k.as_str(), "udp" | "lite") && v.is_boolean()),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(strings::PORTS_PARAMS.to_string())
    }
}

fn flag(params: &Value, key: &str) -> bool {
    params.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// What a row says about a process: its name and the argument that says
/// what it serves. `next dev` for `node …/node_modules/.bin/next dev`,
/// `http.server` for `python3 -m http.server --bind 0.0.0.0 8000`,
/// `manage.py runserver`, `vite`. An interpreter's script (or `-m`
/// module) stands for the program; then the next argument joins it when it
/// is a word (a subcommand), not a flag, a number or a path. Never the
/// whole command line: arguments can hold secrets.
pub fn label(comm: &str, cmdline: &[String]) -> String {
    let name = procfs::display_name(comm, cmdline);
    let args = cmdline.get(1..).unwrap_or(&[]);
    let (base, rest): (String, &[String]) = if group::INTERPRETERS.contains(&comm) {
        let mut found = None;
        let mut i = 0;
        while let Some(a) = args.get(i) {
            match a.as_str() {
                "-m" => {
                    found = args
                        .get(i + 1)
                        .map(|m| (m.clone(), args.get(i + 2..).unwrap_or(&[])));
                    break;
                }
                // Code on the command line: the interpreter's name alone.
                "-c" | "-e" | "--eval" => break,
                f if f.starts_with('-') => i += 1,
                script => {
                    found = Some((
                        procfs::basename(script).to_string(),
                        args.get(i + 1..).unwrap_or(&[]),
                    ));
                    break;
                }
            }
        }
        found.unwrap_or((name, &[]))
    } else {
        (name, args)
    };
    let word = |a: &str| {
        let mut c = a.chars();
        c.next().is_some_and(|f| f.is_ascii_alphabetic())
            && c.all(|ch| ch.is_ascii_alphanumeric() || "._-".contains(ch))
    };
    let mut out = match rest.first().filter(|a| word(a)) {
        Some(sub) => format!("{base} {sub}"),
        None => base,
    };
    if out.chars().count() > LABEL_MAX {
        out = out.chars().take(LABEL_MAX).collect();
    }
    strings::plain(&out).into_owned()
}

/// `http://localhost:<port>`, `https` for the ports TLS usually takes. The
/// port is never probed.
fn url(port: u16) -> String {
    let scheme = if matches!(port, 443 | 8443) {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://localhost:{port}")
}

/// The user's processes that hold one listening socket and form one
/// tree: the root (a holder whose parent does not hold the socket) and
/// its descendants that hold it too, such as a forked server's workers.
#[derive(Debug, Clone)]
struct Holder {
    /// The root's pid and start time: the row's identity.
    pid: u32,
    start: u64,
    name: String,
    label: String,
    /// Every process of the tree that holds the socket, as (pid, start):
    /// the root first, then the rest by pid.
    members: Vec<(u32, u64)>,
    /// Resident memory summed over the members.
    rss: Option<u64>,
}

/// Sums what each member gave, or None when none gave anything.
fn sum(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values.fold(None, |acc, v| match (acc, v) {
        (None, v) => v,
        (Some(a), Some(b)) => Some(a + b),
        (a, None) => a,
    })
}

/// Groups the holders of one socket into trees by their parent links: a
/// holder whose parent does not hold the socket heads a tree, and every
/// other holder joins the tree its chain of holding parents leads to.
/// Trees come in their roots' pid order, each root first.
fn trees(holders: &[&procfs::Proc]) -> Vec<Vec<usize>> {
    let index: HashMap<u32, usize> = holders
        .iter()
        .enumerate()
        .map(|(i, p)| (p.pid(), i))
        .collect();
    let root_of = |mut i: usize| {
        // A loop in the parent links (pids reused mid-walk) cannot hang it.
        for _ in 0..64 {
            match index.get(&holders[i].stat.ppid) {
                Some(&parent) if parent != i => i = parent,
                _ => break,
            }
        }
        i
    };
    let mut by_root: Vec<(usize, Vec<usize>)> = Vec::new();
    for i in 0..holders.len() {
        let r = root_of(i);
        match by_root.iter_mut().find(|(root, _)| *root == r) {
            Some((_, members)) => members.push(i),
            None => by_root.push((r, vec![i])),
        }
    }
    by_root.sort_by_key(|(r, _)| holders[*r].pid());
    by_root
        .into_iter()
        .map(|(r, mut members)| {
            members.sort_by_key(|&i| (i != r, holders[i].pid()));
            members
        })
        .collect()
}

/// Which addresses of one owner and port fold into one row: the
/// unspecified pair (`0.0.0.0` and `::`), the loopback pair (`127.0.0.1`
/// and `::1`), or one address alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum BindKey {
    Any,
    Loopback,
    Addr(IpAddr),
}

fn bind_key(addr: IpAddr) -> BindKey {
    match addr {
        a if a.is_unspecified() => BindKey::Any,
        IpAddr::V4(a) if a.octets() == [127, 0, 0, 1] => BindKey::Loopback,
        IpAddr::V6(a) if a.is_loopback() => BindKey::Loopback,
        a => BindKey::Addr(a),
    }
}

/// Whose row it is, for folding: a process of the user's, or a uid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum OwnerKey {
    Process(u32, u64),
    Uid(u32),
}

/// The families a socket reaches: a `::` socket takes IPv4 too unless
/// `bindv6only` is set. A socket that set IPV6_V6ONLY itself is still
/// shown as both: the table does not say, and overstating reach is the
/// safe direction.
fn families_of(addr: IpAddr, v6only: bool) -> Vec<u8> {
    match addr {
        IpAddr::V4(_) => vec![4],
        IpAddr::V6(a) if a.is_unspecified() && !v6only => vec![4, 6],
        IpAddr::V6(_) => vec![6],
    }
}

/// One row before folding.
struct Raw<'a> {
    socket: &'a Socket,
    holder: Option<&'a Holder>,
}

pub struct Ports {
    #[expect(
        dead_code,
        reason = "part 1b asks it which containers publish ports (docker ps)"
    )]
    sender: Arc<dyn Sender>,
    own_uid: u32,
    tck: u64,
    page: u64,
    /// Whether any subscriber (or the `get`) asked for UDP.
    udp: bool,
    /// Whether every subscriber (or the `get`) is lite.
    lite: bool,
    cache: procfs::Cache,
    /// Inode to the user's processes that hold it.
    owners: HashMap<u64, Vec<Holder>>,
    /// The user's listening inodes at the last rebuild; None before one.
    built_for: Option<BTreeSet<u64>>,
    rebuilds: u64,
    /// The interface table, when read, with when and for which addresses.
    table: Option<(Instant, BTreeSet<IpAddr>, Table)>,
    system: SystemPorts,
}

impl Ports {
    pub fn new(sender: Arc<dyn Sender>) -> Self {
        Ports {
            sender,
            own_uid: crate::sys::uid(),
            tck: 100,
            page: 4096,
            udp: false,
            lite: false,
            cache: procfs::Cache::default(),
            owners: HashMap::new(),
            built_for: None,
            rebuilds: 0,
            table: None,
            system: SystemPorts::default(),
        }
    }

    /// The interface table for these sockets: read again when the set of
    /// addresses only it can place changed, or after a minute; not read
    /// at all while every address places itself.
    fn table(&mut self, root: &Root, sockets: &[Socket], now: Instant) -> Option<&Table> {
        let specific: BTreeSet<IpAddr> = sockets
            .iter()
            .map(|s| s.addr)
            .filter(|a| ifaces::intrinsic(*a).is_none())
            .collect();
        if specific.is_empty() {
            return None;
        }
        let fresh = self.table.as_ref().is_some_and(|(at, keys, _)| {
            *keys == specific && now.saturating_duration_since(*at) < TABLE_MAX_AGE
        });
        if !fresh {
            self.table = Some((now, specific, Table::read(root)));
        }
        self.table.as_ref().map(|(_, _, t)| t)
    }

    /// Whether the map must be built again: the user's inodes changed, or
    /// a mapped process (any member of a tree, not only its root) is gone
    /// or another one. Refreshes the trees' memory when it need not.
    fn stale(&mut self, root: &Root, mine: &BTreeSet<u64>) -> bool {
        if self.built_for.as_ref() != Some(mine) {
            return true;
        }
        let page = self.page;
        for h in self.owners.values_mut().flatten() {
            let mut rss = Vec::with_capacity(h.members.len());
            for &(pid, start) in &h.members {
                let st = procfs::read_text(root, &format!("proc/{pid}/stat"))
                    .as_deref()
                    .and_then(procfs::stat);
                if st.is_none_or(|s| s.start != start) {
                    return true;
                }
                rss.push(
                    root.read(&format!("proc/{pid}/statm"))
                        .ok()
                        .as_deref()
                        .and_then(procfs::statm)
                        .map(|m| m.resident * page),
                );
            }
            h.rss = sum(rss.into_iter());
        }
        false
    }

    /// Walks the user's processes and maps each wanted inode to those that
    /// hold it.
    fn rebuild(&mut self, root: &Root, mine: BTreeSet<u64>) {
        let procs = procfs::walk(root, &mut self.cache, self.own_uid);
        let own: Vec<&procfs::Proc> = procs
            .iter()
            .filter(|p| p.info.uid == Some(self.own_uid) && !p.kernel())
            .collect();
        let found = owner::owners_of(root, &own, &mine);
        let by_key: HashMap<(u32, u64), &procfs::Proc> =
            own.iter().map(|p| (p.key(), *p)).collect();
        let page = self.page;
        self.owners = found
            .into_iter()
            .map(|(inode, keys)| {
                let procs: Vec<&procfs::Proc> =
                    keys.iter().filter_map(|k| by_key.get(k).copied()).collect();
                let holders = trees(&procs)
                    .into_iter()
                    .map(|tree| {
                        let p = procs[tree[0]];
                        Holder {
                            pid: p.pid(),
                            start: p.stat.start,
                            name: p.name(),
                            label: label(&p.stat.comm, &p.info.cmdline),
                            members: tree.iter().map(|&i| procs[i].key()).collect(),
                            rss: sum(tree
                                .iter()
                                .map(|&i| procs[i].mem.map(|m| m.resident * page))),
                        }
                    })
                    .collect();
                (inode, holders)
            })
            .collect();
        self.built_for = Some(mine);
        self.rebuilds += 1;
    }

    /// One sample: the rows, folded and sorted, and what they were read
    /// with. `force` rebuilds the owner map (a `get`).
    fn measure(&mut self, root: &Root, now: Instant, force: bool) -> Value {
        let sockets = procnet::read(root, self.udp);
        let v6only = root
            .read("proc/sys/net/ipv6/bindv6only")
            .is_ok_and(|t| t.trim() == "1");
        let lite = self.lite;
        if !lite {
            let mine: BTreeSet<u64> = sockets
                .iter()
                .filter(|s| s.uid == self.own_uid)
                .map(|s| s.inode)
                .collect();
            if force || self.stale(root, &mine) {
                self.rebuild(root, mine);
            }
        }
        let table = self.table(root, &sockets, now).cloned().unwrap_or_default();
        let raws: Vec<Raw> = sockets
            .iter()
            .flat_map(|s| {
                let holders = (!lite && s.uid == self.own_uid)
                    .then(|| self.owners.get(&s.inode))
                    .flatten()
                    .filter(|h| !h.is_empty());
                match holders {
                    Some(hs) => hs
                        .iter()
                        .map(|h| Raw {
                            socket: s,
                            holder: Some(h),
                        })
                        .collect::<Vec<_>>(),
                    None => vec![Raw {
                        socket: s,
                        holder: None,
                    }],
                }
            })
            .collect();
        let rows = self.fold(root, &raws, &table, v6only);
        json!({
            "rows": rows,
            "lite": lite,
            "docker": "absent",
            "system": self.system,
        })
    }

    /// One row per port, protocol, owner and bind pair, sorted by port,
    /// protocol and pid.
    fn fold(&self, root: &Root, raws: &[Raw], table: &Table, v6only: bool) -> Vec<PortRow> {
        type Key = (u16, Proto, OwnerKey, BindKey);
        let mut groups: Vec<(Key, Vec<&Raw>)> = Vec::new();
        for r in raws {
            let owner = match r.holder {
                Some(h) => OwnerKey::Process(h.pid, h.start),
                None => OwnerKey::Uid(r.socket.uid),
            };
            let key = (
                r.socket.port,
                r.socket.proto,
                owner,
                bind_key(r.socket.addr),
            );
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push(r),
                None => groups.push((key, vec![r])),
            }
        }
        let mut rows: Vec<PortRow> = groups
            .into_iter()
            .map(|((port, proto, _, _), members)| {
                let first = members[0];
                // The IPv4 address names a folded pair.
                let shown = members
                    .iter()
                    .map(|r| r.socket.addr)
                    .find(IpAddr::is_ipv4)
                    .unwrap_or(first.socket.addr);
                let mut families: Vec<u8> = members
                    .iter()
                    .flat_map(|r| families_of(r.socket.addr, v6only))
                    .collect();
                families.sort_unstable();
                families.dedup();
                let scope: Scope = table.scope(shown);
                let uid = first.socket.uid;
                let owner = if uid == self.own_uid {
                    "you"
                } else if uid == 0 {
                    "root"
                } else {
                    "other"
                };
                let h = first.holder;
                PortRow {
                    port,
                    proto: proto.name().to_string(),
                    bind: shown.to_string(),
                    families,
                    scope: scope.name().to_string(),
                    exposed: scope.exposed(),
                    owner: owner.to_string(),
                    uid: Some(uid),
                    pid: h.map(|h| h.pid),
                    pids: h
                        .map(|h| h.members.iter().map(|m| m.0).collect())
                        .unwrap_or_default(),
                    start: h.map(|h| h.start),
                    process: h.map(|h| strings::plain(&h.name).into_owned()),
                    label: h.map(|h| h.label.clone()),
                    rss_bytes: h.and_then(|h| h.rss),
                    uptime_sec: h.and_then(|h| procfs::age_secs(root, h.start, self.tck)),
                    url: url(port),
                    ..Default::default()
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            (a.port, &a.proto, a.pid, &a.bind, a.uid)
                .cmp(&(b.port, &b.proto, b.pid, &b.bind, b.uid))
        });
        rows
    }
}

/// One subscriber's view: the UDP rows only when it asked, its own counts,
/// or with `lite` the counts alone (`total` counts every row there too; a
/// badge shows it, coloured by `exposedCount`).
pub fn view(data: &Value, params: &Value) -> Value {
    let udp = flag(params, "udp");
    let rows: Vec<PortRow> = serde_json::from_value(data["rows"].clone()).unwrap_or_default();
    let rows: Vec<PortRow> = rows
        .into_iter()
        .filter(|r| udp || r.proto == Proto::Tcp.name())
        .collect();
    let mine = || rows.iter().filter(|r| r.owner == "you");
    let exposed_count = mine().filter(|r| r.exposed).count() as u32;
    let left_behind_count = mine()
        .filter(|r| r.launcher.as_ref().is_some_and(|l| l.session_ended))
        .count() as u32;
    if flag(params, "lite") {
        return serde_json::to_value(PortsLite {
            lite: true,
            total: rows.len() as u32,
            exposed_count,
            left_behind_count,
            udp,
        })
        .unwrap_or(Value::Null);
    }
    let docker = if data["lite"].as_bool() == Some(true) {
        "off"
    } else {
        data["docker"].as_str().unwrap_or("absent")
    };
    serde_json::to_value(PortsData {
        total: rows.len() as u32,
        exposed_count,
        left_behind_count,
        udp,
        docker: docker.to_string(),
        system: serde_json::from_value(data["system"].clone()).unwrap_or_default(),
        ports: rows,
    })
    .unwrap_or(Value::Null)
}

impl Sampler for Ports {
    fn stream(&self) -> &'static str {
        PORTS
    }

    fn default_interval(&self) -> Duration {
        Duration::from_secs(5)
    }

    fn probe(&mut self, root: &Root) -> Result<(), String> {
        if let Err(e) = root.read("proc/net/tcp") {
            root.read("proc/net/tcp6")
                .map_err(|_| strings::ports_unavailable(&e.to_string()))?;
        }
        // The daemon's own uid through Root, as `apps` reads it: a fixture
        // tree says whose sockets are "yours".
        if let Some(uid) = root
            .read("proc/self/status")
            .ok()
            .and_then(|t| procfs::status(&t).uid)
        {
            self.own_uid = uid;
        }
        self.tck = selfcost::clk_tck(root);
        self.page = selfcost::page_size(root);
        Ok(())
    }

    fn wants(&mut self, params: &[Value]) {
        self.udp = params.iter().any(|p| flag(p, "udp"));
        self.lite = !params.is_empty() && params.iter().all(|p| flag(p, "lite"));
    }

    fn sample(&mut self, root: &Root, now: Instant) -> Value {
        self.measure(root, now, false)
    }

    /// A `get` (the page's Refresh, the CLI) maps the owners afresh, unless
    /// it asked for the counts alone.
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        let force = !self.lite;
        self.measure(root, now, force)
    }

    fn view(&self) -> Option<View> {
        Some(view)
    }

    fn accepts(&self) -> Option<Accepts> {
        Some(accepts)
    }

    /// Nobody subscribes: the process cache, the owner map and the
    /// interface table were theirs.
    fn idle(&mut self) {
        self.cache = procfs::Cache::default();
        self.owners.clear();
        self.built_for = None;
        self.table = None;
    }

    fn status(&self) -> Vec<(&'static str, Value)> {
        let owners: BTreeSet<u32> = self
            .owners
            .values()
            .flatten()
            .flat_map(|h| h.members.iter().map(|m| m.0))
            .collect();
        vec![(
            "ports",
            json!({"owners": owners.len(), "rebuilds": self.rebuilds}),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn params_are_two_booleans() {
        for good in [
            Value::Null,
            json!({}),
            json!({"udp": true}),
            json!({"udp": false, "lite": true}),
        ] {
            assert!(accepts(&good).is_ok(), "{good}");
        }
        for bad in [
            json!({"udp": "yes"}),
            json!({"sort": "x"}),
            json!({"lite": 1}),
            json!([]),
            json!("udp"),
        ] {
            assert!(accepts(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_label_names_what_is_served_not_the_command_line() {
        let l = |comm: &str, cmd: &str| label(comm, &args(cmd));
        assert_eq!(
            l(
                "node",
                "node /users/user/code/acme-web/node_modules/.bin/next dev"
            ),
            "next dev"
        );
        assert_eq!(
            l("python3", "python3 -m http.server --bind 0.0.0.0 8000"),
            "http.server"
        );
        assert_eq!(
            l("python3", "python3 manage.py runserver 0.0.0.0:8000"),
            "manage.py runserver"
        );
        assert_eq!(l("node", "node node_modules/.bin/vite --port 5173"), "vite");
        assert_eq!(l("node", "node --inspect server.js"), "server.js");
        assert_eq!(l("python3", "python3 -c print(1)"), "python3");
        assert_eq!(
            l("example-sync", "/usr/bin/example-sync --no-browser"),
            "example-sync"
        );
        assert_eq!(l("redis-server", "redis-server *:6379"), "redis-server");
        assert_eq!(l("opencode", "opencode serve"), "opencode serve");
        let long = l("node", &format!("node {} dev", "x".repeat(60)));
        assert_eq!(long.chars().count(), LABEL_MAX);
        assert!(!l("node", "node a\u{1b}[2J.js").contains('\u{1b}'));
    }

    #[test]
    fn urls_take_https_for_the_tls_ports() {
        assert_eq!(url(3000), "http://localhost:3000");
        assert_eq!(url(443), "https://localhost:443");
        assert_eq!(url(8443), "https://localhost:8443");
    }
}
