//! The `ports` stream on fixture roots (plan 007, part 1a): the socket
//! tables of both families parse, and only listeners (TCP) or bound
//! sockets (UDP, only when asked) are kept; a dual-stack listener is one
//! row marked as both families; each bind address gets its scope, and
//! `exposedCount` counts only the user's rows; inodes map to the user's
//! processes that hold them, and root's or another user's sockets get no
//! pid; the owner map is rebuilt only when the user's inodes change or a
//! mapped process is gone; the lite view reads no fd; uptime, memory and
//! the label come from the owner; and the stream's golden file.

use mirqab_core::Root;
use mirqab_core::act::Recording;
use mirqab_core::proto::stream::{PortRow, PortsData};
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::apps::Apps;
use mirqab_core::samplers::ports::procnet::{self, Proto};
use mirqab_core::samplers::ports::{self, Ports};
use mirqab_core::selfcost;
use serde_json::{Value, json};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn fixture(name: &str) -> Root {
    Root::new(fixture_dir(name))
}

/// A scratch directory under the caller's TMPDIR, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        let kind = e.file_type().unwrap();
        if kind.is_symlink() {
            // The fixtures' links name paths of an imagined machine:
            // copied as links, never followed.
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
        } else if kind.is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A copy of root-desktop to change.
fn scratch_copy(name: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("mirqab-ports-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy_tree(&fixture_dir("root-desktop"), &d);
    Scratch(d)
}

/// Appends rows to a table of a scratch tree.
fn add_rows(dir: &Path, table: &str, rows: &[&str]) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("proc/net").join(table))
        .unwrap();
    for r in rows {
        writeln!(f, "{r}").unwrap();
    }
}

/// A listening TCP row in the kernel's layout.
fn tcp_row(local: &str, uid: u32, inode: u64) -> String {
    format!(
        "  99: {local} 00000000:0000 0A 00000000:00000000 00:00000000 00000000 {uid:5}        0 {inode} 1 0000000000000000 100 0 0 10 0"
    )
}

fn probed(root: &Root) -> Ports {
    let mut p = Ports::new(Arc::new(Recording::new()));
    p.probe(root).expect("the fixture has socket tables");
    p
}

/// A `get` with these params, as the server makes it: the params, the
/// snapshot, the view.
fn get(p: &mut Ports, root: &Root, params: Value) -> Value {
    p.wants(std::slice::from_ref(&params));
    let data = p.snapshot(root, Instant::now());
    ports::view(&data, &params)
}

/// A subscriber's tick with these subscribers' params.
fn tick(p: &mut Ports, root: &Root, params: &[Value]) -> Value {
    p.wants(params);
    p.sample(root, Instant::now())
}

fn data(v: Value) -> PortsData {
    serde_json::from_value(v).expect("the stream's shape")
}

fn rows_on(d: &PortsData, port: u16) -> Vec<&PortRow> {
    d.ports.iter().filter(|r| r.port == port).collect()
}

fn row(d: &PortsData, port: u16) -> &PortRow {
    let rows = rows_on(d, port);
    assert_eq!(rows.len(), 1, "one row on {port}: {rows:?}");
    rows[0]
}

fn rebuilds(p: &Ports) -> u64 {
    let status = p.status();
    let (_, v) = status
        .iter()
        .find(|(k, _)| *k == "ports")
        .expect("ports in status");
    v["rebuilds"].as_u64().unwrap()
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn tcp_rows_parse_both_families_and_only_listeners() {
    let got: Vec<(IpAddr, u16, u32, u64)> = procnet::read(&fixture("root-desktop"), false)
        .into_iter()
        .map(|s| {
            assert_eq!(s.proto, Proto::Tcp);
            (s.addr, s.port, s.uid, s.inode)
        })
        .collect();
    assert_eq!(
        got,
        [
            (ip("127.0.0.1"), 3000, 1000, 70010),
            (ip("0.0.0.0"), 8000, 1000, 70060),
            (ip("192.0.2.10"), 9000, 1000, 70070),
            (ip("127.0.0.1"), 8384, 1000, 70030),
            (ip("203.0.113.5"), 22000, 1000, 70031),
            (ip("127.0.0.1"), 4096, 1000, 70040),
            (ip("0.0.0.0"), 22, 0, 11082),
            (ip("0.0.0.0"), 8088, 0, 80001),
            (ip("127.0.0.1"), 631, 0, 19998),
            (ip("127.0.0.1"), 6379, 999, 90001),
            (ip("::"), 5173, 1000, 70020),
            (ip("::"), 22, 0, 11083),
            (ip("::"), 8088, 0, 80002),
            (ip("::1"), 631, 0, 19999),
        ]
    );
    // Four host-order words in address order (plan 007 §5.1).
    assert_eq!(
        procnet::address("00000000000000000000000001000000"),
        Some(ip("::1"))
    );
    assert_eq!(
        procnet::address("0000000000000000FFFF00000100007F"),
        Some(ip("127.0.0.1")),
        "a v4-mapped address is the IPv4 one"
    );
    // An established connection is not a listener; a cut line is skipped.
    let header = std::fs::read_to_string(fixture_dir("root-desktop/proc/net/tcp")).unwrap();
    let text = format!(
        "{header}   9: 0100007F:1F90 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 70500 1 0000000000000000 20 4 30 10 -1\n  10: 0100007F:1F91 00000000:0000 0A 00000000\n  11: 0100007F:1F92\n"
    );
    let parsed = procnet::parse(&text, Proto::Tcp, "0A");
    assert_eq!(parsed.len(), 10, "{parsed:?}");
    assert!(parsed.iter().all(|s| s.port < 8080 || s.port > 8082));
}

#[test]
fn udp_rows_are_bound_sockets_and_only_when_asked() {
    let root = fixture("root-desktop");
    let mut p = probed(&root);
    let with = data(get(&mut p, &root, json!({"udp": true})));
    assert!(with.udp);
    let udp: Vec<(u16, &str)> = with
        .ports
        .iter()
        .filter(|r| r.proto == "udp")
        .map(|r| (r.port, r.bind.as_str()))
        .collect();
    assert_eq!(udp, [(53, "127.0.0.53"), (41234, "0.0.0.0")]);
    assert_eq!(with.total, 13);
    assert_eq!(with.exposed_count, 5, "41234 on 0.0.0.0 is yours");
    let mine = row(&with, 41234);
    assert_eq!((mine.pid, mine.owner.as_str()), (Some(6610), "you"));
    let without = data(get(&mut p, &root, json!({})));
    assert!(!without.udp);
    assert!(without.ports.iter().all(|r| r.proto == "tcp"));
    assert_eq!(without.total, 11);
    assert_eq!(without.exposed_count, 4);
    // One sample serves both: the view takes the UDP rows out.
    let sample = tick(&mut p, &root, &[json!({"udp": true}), json!({})]);
    assert_eq!(data(ports::view(&sample, &json!({}))).total, 11);
    assert_eq!(data(ports::view(&sample, &json!({"udp": true}))).total, 13);
    // Nobody asked: the UDP tables are not read.
    let sample = tick(&mut p, &root, &[json!({})]);
    assert_eq!(data(ports::view(&sample, &json!({"udp": true}))).total, 11);
}

#[test]
fn a_dual_stack_listener_is_one_row_marked_both() {
    let root = fixture("root-desktop");
    let mut p = probed(&root);
    let d = data(get(&mut p, &root, json!({})));
    let vite = row(&d, 5173);
    assert_eq!(
        (vite.bind.as_str(), vite.families.as_slice()),
        ("::", &[4, 6][..])
    );
    let proxy = row(&d, 8088);
    assert_eq!(
        (proxy.bind.as_str(), proxy.families.as_slice()),
        ("0.0.0.0", &[4, 6][..])
    );
    assert_eq!(proxy.owner, "root");
    let ssh = row(&d, 22);
    assert_eq!(ssh.families, [4, 6]);
    let cups = row(&d, 631);
    assert_eq!(
        (cups.bind.as_str(), cups.families.as_slice()),
        ("127.0.0.1", &[4, 6][..])
    );
    assert_eq!(row(&d, 3000).families, [4]);
    // With bindv6only set, a `::` socket takes IPv6 only.
    let s = scratch_copy("v6only");
    std::fs::write(s.0.join("proc/sys/net/ipv6/bindv6only"), "1\n").unwrap();
    let root = Root::new(&s.0);
    let d = data(get(&mut probed(&root), &root, json!({})));
    assert_eq!(row(&d, 5173).families, [6]);
    assert_eq!(row(&d, 22).families, [4, 6], "a pair is still both");
}

#[test]
fn each_bind_address_gets_its_scope() {
    let root = fixture("root-desktop");
    let d = data(get(&mut probed(&root), &root, json!({})));
    let scope = |port: u16| {
        let r = row(&d, port);
        (r.scope.as_str(), r.exposed)
    };
    assert_eq!(scope(3000), ("local", false));
    assert_eq!(scope(8000), ("all", true));
    assert_eq!(scope(9000), ("lan", true));
    assert_eq!(scope(22000), ("vpn", true));
    assert_eq!(scope(5173), ("all", true));
    assert_eq!(scope(22), ("all", true), "root's row says so too");
    assert_eq!(scope(8088), ("all", true));
    // Only your exposed rows count: 8000, 9000, 22000 and 5173, not 22
    // or 8088.
    assert_eq!(d.exposed_count, 4);
    let s = scratch_copy("scopes");
    add_rows(
        &s.0,
        "tcp",
        &[
            &tcp_row("016433C6:1388", 1000, 70901),
            &tcp_row("630200C0:1389", 1000, 70902),
            &tcp_row("140200C0:138A", 1000, 70903),
        ],
    );
    add_rows(
        &s.0,
        "tcp6",
        &[&tcp_row(
            "000080FE000000000000000001000000:138B",
            1000,
            70904,
        )],
    );
    let root = Root::new(&s.0);
    let d = data(get(&mut probed(&root), &root, json!({})));
    let scope = |port: u16| {
        let r = row(&d, port);
        (r.bind.as_str(), r.scope.as_str(), r.exposed)
    };
    assert_eq!(scope(5000), ("198.51.100.1", "bridge", true));
    assert_eq!(scope(5001), ("192.0.2.99", "unknown", true));
    assert_eq!(
        scope(5002),
        ("192.0.2.20", "lan", true),
        "a wireless address"
    );
    assert_eq!(scope(5003), ("fe80::1", "link", true));
    assert_eq!(d.exposed_count, 8);
}

#[test]
fn inodes_map_to_the_processes_that_hold_them_and_root_has_no_pid() {
    let s = scratch_copy("owners");
    add_rows(&s.0, "tcp", &[&tcp_row("0100007F:1F41", 1000, 79999)]);
    let root = Root::new(&s.0);
    let d = data(get(&mut probed(&root), &root, json!({})));
    let next = row(&d, 3000);
    assert_eq!(
        (next.owner.as_str(), next.pid, next.start),
        ("you", Some(6410), Some(9100))
    );
    let vite = row(&d, 5173);
    assert_eq!((vite.pid, vite.start), (Some(3020), Some(9000)));
    let sync = rows_on(&d, 8384);
    assert_eq!(sync[0].pid, Some(1300), "one process, two sockets");
    assert_eq!(row(&d, 22000).pid, Some(1300));
    // Root's sshd: its owner and no process, though root processes are
    // listed in the tree (systemd-logind is pid 430).
    let ssh = row(&d, 22);
    assert_eq!(
        (ssh.owner.as_str(), ssh.uid, ssh.pid, ssh.start),
        ("root", Some(0), None, None)
    );
    assert!(ssh.process.is_none() && ssh.label.is_none() && ssh.uptime_sec.is_none());
    let redis = row(&d, 6379);
    assert_eq!(
        (redis.owner.as_str(), redis.uid, redis.pid),
        ("other", Some(999), None)
    );
    // A socket of yours that no fd of yours names.
    let lost = row(&d, 8001);
    assert_eq!(
        (lost.owner.as_str(), lost.pid, lost.process.as_deref()),
        ("you", None, None)
    );
}

#[test]
fn the_owner_map_is_rebuilt_only_when_the_inode_set_changes() {
    let root = fixture("root-desktop");
    let mut p = probed(&root);
    let all = [json!({})];
    for _ in 0..3 {
        tick(&mut p, &root, &all);
    }
    assert_eq!(rebuilds(&p), 1, "the same sockets: built once");
    let s = scratch_copy("rebuild");
    let copy = Root::new(&s.0);
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 1, "the same tables in a copy");
    add_rows(&s.0, "tcp", &[&tcp_row("0100007F:1F41", 1000, 70061)]);
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 2, "a new listening socket of yours");
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 2);
    // A root socket more changes nothing of yours.
    add_rows(&s.0, "tcp", &[&tcp_row("00000000:0019", 0, 11090)]);
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 2, "root's sockets are not mapped");
    // A mapped process exits: its stat cannot be read.
    std::fs::remove_dir_all(s.0.join("proc/6600")).unwrap();
    let d = data(ports::view(&tick(&mut p, &copy, &all), &json!({})));
    assert_eq!(rebuilds(&p), 3, "a mapped process is gone");
    assert_eq!(row(&d, 8000).pid, None);
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 3);
    // Another process at a mapped pid: a new start time.
    let stat = s.0.join("proc/6610/stat");
    let text = std::fs::read_to_string(&stat).unwrap();
    std::fs::write(&stat, text.replace(" 9550 ", " 9999 ")).unwrap();
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 4, "a mapped pid is another process");
    // A `get` builds it afresh; `idle` forgets it.
    get(&mut p, &copy, json!({}));
    assert_eq!(rebuilds(&p), 5);
    p.idle();
    tick(&mut p, &copy, &all);
    assert_eq!(rebuilds(&p), 6);
}

#[test]
fn the_lite_view_reads_no_fd_and_gives_counts() {
    let root = fixture("root-desktop");
    let mut p = probed(&root);
    let lite = json!({"lite": true});
    let sample = tick(&mut p, &root, std::slice::from_ref(&lite));
    assert_eq!(
        rebuilds(&p),
        0,
        "no owner map while every subscriber is lite"
    );
    // Your TCP rows: 3000, 4096, 5173, 8000, 8384, 9000, 22000.
    assert_eq!(
        ports::view(&sample, &lite),
        json!({"lite": true, "total": 7, "exposedCount": 4, "leftBehindCount": 0, "udp": false})
    );
    assert_eq!(
        ports::view(&sample, &json!({"lite": true, "udp": true})),
        json!({"lite": true, "total": 7, "exposedCount": 4, "leftBehindCount": 0, "udp": true}),
        "UDP was not read for a TCP-only subscriber"
    );
    let get_lite = get(&mut p, &root, lite.clone());
    assert_eq!(get_lite["total"], 7);
    assert_eq!(rebuilds(&p), 0, "a lite get maps nothing either");
    // A subscriber that is not lite brings the map.
    let sample = tick(&mut p, &root, &[lite.clone(), json!({})]);
    assert_eq!(rebuilds(&p), 1);
    assert_eq!(data(ports::view(&sample, &json!({}))).docker, "absent");
    assert_eq!(ports::view(&sample, &lite)["exposedCount"], 4);
}

#[test]
fn uptime_rss_and_label_come_from_the_owner() {
    let root = fixture("root-desktop");
    let d = data(get(&mut probed(&root), &root, json!({})));
    let next = row(&d, 3000);
    // proc/uptime is 3600.50 s; the process started 9100 ticks (91 s)
    // after boot.
    assert_eq!(next.uptime_sec, Some(3509));
    let mut apps = Apps::new();
    apps.probe(&root).unwrap();
    let inspected = apps
        .query(&root, "inspect", &json!({"pid": 6410}))
        .expect("the inspector")
        .finish()
        .expect("an answer");
    assert_eq!(inspected["ageSec"], json!(next.uptime_sec));
    assert_eq!(
        next.rss_bytes,
        Some(60_000 * selfcost::page_size(&root)),
        "statm's resident pages"
    );
    assert_eq!(next.process.as_deref(), Some("node"));
    assert_eq!(next.label.as_deref(), Some("next dev"));
    assert_eq!(next.url, "http://localhost:3000");
    let py = row(&d, 8000);
    assert_eq!(py.process.as_deref(), Some("python3"));
    assert_eq!(py.label.as_deref(), Some("http.server"));
    assert_eq!(row(&d, 5173).label.as_deref(), Some("vite"));
    // Between rebuilds a tick reads the mapped processes' memory again.
    let s = scratch_copy("rss");
    let copy = Root::new(&s.0);
    let mut p = probed(&copy);
    tick(&mut p, &copy, &[json!({})]);
    std::fs::write(
        s.0.join("proc/6410/statm"),
        "180000 1000 10000 100 0 30000 0\n",
    )
    .unwrap();
    let d = data(ports::view(&tick(&mut p, &copy, &[json!({})]), &json!({})));
    assert_eq!(rebuilds(&p), 1);
    assert_eq!(
        row(&d, 3000).rss_bytes,
        Some(1000 * selfcost::page_size(&copy))
    );
}

#[test]
fn the_stream_matches_its_golden() {
    let root = fixture("root-desktop");
    let got = get(&mut probed(&root), &root, json!({"udp": true}));
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/ports.json")).unwrap();
    assert_eq!(
        got,
        golden,
        "actual:\n{}",
        serde_json::to_string_pretty(&got).unwrap()
    );
}
