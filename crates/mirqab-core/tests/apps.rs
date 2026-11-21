//! The `apps` sampler and the process inspector on fixture roots (plan 006,
//! Tests): each grouping rule picks the expected group; the System group;
//! kernel threads hidden; CPU and storage rates from the `.t1` twin;
//! sorting, `limit` and `expand`; a pid reused with a new start time is a
//! new process; the inspector's golden output for five processes, the
//! verdict rules, `pacman` asked once per path, and unreadable files left
//! out rather than failing. Part 3: `start` in every listed process, and
//! the `process` stream (one pid's `stat` and `statm`): its figures across
//! the `.t1` twin, `gone` for a missing pid and a changed start time.
//! Part 4: the "idle but heavy" tag with a fake focus source, the
//! `top-apps` minute row, `apps.inspect` with a start time (S5), GPU
//! memory read only from DRM fds and capped (D2), `.flatpak-info` read
//! only as a regular file (D7) and never through a FUSE root (audit
//! 2026-11-21, L7), and `get`'s first reading taken in
//! `prime` (D1). From the audit: code on an interpreter's command line
//! never names a terminal job; a `process` get's primed reading serves
//! that get only, and a get whose baseline a tick replaced does not wait
//! under the lock.

use mirqab_core::Root;
use mirqab_core::desktop::{Focus, FocusedSince, focused_since};
use mirqab_core::proto::stream::{AppGroup, AppsData, Inspect};
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::apps::focus::Thresholds;
use mirqab_core::samplers::apps::{self, Apps};
use mirqab_core::samplers::process::{self, Process};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
}

/// A scratch directory under the caller's TMPDIR, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("mirqab-apps-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// The stub pacman copied into `dir`, with a call log beside it.
fn pacman(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/bin/pacman");
    let dst = dir.join("pacman");
    std::fs::copy(src, &dst).unwrap();
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("pacman.log"), "").unwrap();
    dst
}

fn probed(pacman: &Path) -> Apps {
    let mut a = Apps::with_pacman(pacman.to_string_lossy());
    a.probe(&fixture("root-desktop")).unwrap();
    a
}

fn data(v: Value) -> AppsData {
    serde_json::from_value(v).expect("the stream's shape")
}

fn by_id<'a>(d: &'a AppsData, id: &str) -> &'a AppGroup {
    d.groups
        .iter()
        .find(|g| g.id == id)
        .unwrap_or_else(|| panic!("no group {id} in {:?}", ids(d)))
}

fn ids(d: &AppsData) -> Vec<&str> {
    d.groups.iter().map(|g| g.id.as_str()).collect()
}

/// A sample of root-desktop and one of root-desktop.t1 two seconds later.
fn two_samples(a: &mut Apps) -> (AppsData, AppsData) {
    let t0 = Instant::now();
    let first = data(a.sample(&fixture("root-desktop"), t0));
    let second = data(a.sample(&fixture("root-desktop.t1"), t0 + Duration::from_secs(2)));
    (first, second)
}

#[test]
fn each_grouping_rule_picks_the_expected_group() {
    let s = scratch("rules");
    let mut a = probed(&pacman(&s.0));
    let d = data(a.sample(&fixture("root-desktop"), Instant::now()));
    let check = |id: &str, name: &str, kind: &str, pids: &[u32]| {
        let g = by_id(&d, id);
        assert_eq!((g.name.as_str(), g.kind.as_str()), (name, kind), "{id}");
        assert_eq!(g.pids, pids, "{id}");
    };
    // Rule 1: a launcher's scope whose app id maps to no entry is named
    // after its main process (not the launcher's shell, not Hyprland); here
    // the main process is an entry's Exec= program.
    check(
        "scope:app-Hyprland-browser.sh-5650fedc",
        "Example Browser",
        "app",
        &[2000, 2001, 2010, 2011, 2012],
    );
    assert_eq!(
        by_id(&d, "scope:app-Hyprland-browser.sh-5650fedc")
            .icon
            .as_deref(),
        Some("example-browser")
    );
    check(
        "scope:app-Hyprland-editor-0a1b2c3d",
        "editor",
        "app",
        &[7100],
    );
    check("scope:app-Hyprland-tool-9a8b7c6d", "tool", "app", &[6000]);
    // A desktop-id scope and an app service take the entry's name.
    check(
        "scope:app-org.example.Calculator-24680",
        "Calculator",
        "app",
        &[8000],
    );
    check(
        "service:app-org.example.Sync@autostart",
        "Example Sync",
        "service",
        &[1300],
    );
    // Plain services are known by their unit; the user manager is one.
    check("service:mirqab", "mirqab", "service", &[4321]);
    check("service:pipewire", "pipewire", "service", &[1210]);
    check("service:init", "systemd", "service", &[1100, 1101]);
    assert_eq!(
        by_id(&d, "service:mirqab").unit.as_deref(),
        Some("mirqab.service")
    );
    // Rule 2: the Flatpak scope and a sandboxed process outside it.
    check(
        "flatpak:org.example.Notes",
        "Example Notes",
        "flatpak",
        &[4000, 4001, 4100],
    );
    // Rule 3: a rootless container under the user manager, its pid 5001
    // on one of the user's sub-uids (review of PR #16).
    check(
        "container:c0ffee00c0ff",
        "postgres (container c0ffee00c0ff)",
        "container",
        &[5000, 5001],
    );
    // Rule 4: the terminal keeps itself; each shell's session is its job.
    check("scope:app-Hyprland-foot-3e4f5a6b", "Foot", "app", &[3000]);
    check("session:3010", "vite", "terminal", &[3010, 3020, 3021]);
    check("session:3030", "cargo", "terminal", &[3030, 3040, 3041]);
    // A bare interpreter is named as itself, never after the desktop entry
    // whose Exec= happens to run the same interpreter (review of PR #16).
    check("session:3050", "python3", "terminal", &[3050, 3060]);
    check(
        "scope:app-Hyprland-script-1a2b3c4d",
        "python3",
        "app",
        &[7500],
    );
    // A NoDisplay handler running the browser's program does not rename it
    // (the browser group's name is asserted above).
    // An escaped dash in a unit name belongs to the application id.
    check(
        "scope:app-Hyprland-two\\x2dwords-0f0f0f0f",
        "Two Words",
        "app",
        &[7300],
    );
    check(
        "flatpak:org.example.Two-Words",
        "Two Words Reader",
        "flatpak",
        &[7400],
    );
    // Rule 5: the process tree stops below a shell, and at another user.
    check("tree:6201", "devserver", "process", &[6201, 6202]);
    check("tree:1050", "Hyprland", "process", &[1050, 6200]);
    assert_eq!(
        by_id(&d, "tree:6201").processes.as_ref().unwrap()[1].name,
        "devserver-worker",
        "a name the kernel cut short is the program's"
    );
    // Other users fold into System (a root-run container too); kernel
    // threads are nowhere.
    check("system", "System", "system", &[1, 410, 430, 600, 900, 5100]);
    for g in &d.groups {
        assert!(!g.pids.contains(&2) && !g.pids.contains(&3), "{}", g.id);
    }
    assert_eq!(d.totals.procs, 54);
    assert_eq!(d.totals.cores, Some(16));
    assert_eq!(d.totals.groups, 28);
    assert_eq!(d.groups.iter().map(|g| g.procs).sum::<u32>(), 54);
}

#[test]
fn cpu_and_storage_come_from_the_difference() {
    let s = scratch("delta");
    let mut a = probed(&pacman(&s.0));
    let (first, second) = two_samples(&mut a);
    assert!(
        first.groups.iter().all(|g| g.cpu.is_none()),
        "no baseline yet"
    );
    assert_eq!(first.totals.cpu, None);
    let cpu = |id: &str| by_id(&second, id).cpu;
    // Ticks over two seconds at 100 a second, in percent of one core.
    assert_eq!(cpu("scope:app-Hyprland-browser.sh-5650fedc"), Some(66.0));
    assert_eq!(cpu("session:3030"), Some(91.0));
    assert_eq!(cpu("session:3010"), Some(12.0));
    assert_eq!(cpu("system"), Some(3.0));
    assert_eq!(cpu("service:init"), Some(0.0));
    let browser = by_id(&second, "scope:app-Hyprland-browser.sh-5650fedc");
    let top = browser.top.as_ref().unwrap();
    assert_eq!(
        (top.pid, top.name.as_str(), top.cpu),
        (2001, "browser", Some(40.0))
    );
    assert_eq!(browser.io_read_per_sec, Some(1_048_576.0));
    assert_eq!(browser.io_write_per_sec, Some(524_288.0));
    assert_eq!(
        by_id(&second, "session:3030").io_write_per_sec,
        Some(26_214_400.0)
    );
    assert_eq!(by_id(&second, "system").io_read_per_sec, None, "unreadable");
    // Resident minus shared for each member, the largest shared part once.
    let pages = (200_000 - 30_000)
        + (100_000 - 25_000)
        + (50_000 - 20_000)
        + (10_000 - 8_000)
        + (1_000 - 800)
        + 30_000;
    assert_eq!(browser.rss_bytes, pages * 4096);
    assert_eq!(second.totals.cpu, Some(201.5));
}

#[test]
fn a_reused_pid_is_a_new_process() {
    let s = scratch("reuse");
    let mut a = probed(&pacman(&s.0));
    let (first, second) = two_samples(&mut a);
    by_id(&first, "scope:app-Hyprland-oldjob-11111111");
    assert!(
        !ids(&second).contains(&"scope:app-Hyprland-oldjob-11111111"),
        "the old process is gone"
    );
    // 7000 again, started later and with more ticks than the old one had:
    // not 600 % from the old process's counter, but a new process.
    let new = by_id(&second, "scope:app-Hyprland-newjob-22222222");
    assert_eq!((new.name.as_str(), new.cpu), ("newjob", Some(0.0)));
    // 7200 is reused with the same name and a new start time, so the start
    // time alone must tell them apart: not 1000 % from the old counter.
    let worker = by_id(&second, "scope:app-Hyprland-worker-33333333");
    assert_eq!((worker.name.as_str(), worker.cpu), ("worker", Some(0.0)));
    assert_eq!(
        by_id(&first, "scope:app-Hyprland-worker-33333333").pids,
        [7200]
    );
}

#[test]
fn views_sort_limit_and_expand() {
    let s = scratch("views");
    let mut a = probed(&pacman(&s.0));
    let (_, second) = two_samples(&mut a);
    let full = serde_json::to_value(&second).unwrap();
    let names = |v: &Value| -> Vec<String> {
        v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap().to_string())
            .collect()
    };
    let cpu = apps::view(&full, &json!({"limit": 3}));
    assert_eq!(cpu["sort"], "cpu");
    assert_eq!(
        names(&cpu),
        ["cargo", "Example Browser", "vite", "System"],
        "the System group comes last, outside the limit"
    );
    let mem = apps::view(&full, &json!({"sort": "mem", "limit": 2}));
    assert_eq!(names(&mem), ["cargo", "Example Browser", "System"]);
    // The count before the limit, so a reader can tell the list was cut.
    assert_eq!(mem["totals"]["groups"], 28);
    assert_eq!(mem["groups"].as_array().unwrap().len(), 3);
    let name = apps::view(&full, &json!({"sort": "name", "limit": 3}));
    assert_eq!(names(&name), ["Calculator", "cargo", "devserver", "System"]);
    let default = apps::view(&full, &Value::Null);
    assert_eq!(default["groups"].as_array().unwrap().len(), 28);
    assert!(
        default["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g.get("processes").is_none()),
        "processes only when expanded"
    );
    let one = apps::view(&full, &json!({"expand": ["session:3030"]}));
    for g in one["groups"].as_array().unwrap() {
        assert_eq!(g.get("processes").is_some(), g["id"] == "session:3030");
    }
    let cargo = one["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "session:3030")
        .unwrap();
    assert_eq!(cargo["processes"][0]["name"], "rustc", "heaviest first");
    let all = apps::view(&full, &json!({"expand": true}));
    assert!(
        all["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g.get("processes").is_some())
    );
}

#[test]
fn the_stream_matches_its_golden_file() {
    let s = scratch("golden");
    let mut a = probed(&pacman(&s.0));
    let (_, second) = two_samples(&mut a);
    // Every group open, as the Apps page asks (plan 006, part 3): the
    // golden holds each process with its start time.
    let got = apps::view(
        &serde_json::to_value(&second).unwrap(),
        &json!({"expand": true}),
    );
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    assert_eq!(
        got,
        golden,
        "actual:\n{}",
        serde_json::to_string_pretty(&got).unwrap()
    );
}

fn inspect(a: &mut Apps, root: &Root, pid: u32) -> Value {
    a.query(root, "inspect", &json!({ "pid": pid }))
        .expect("a query the sampler knows")
        .finish()
        .expect("an answer")
}

#[test]
fn the_inspector_matches_its_golden_file() {
    let s = scratch("inspect");
    let mut a = probed(&pacman(&s.0));
    let root = fixture("root-desktop");
    let got: Vec<Value> = [2001, 4001, 6000, 6201, 410]
        .iter()
        .map(|&pid| inspect(&mut a, &root, pid))
        .collect();
    let got = Value::Array(got);
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/apps-inspect.json")).unwrap();
    assert_eq!(
        got,
        golden,
        "actual:\n{}",
        serde_json::to_string_pretty(&got).unwrap()
    );
    for v in got.as_array().unwrap() {
        let _: Inspect = serde_json::from_value(v.clone()).expect("the inspector's shape");
    }
}

#[test]
fn verdicts_follow_the_rules() {
    let s = scratch("verdicts");
    let mut a = probed(&pacman(&s.0));
    let root = fixture("root-desktop");
    let mut verdict = |pid: u32| {
        let v = inspect(&mut a, &root, pid);
        (
            v["verdict"]["code"].as_str().unwrap().to_string(),
            v["verdict"]["text"].as_str().unwrap().to_string(),
        )
    };
    assert_eq!(verdict(3).0, "kernel");
    assert_eq!(verdict(410).0, "other-user");
    assert_eq!(
        verdict(3041),
        ("packaged".into(), "Packaged by extra/rust".into())
    );
    assert_eq!(
        verdict(7100),
        (
            "packaged".into(),
            "Packaged by editor-git, which no sync repository provides (built locally or from the AUR)"
                .into()
        )
    );
    assert_eq!(verdict(6000).0, "replaced", "even though a package owns it");
    assert_eq!(verdict(4100).0, "flatpak");
    assert_eq!(verdict(5000).0, "container");
    assert_eq!(
        verdict(6201),
        (
            "home".into(),
            "Not owned by any package, running from your home directory".into()
        )
    );
    assert_eq!(verdict(1300).0, "unowned");
    // A sandboxed path means nothing to the host's package database: it is
    // never asked about one.
    let log = std::fs::read_to_string(s.0.join("pacman.log")).unwrap();
    assert!(!log.contains("/app/bin/"), "{log}");
    assert!(!log.contains("postgres"), "{log}");
}

#[test]
fn pacman_is_asked_once_per_path_and_its_absence_is_no_error() {
    let s = scratch("once");
    let mut a = probed(&pacman(&s.0));
    let root = fixture("root-desktop");
    inspect(&mut a, &root, 2010);
    inspect(&mut a, &root, 2011);
    let log = std::fs::read_to_string(s.0.join("pacman.log")).unwrap();
    assert_eq!(
        log,
        "-Qqo /usr/lib/example-browser/browser\n-Sddp --print-format %r example-browser\n"
    );

    let mut none = probed(&s.0.join("no-such-pacman"));
    let v = inspect(&mut none, &root, 2001);
    assert_eq!(v["verdict"]["code"], "unknown");
    assert!(v.get("package").is_none());
    assert_eq!(v["exe"], "/usr/lib/example-browser/browser");
}

#[test]
fn unreadable_files_are_left_out_not_errors() {
    // A process with nothing but its stat: grouped (as another user's, its
    // owner unknown) and inspected without failing; and a tree with no
    // processes at all.
    let s = scratch("bare");
    let p = s.0.join("proc/9000");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("stat"),
        "9000 (lonely) S 1 9000 9000 0 -1 4194560 0 0 0 0 5 5 0 0 20 0 1 0 100 0 0 0",
    )
    .unwrap();
    std::fs::create_dir_all(s.0.join("proc/self")).unwrap();
    std::fs::write(
        s.0.join("proc/self/status"),
        "Uid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let d = data(a.sample(&root, Instant::now()));
    assert_eq!(by_id(&d, "system").pids, [9000]);
    assert_eq!(by_id(&d, "system").rss_bytes, 0);
    let v = inspect(&mut a, &root, 9000);
    assert_eq!(v["name"], "lonely");
    assert_eq!(v["limited"], true);
    for absent in [
        "user",
        "memory",
        "exe",
        "sandbox",
        "files",
        "cmdline",
        "startedAt",
    ] {
        assert!(v.get(absent).is_none(), "{absent} in {v}");
    }

    let mut min = Apps::with_pacman("pacman-not-needed");
    min.probe(&fixture("root-minimal")).unwrap();
    let empty = data(min.sample(&fixture("root-minimal"), Instant::now()));
    assert!(empty.groups.is_empty());
    assert_eq!(empty.totals.procs, 0);
}

#[test]
fn the_architecture_is_read_only_inside_the_tree() {
    // The ELF header comes through proc/<pid>/exe. In a fixture that link
    // names a path of the live machine, which is never read: arch is left
    // out for every fixture process, and read for a link inside the tree.
    let s = scratch("arch");
    let mut header = vec![0u8; 64];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = 2;
    header[5] = 1;
    header[18] = 183;
    std::fs::create_dir_all(s.0.join("usr/bin")).unwrap();
    std::fs::write(s.0.join("usr/bin/emulated"), &header).unwrap();
    for (pid, target) in [
        (9100, s.0.join("usr/bin/emulated")),
        (9101, PathBuf::from("/bin/sh")),
    ] {
        let p = s.0.join(format!("proc/{pid}"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("stat"),
            format!("{pid} (x) S 1 {pid} {pid} 0 -1 4194560 0 0 0 0 1 1 0 0 20 0 1 0 100 0 0 0"),
        )
        .unwrap();
        std::fs::write(p.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").unwrap();
        std::os::unix::fs::symlink(&target, p.join("exe")).unwrap();
    }
    header[18] = 62;
    std::fs::create_dir_all(s.0.join("proc/self")).unwrap();
    std::fs::write(s.0.join("usr/bin/daemon"), &header).unwrap();
    std::os::unix::fs::symlink(s.0.join("usr/bin/daemon"), s.0.join("proc/self/exe")).unwrap();
    std::fs::write(
        s.0.join("proc/self/status"),
        "Uid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let v = inspect(&mut a, &root, 9100);
    assert_eq!(
        v["arch"],
        json!({"machine": "aarch64", "bits": 64, "foreign": true})
    );
    assert!(inspect(&mut a, &root, 9101).get("arch").is_none());
    let fx = fixture("root-desktop");
    let mut b = probed(&s.0.join("pacman"));
    assert!(inspect(&mut b, &fx, 2001).get("arch").is_none());
}

/// A scratch tree with `proc/self/status` saying the daemon runs as 1000.
fn bare_root(name: &str) -> Scratch {
    let s = scratch(name);
    std::fs::create_dir_all(s.0.join("proc/self")).unwrap();
    std::fs::write(
        s.0.join("proc/self/status"),
        "Uid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    s
}

#[test]
fn a_name_that_is_not_utf8_is_still_listed() {
    // Any process may name itself with any bytes (prctl PR_SET_NAME); a
    // name that is not UTF-8 must not hide it from the list or from the
    // inspector (review of PR #16).
    let s = bare_root("utf8");
    let p = s.0.join("proc/9200");
    std::fs::create_dir_all(&p).unwrap();
    let mut stat =
        b"9200 (bad\xff\xfename) S 1 9200 9200 0 -1 4194560 0 0 0 0 5 5 0 0 20 0 1 0 100 0 0 0"
            .to_vec();
    stat.push(b'\n');
    std::fs::write(p.join("stat"), stat).unwrap();
    std::fs::write(
        p.join("status"),
        b"Name:\tbad\xff\xfename\nUid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let d = data(a.sample(&root, Instant::now()));
    assert_eq!(d.groups.len(), 1, "{:?}", ids(&d));
    assert_eq!(d.groups[0].pids, [9200]);
    assert_eq!(d.groups[0].kind, "process", "the user's, not System");
    assert!(
        d.groups[0].name.contains('\u{fffd}'),
        "{}",
        d.groups[0].name
    );
    let v = inspect(&mut a, &root, 9200);
    assert_eq!(v["limited"], false, "{v}");
    assert!(v["name"].as_str().unwrap().contains('\u{fffd}'));
}

#[test]
fn a_process_moved_after_its_first_sample_is_regrouped() {
    // A launcher may move a process into its scope just after it starts;
    // pid, start time and name stay the same. The second sighting reads
    // the process again once (review of PR #16).
    let s = bare_root("moved");
    let p = s.0.join("proc/9300");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("stat"),
        "9300 (late) S 1 9300 9300 0 -1 4194560 0 0 0 0 5 5 0 0 20 0 1 0 100 0 0 0\n",
    )
    .unwrap();
    std::fs::write(p.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").unwrap();
    std::fs::write(
        p.join("cgroup"),
        "0::/user.slice/user-1000.slice/session-2.scope\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let t0 = Instant::now();
    let first = data(a.sample(&root, t0));
    assert_eq!(ids(&first), ["tree:9300"]);
    std::fs::write(
        p.join("cgroup"),
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-Hyprland-late-0a0a0a0a.scope\n",
    )
    .unwrap();
    let second = data(a.sample(&root, t0 + Duration::from_secs(2)));
    assert_eq!(ids(&second), ["scope:app-Hyprland-late-0a0a0a0a"]);
    // Settled: a third move is not seen (one extra read per process).
    std::fs::write(
        p.join("cgroup"),
        "0::/user.slice/user-1000.slice/session-2.scope\n",
    )
    .unwrap();
    let third = data(a.sample(&root, t0 + Duration::from_secs(4)));
    assert_eq!(ids(&third), ["scope:app-Hyprland-late-0a0a0a0a"]);
}

#[test]
fn code_on_an_interpreters_command_line_never_names_a_job() {
    // A terminal job is named after the script an interpreter runs. Code
    // given inline (`python3 -c CODE`) or an option's value is no script:
    // the job is named after the interpreter, since its name reaches the
    // top-apps history row and agents (audit of plan 006).
    let s = bare_root("inline");
    let write = |pid: u32, ppid: u32, sid: u32, tpgid: u32, start: u32, argv: &[&str]| {
        let p = s.0.join(format!("proc/{pid}"));
        std::fs::create_dir_all(&p).unwrap();
        let comm = argv[0].trim_start_matches('-');
        std::fs::write(
            p.join("stat"),
            format!("{pid} ({comm}) S {ppid} {pid} {sid} 34816 {tpgid} 4194560 0 0 0 0 5 5 0 0 20 0 1 0 {start} 0 0 0"),
        )
        .unwrap();
        std::fs::write(p.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").unwrap();
        std::fs::write(
            p.join("cgroup"),
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-Hyprland-foot-0b0b0b0b.scope\n",
        )
        .unwrap();
        let mut cmdline = argv.join("\0");
        cmdline.push('\0');
        std::fs::write(p.join("cmdline"), cmdline).unwrap();
    };
    write(9500, 1, 9500, 0, 50, &["foot"]);
    let cases: &[(&[&str], &str)] = &[
        (
            &[
                "python3",
                "-c",
                "import urllib.request as u; u.urlopen('https://api.example.org/v1/items?token=s3cr3t')",
            ],
            "python3",
        ),
        (&["python3", "-c", "x/y?token=z"], "python3"),
        (&["python3", "-Bc", "print(1)"], "python3"),
        (&["node", "-e", "require('a/b')"], "node"),
        (&["node", "--eval=require('a/b')"], "node"),
        (&["perl", "-lne", "print if /x/", "notes.txt"], "perl"),
        (&["ruby", "-e", "puts 1"], "ruby"),
        (&["deno", "eval", "console.log(1)"], "deno"),
        (&["python3", "-W", "ignore", "tool.py"], "tool.py"),
        (&["node", "-r", "dotenv/config", "app.js"], "app.js"),
        (&["node", "--env-file", ".env", "app.js"], "app.js"),
        (&["ruby", "-E", "UTF-8", "x.rb"], "x.rb"),
        (&["ruby", "-C", "../proj", "x.rb"], "x.rb"),
        (&["deno", "run", "-A", "app.ts"], "app.ts"),
        (&["node", "node_modules/.bin/vite"], "vite"),
        (&["python3", "-m", "http.server"], "http.server"),
        (&["python3", "-u", "my script.py"], "python3"),
    ];
    for (n, (argv, _)) in cases.iter().enumerate() {
        let shell = 9510 + 10 * n as u32;
        write(shell, 9500, shell, shell + 1, 100, &["-zsh"]);
        write(shell + 1, shell, shell, shell + 1, 200, argv);
    }
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let d = data(a.sample(&root, Instant::now()));
    for (n, (argv, name)) in cases.iter().enumerate() {
        let g = by_id(&d, &format!("session:{}", 9510 + 10 * n as u32));
        assert_eq!(g.name, *name, "{argv:?}");
    }
}

#[test]
fn every_listed_process_carries_its_start_time() {
    let s = scratch("start");
    let mut a = probed(&pacman(&s.0));
    let d = data(apps::view(
        &a.sample(&fixture("root-desktop"), Instant::now()),
        &json!({"expand": true}),
    ));
    let browser = by_id(&d, "scope:app-Hyprland-browser.sh-5650fedc");
    let starts: Vec<(u32, u64)> = browser
        .processes
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| (p.pid, p.start))
        .collect();
    // The values `wouldDo` carries for the same processes.
    assert!(starts.contains(&(2001, 5010)), "{starts:?}");
    assert!(starts.contains(&(2000, 5000)), "{starts:?}");
    for g in &d.groups {
        for p in g.processes.as_ref().unwrap() {
            assert!(p.start > 0, "{} has a start time", p.pid);
        }
    }
}

fn process_probed() -> Process {
    let mut p = Process::new();
    p.probe(&fixture("root-desktop")).unwrap();
    p
}

/// Two readings for `params`: root-desktop, then root-desktop.t1 two
/// seconds later, each shaped by the stream's view.
fn process_twice(params: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let mut p = process_probed();
    p.wants(params);
    let t0 = Instant::now();
    let a = p.sample(&fixture("root-desktop"), t0);
    p.wants(params);
    let b = p.sample(&fixture("root-desktop.t1"), t0 + Duration::from_secs(2));
    let shape = |d: &Value| params.iter().map(|q| process::view(d, q)).collect();
    (shape(&a), shape(&b))
}

#[test]
fn the_process_stream_reads_one_pid_across_two_ticks() {
    let (first, second) = process_twice(&[json!({"pid": 2001})]);
    assert_eq!(first[0]["cpu"], Value::Null, "no rate on the first reading");
    assert_eq!(first[0]["start"], 5010);
    // 80 ticks of 100 in 2 s: 40 % of one core.
    assert_eq!(second[0]["cpu"], 40.0);
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/process.json")).unwrap();
    assert_eq!(
        second[0],
        golden,
        "actual:\n{}",
        serde_json::to_string_pretty(&second[0]).unwrap()
    );
}

#[test]
fn a_missing_pid_or_a_new_start_time_is_gone() {
    let params = [
        json!({"pid": 99999}),
        // 7000 is another process in .t1 (procs.tsv): pinned at the first.
        json!({"pid": 7000}),
        // Named with its start time: the old process, then gone.
        json!({"pid": 7000, "start": 300000}),
        // Named with the new one's: gone at first, there in .t1.
        json!({"pid": 7000, "start": 360000}),
    ];
    let (first, second) = process_twice(&params);
    let gone = |pid: u32| json!({"pid": pid, "gone": true});
    assert_eq!(first[0], gone(99999));
    assert_eq!(second[0], gone(99999));
    assert_eq!(first[1]["name"], "oldjob");
    assert_eq!(second[1], gone(7000), "another process took the pid");
    assert_eq!(first[2]["name"], "oldjob");
    assert_eq!(second[2], gone(7000));
    assert_eq!(first[3], gone(7000));
    assert_eq!(second[3]["name"], "newjob");
    assert_eq!(second[3]["cpu"], Value::Null, "no baseline for the new one");

    // A pid empty when first watched stays gone when a process takes it.
    let mut p = process_probed();
    let ask = [json!({"pid": 7000})];
    p.wants(&ask);
    let s = scratch("procgone");
    std::fs::create_dir_all(s.0.join("proc")).unwrap();
    let t0 = Instant::now();
    let empty = p.sample(&Root::new(&s.0), t0);
    assert_eq!(process::view(&empty, &ask[0]), gone(7000));
    p.wants(&ask);
    let later = p.sample(&fixture("root-desktop"), t0 + Duration::from_secs(1));
    assert_eq!(process::view(&later, &ask[0]), gone(7000));
}

#[test]
fn the_process_stream_takes_only_a_pid_and_a_start() {
    let p = process_probed();
    let accepts = p.accepts().expect("the stream checks its params");
    assert!(accepts(&json!({"pid": 2001})).is_ok());
    assert!(accepts(&json!({})).is_err());
    assert!(accepts(&Value::Null).is_err());
    assert!(p.live(), "a live graph may ask for 1 s");
    assert_eq!(p.stream(), "process");
}

#[test]
fn a_pin_does_not_outlive_its_subscribers() {
    // Review of PR #18: with no subscriber the loop tells the sampler
    // (`idle`), and the next one watches the process at the pid now.
    let ask = [json!({"pid": 7000})];
    let mut p = process_probed();
    let t0 = Instant::now();
    p.wants(&ask);
    let old = p.sample(&fixture("root-desktop"), t0);
    assert_eq!(process::view(&old, &ask[0])["name"], "oldjob");
    p.idle();
    p.wants(&ask);
    let new = p.sample(&fixture("root-desktop.t1"), t0 + Duration::from_secs(2));
    assert_eq!(process::view(&new, &ask[0])["name"], "newjob");
    assert_eq!(
        process::view(&new, &ask[0])["cpu"],
        Value::Null,
        "no baseline kept"
    );

    // A get never reads the pins: the process at the pid now.
    let mut p = process_probed();
    p.wants(&ask);
    p.sample(&fixture("root-desktop"), t0);
    p.wants(&ask);
    let got = p.snapshot(&fixture("root-desktop.t1"), Instant::now());
    assert_eq!(process::view(&got, &ask[0])["name"], "newjob");
}

#[test]
fn a_get_of_a_missing_pid_does_not_wait() {
    let mut p = process_probed();
    let ask = [json!({"pid": 99999})];
    p.wants(&ask);
    let t = Instant::now();
    let got = p.snapshot(&fixture("root-desktop"), t);
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(
        process::view(&got, &ask[0]),
        json!({"pid": 99999, "gone": true})
    );
}

// --- Part 4 ------------------------------------------------------------------

/// Focus changes, oldest first (`Some(pid)` a window, `None` none).
type History = Vec<(Instant, Option<u32>)>;

/// A focus source the test steers: a history of focus changes read by
/// the same rule as Hyprland's (`desktop::focused_since`), whether a
/// start succeeds, and how often it was started and stopped.
#[derive(Clone, Default)]
struct FakeFocus {
    history: Arc<Mutex<History>>,
    available: Arc<Mutex<bool>>,
    running: Arc<Mutex<bool>>,
    calls: Arc<Mutex<(u32, u32)>>,
}

impl FakeFocus {
    /// A source that starts, with nothing focused yet.
    fn up() -> FakeFocus {
        let f = FakeFocus::default();
        *f.available.lock().unwrap() = true;
        f
    }
    /// Focus goes to `pid` (None: no window) at `at`.
    fn focus(&self, at: Instant, pid: Option<u32>) {
        self.history.lock().unwrap().push((at, pid));
    }
}

impl Focus for FakeFocus {
    fn start(&mut self, _root: &Root, _uid: u32) -> Result<&'static str, String> {
        self.calls.lock().unwrap().0 += 1;
        if *self.available.lock().unwrap() {
            *self.running.lock().unwrap() = true;
            Ok("fake")
        } else {
            Err("no source".into())
        }
    }
    fn focused(&mut self, since: Instant) -> Option<FocusedSince> {
        if !*self.running.lock().unwrap() {
            return None;
        }
        Some(focused_since(&self.history.lock().unwrap(), since))
    }
    fn source(&self) -> Option<&'static str> {
        self.running.lock().unwrap().then_some("fake")
    }
    fn stop(&mut self) {
        *self.running.lock().unwrap() = false;
        self.calls.lock().unwrap().1 += 1;
    }
}

const CARGO: &str = "session:3030";
const BROWSER: &str = "scope:app-Hyprland-browser.sh-5650fedc";

/// Samples root-desktop and its `.t1` twin in turn, two seconds apart,
/// from t0 + `from` s to t0 + `to` s; returns the last frame.
fn alternate(a: &mut Apps, t0: Instant, from: u64, to: u64) -> AppsData {
    let mut last = None;
    for s in (from..=to).step_by(2) {
        let tree = if (s / 2) % 2 == 0 {
            "root-desktop"
        } else {
            "root-desktop.t1"
        };
        last = Some(data(a.sample(&fixture(tree), t0 + Duration::from_secs(s))));
    }
    last.unwrap()
}

#[test]
fn a_busy_group_out_of_focus_is_tagged_once_its_window_is_full() {
    let s = scratch("heavy");
    let mut a = probed(&pacman(&s.0));
    let fake = FakeFocus::up();
    let t0 = Instant::now();
    fake.focus(t0, Some(2001));
    a.set_focus(Box::new(fake.clone()));
    a.set_thresholds(Thresholds {
        window: Duration::from_secs(3),
        cpu: 20.0,
    });
    // cargo is 91 % and 0 % in turn (the twin's times go back), so 45.5 %
    // on average; the browser is as busy, but it has the focus.
    let d = alternate(&mut a, t0, 0, 4);
    assert_eq!(d.focus.as_deref(), Some("fake"));
    assert!(!by_id(&d, CARGO).idle_heavy, "the window is not full yet");
    let d = alternate(&mut a, t0, 6, 6);
    assert!(by_id(&d, CARGO).idle_heavy, "a full window, unfocused");
    assert!(!by_id(&d, BROWSER).idle_heavy, "focused now");
    assert!(
        !by_id(&d, "session:3010").idle_heavy,
        "vite: 6 % on average"
    );
    assert!(
        d.groups.iter().filter(|g| g.idle_heavy).count() == 1,
        "{:?}",
        d.groups
            .iter()
            .filter(|g| g.idle_heavy)
            .map(|g| &g.id)
            .collect::<Vec<_>>()
    );
    // The serialised frame says it; an untagged group says nothing.
    let v = serde_json::to_value(&d).unwrap();
    let row = |id: &str| {
        v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(row(CARGO)["idleHeavy"], true);
    assert!(row(BROWSER).get("idleHeavy").is_none());
    // A get while subscribed carries the tag too, and moves nothing.
    let got = data(a.snapshot(&fixture("root-desktop.t1"), t0 + Duration::from_secs(8)));
    assert!(by_id(&got, CARGO).idle_heavy);
    // Alt-tab to the terminal: cargo is in view at once, and the browser,
    // focused a second ago, is not "idle" (review of PR #20, B1).
    fake.focus(t0 + Duration::from_secs(7), Some(3000));
    let d = alternate(&mut a, t0, 8, 8);
    assert!(!by_id(&d, CARGO).idle_heavy, "cleared at once");
    assert!(!by_id(&d, BROWSER).idle_heavy, "it had focus a second ago");
    // Window [7, 10] still touches the browser's focus, which ended at 7.
    let d = alternate(&mut a, t0, 10, 10);
    assert!(!by_id(&d, BROWSER).idle_heavy, "focus lost only 3 s ago");
    // Window [9, 12]: a whole window since the browser lost focus.
    let d = alternate(&mut a, t0, 12, 12);
    assert!(
        by_id(&d, BROWSER).idle_heavy,
        "a full window since focus was lost"
    );
    assert!(!by_id(&d, CARGO).idle_heavy, "the terminal still has focus");
    // Left without subscribers: the source stops and the windows go.
    assert!(a.windows() > 0);
    a.idle();
    assert_eq!(a.windows(), 0);
    assert_eq!(
        *fake.calls.lock().unwrap(),
        (1, 1),
        "started once, stopped once"
    );
    // A page opened later starts the source again and counts afresh.
    let d = alternate(&mut a, t0, 14, 16);
    assert_eq!(fake.calls.lock().unwrap().0, 2);
    assert!(!by_id(&d, BROWSER).idle_heavy);
}

#[test]
fn no_window_in_focus_for_a_whole_window_tags() {
    let s = scratch("blank");
    let mut a = probed(&pacman(&s.0));
    let fake = FakeFocus::up();
    let t0 = Instant::now();
    fake.focus(t0, Some(2001));
    fake.focus(t0 + Duration::from_secs(1), None);
    a.set_focus(Box::new(fake.clone()));
    a.set_thresholds(Thresholds {
        window: Duration::from_secs(3),
        cpu: 20.0,
    });
    let d = alternate(&mut a, t0, 0, 4);
    assert!(!by_id(&d, BROWSER).idle_heavy, "not a full window yet");
    let d = alternate(&mut a, t0, 6, 6);
    assert!(
        by_id(&d, BROWSER).idle_heavy,
        "no window since 1 s: window [3, 6]"
    );
    assert!(by_id(&d, CARGO).idle_heavy);
}

#[test]
fn a_focus_source_that_is_not_running_is_asked_again_every_30_s() {
    let s = scratch("retry");
    let mut a = probed(&pacman(&s.0));
    let fake = FakeFocus::default();
    a.set_focus(Box::new(fake.clone()));
    let t0 = Instant::now();
    let root = fixture("root-desktop");
    let starts = || fake.calls.lock().unwrap().0;
    for at in [0, 2, 28] {
        a.sample(&root, t0 + Duration::from_secs(at));
    }
    assert_eq!(starts(), 1, "once, not every tick");
    a.sample(&root, t0 + Duration::from_secs(30));
    assert_eq!(starts(), 2, "again after 30 s (review of PR #20, S3)");
    *fake.available.lock().unwrap() = true;
    let d = data(a.sample(&root, t0 + Duration::from_secs(60)));
    assert_eq!(starts(), 3);
    assert_eq!(
        d.focus.as_deref(),
        Some("fake"),
        "found while the page is open"
    );
    a.sample(&root, t0 + Duration::from_secs(62));
    assert_eq!(starts(), 3, "a running source is not started again");
    a.idle();
    a.sample(&root, t0 + Duration::from_secs(64));
    assert_eq!(starts(), 4, "after idle, at once");
}

#[test]
fn without_a_focus_source_nothing_is_tagged() {
    let s = scratch("nofocus");
    let mut a = probed(&pacman(&s.0));
    let fake = FakeFocus::default();
    a.set_focus(Box::new(fake.clone()));
    a.set_thresholds(Thresholds {
        window: Duration::from_secs(3),
        cpu: 20.0,
    });
    let d = alternate(&mut a, Instant::now(), 0, 12);
    assert!(d.focus.is_none());
    assert!(d.groups.iter().all(|g| !g.idle_heavy));
    assert_eq!(a.windows(), 0, "no windows are kept without a source");
    assert_eq!(
        fake.calls.lock().unwrap().0,
        1,
        "asked once, not every tick"
    );
}

fn on() -> Value {
    json!({"trends": {"topApps": true}})
}

#[test]
fn the_top_apps_row_names_the_heaviest_and_only_when_it_says_something() {
    let s = scratch("top");
    let mut a = probed(&pacman(&s.0));
    let t0 = Instant::now();
    // No subscriber: the first minute only takes its baseline.
    assert!(a.top_apps(&fixture("root-desktop"), &on(), t0).is_empty());
    // Two seconds' worth of the twin's ticks over a whole minute: cargo
    // at 3 % of a core, under the 10 % rule.
    assert!(
        a.top_apps(
            &fixture("root-desktop.t1"),
            &on(),
            t0 + Duration::from_secs(60)
        )
        .is_empty()
    );
    let walks = a.walks();
    assert_eq!(walks, 2, "one walk a minute (the probe walks none)");
    // The subscriber's frames: cargo at 91 %, read from the last sample.
    let _ = two_samples(&mut a);
    let walks = a.walks();
    let rows = a.top_apps(&fixture("root-desktop"), &on(), Instant::now());
    assert_eq!(a.walks(), walks, "no walk of its own while subscribed");
    assert_eq!(rows.len(), 1);
    let (kind, row) = &rows[0];
    assert_eq!(*kind, apps::TOP_APPS);
    assert_eq!(
        row["cpu"],
        json!([["cargo", 91.0], ["Example Browser", 66.0], ["vite", 12.0]])
    );
    assert_eq!(row["mem"][0], json!(["cargo", 1_710_080_000_u64]));
    assert_eq!(row["mem"][1][0], "Example Browser");
    assert_eq!(row["mem"][2][0], "Hyprland");
    assert_eq!(row["procs"], 53);
    assert_eq!(
        row.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["cpu", "mem", "procs"],
        "names only: no ids, units, pids or command lines"
    );
    let text = row.to_string();
    for no in ["scope:", "session:", "2001", "/usr/", "--"] {
        assert!(!text.contains(no), "{no} in {text}");
    }
    // The same six names again: nothing.
    assert!(
        a.top_apps(&fixture("root-desktop"), &on(), Instant::now())
            .is_empty()
    );
    // Off: nothing, and no walk.
    a.idle();
    let walks = a.walks();
    let off = json!({"trends": {"topApps": false}});
    assert!(
        a.top_apps(&fixture("root-desktop"), &off, Instant::now())
            .is_empty()
    );
    assert_eq!(a.walks(), walks);
}

#[test]
fn the_top_apps_row_from_walks_a_minute_apart() {
    let s = scratch("topwalk");
    let mut a = probed(&pacman(&s.0));
    let t0 = Instant::now();
    assert!(a.top_apps(&fixture("root-desktop"), &on(), t0).is_empty());
    // Two seconds apart, so the rates are the twin's.
    let rows = a.top_apps(
        &fixture("root-desktop.t1"),
        &on(),
        t0 + Duration::from_secs(2),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1["cpu"][0], json!(["cargo", 91.0]));
    // A baseline older than two and a half minutes gives nothing.
    assert!(
        a.top_apps(
            &fixture("root-desktop"),
            &on(),
            t0 + Duration::from_secs(300)
        )
        .is_empty()
    );
}

fn inspect_with(
    a: &mut Apps,
    root: &Root,
    params: Value,
) -> Result<Value, (mirqab_core::proto::Code, String)> {
    a.query(root, "inspect", &params).expect("known").finish()
}

#[test]
fn the_inspector_asked_with_a_start_time_answers_gone_for_another_process() {
    let s = scratch("s5");
    let mut a = probed(&pacman(&s.0));
    // pid 7000 is `oldjob` (start 300000) in root-desktop and `newjob`
    // (start 360000) in the twin.
    let ok = inspect_with(
        &mut a,
        &fixture("root-desktop"),
        json!({"pid": 7000, "start": 300000}),
    )
    .unwrap();
    assert_eq!(ok["name"], "oldjob");
    let (code, msg) = inspect_with(
        &mut a,
        &fixture("root-desktop.t1"),
        json!({"pid": 7000, "start": 300000}),
    )
    .unwrap_err();
    assert_eq!(code, mirqab_core::proto::Code::Failed);
    assert_eq!(msg, "pid 7000 is another process now");
    let log = std::fs::read_to_string(s.0.join("pacman.log")).unwrap();
    assert!(!log.contains("newjob"), "no verdict was formed: {log}");
    // Without a start, as today: the process at the pid now.
    let now = inspect_with(&mut a, &fixture("root-desktop.t1"), json!({"pid": 7000})).unwrap();
    assert_eq!(now["name"], "newjob");
    let (code, _) = inspect_with(
        &mut a,
        &fixture("root-desktop"),
        json!({"pid": 7000, "start": "x"}),
    )
    .unwrap_err();
    assert_eq!(code, mirqab_core::proto::Code::BadRequest);
}

#[test]
fn the_inspector_asks_pacman_after_the_lock() {
    // The verdict's package question is a closure the server runs once it
    // let go of the sampler's lock; a kernel thread needs none.
    let s = scratch("later");
    let mut a = probed(&pacman(&s.0));
    let root = fixture("root-desktop");
    let later = a.query(&root, "inspect", &json!({"pid": 2001})).unwrap();
    assert!(matches!(later, mirqab_core::sampler::Answer::Later(_)));
    let log = std::fs::read_to_string(s.0.join("pacman.log")).unwrap();
    assert!(log.is_empty(), "nothing asked yet: {log}");
    assert_eq!(later.finish().unwrap()["verdict"]["code"], "packaged");
    let kernel = a.query(&root, "inspect", &json!({"pid": 3})).unwrap();
    assert!(matches!(kernel, mirqab_core::sampler::Answer::Ready(_)));
}

/// A process of the user (uid 1000) in a bare root: stat, status, and the
/// login session's cgroup (no user unit claims it).
fn own_proc(root: &Path, pid: u32, comm: &str) -> PathBuf {
    let p = root.join(format!("proc/{pid}"));
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("stat"),
        format!("{pid} ({comm}) S 1 {pid} {pid} 0 -1 4194560 0 0 0 0 5 5 0 0 20 0 1 0 100 0 0 0"),
    )
    .unwrap();
    std::fs::write(p.join("status"), "Uid:\t1000\t1000\t1000\t1000\n").unwrap();
    std::fs::write(
        p.join("cgroup"),
        "0::/user.slice/user-1000.slice/session-2.scope\n",
    )
    .unwrap();
    p
}

fn drm_fdinfo(client: u32, vram_kib: u64) -> String {
    format!(
        "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\ndrm-client-id:\t{client}\ndrm-memory-vram:\t{vram_kib} KiB\n"
    )
}

#[test]
fn gpu_memory_comes_only_from_drm_fds_and_not_past_64_of_them() {
    let s = bare_root("gpu");
    let p = own_proc(&s.0, 9300, "render");
    std::fs::create_dir_all(p.join("fd")).unwrap();
    std::fs::create_dir_all(p.join("fdinfo")).unwrap();
    std::os::unix::fs::symlink("/dev/dri/renderD128", p.join("fd/3")).unwrap();
    std::os::unix::fs::symlink("/tmp/not-a-gpu", p.join("fd/4")).unwrap();
    std::os::unix::fs::symlink("socket:[1]", p.join("fd/5")).unwrap();
    std::fs::write(p.join("fdinfo/3"), drm_fdinfo(1, 1024)).unwrap();
    // It would parse, but its fd is no DRM node: never read.
    std::fs::write(p.join("fdinfo/4"), drm_fdinfo(2, 4096)).unwrap();
    std::fs::write(p.join("fdinfo/5"), "pos:\t0\n").unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let v = inspect(&mut a, &root, 9300);
    assert_eq!(v["gpu"]["vramBytes"], 1024 * 1024, "{v}");
    assert_eq!(v["files"]["count"], 3);

    // 1 100 fds of which 8 are DRM, two clients: the row is there, each
    // client counted once, and no other fdinfo read (review of PR #20,
    // nit 3; the old cap on the listing left it out).
    let q = own_proc(&s.0, 9301, "many");
    std::fs::create_dir_all(q.join("fd")).unwrap();
    std::fs::create_dir_all(q.join("fdinfo")).unwrap();
    let drm_fds = [3u32, 140, 277, 414, 551, 688, 825, 962];
    for n in 0..1100u32 {
        if drm_fds.contains(&n) {
            std::os::unix::fs::symlink("/dev/dri/renderD128", q.join(format!("fd/{n}"))).unwrap();
            std::fs::write(q.join(format!("fdinfo/{n}")), drm_fdinfo(n % 2, 512)).unwrap();
        } else {
            std::os::unix::fs::symlink(format!("socket:[{n}]"), q.join(format!("fd/{n}"))).unwrap();
            // It would parse, but its fd is no DRM node: never read.
            std::fs::write(q.join(format!("fdinfo/{n}")), drm_fdinfo(100 + n, 4096)).unwrap();
        }
    }
    let v = inspect(&mut a, &root, 9301);
    assert_eq!(v["gpu"]["vramBytes"], 2 * 512 * 1024, "{v}");
    assert_eq!(v["files"]["count"], 1100);

    // Exactly 64 DRM fds: the row is there (the cap's boundary).
    let e = own_proc(&s.0, 9303, "edge");
    std::fs::create_dir_all(e.join("fd")).unwrap();
    std::fs::create_dir_all(e.join("fdinfo")).unwrap();
    let cap = mirqab_core::samplers::apps::inspect::MAX_DRM_FDINFO as u32;
    for n in 0..cap {
        std::os::unix::fs::symlink("/dev/dri/renderD128", e.join(format!("fd/{n}"))).unwrap();
        std::fs::write(e.join(format!("fdinfo/{n}")), drm_fdinfo(n, 1)).unwrap();
    }
    let v = inspect(&mut a, &root, 9303);
    assert_eq!(v["gpu"]["vramBytes"], u64::from(cap) * 1024, "{v}");
    assert_eq!(v["files"]["count"], cap);

    // Past 64 DRM fds the row is left out (and the rest still answers).
    let r = own_proc(&s.0, 9302, "lots");
    std::fs::create_dir_all(r.join("fd")).unwrap();
    std::fs::create_dir_all(r.join("fdinfo")).unwrap();
    for n in 0..=mirqab_core::samplers::apps::inspect::MAX_DRM_FDINFO as u32 {
        std::os::unix::fs::symlink("/dev/dri/renderD128", r.join(format!("fd/{n}"))).unwrap();
        std::fs::write(r.join(format!("fdinfo/{n}")), drm_fdinfo(n, 1)).unwrap();
    }
    let v = inspect(&mut a, &root, 9302);
    assert!(v.get("gpu").is_none(), "{}", v["gpu"]);
    assert_eq!(v["name"], "lots");
}

#[test]
fn a_fifo_for_flatpak_info_does_not_stall_the_walk() {
    let s = bare_root("fifo");
    let p = own_proc(&s.0, 9400, "sneaky");
    std::fs::create_dir_all(p.join("root")).unwrap();
    let st = std::process::Command::new("mkfifo")
        .arg(p.join("root/.flatpak-info"))
        .status()
        .expect("mkfifo runs");
    assert!(st.success());
    let base = s.0.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let root = Root::new(&base);
        let mut a = Apps::with_pacman(base.join("pacman").to_string_lossy());
        a.probe(&root).unwrap();
        let d = data(a.sample(&root, Instant::now()));
        let v = inspect(&mut a, &root, 9400);
        tx.send((d, v)).unwrap();
    });
    let (d, v) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the walk and the inspector finish");
    let g = d.groups.iter().find(|g| g.pids.contains(&9400)).unwrap();
    assert_ne!(g.kind, "flatpak");
    assert!(v.get("flatpak").is_none(), "{v}");
    // A symlink there is not followed either.
    let q = own_proc(&s.0, 9401, "linked");
    std::fs::create_dir_all(q.join("root")).unwrap();
    std::fs::write(
        s.0.join("real-info"),
        "[Application]\nname=org.example.Fake\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(s.0.join("real-info"), q.join("root/.flatpak-info")).unwrap();
    let root = Root::new(&s.0);
    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let d = data(a.sample(&root, Instant::now()));
    let g = d.groups.iter().find(|g| g.pids.contains(&9401)).unwrap();
    assert_ne!(g.kind, "flatpak");
}

#[test]
fn flatpak_info_is_not_opened_through_a_fuse_root() {
    // Audit 2026-11-21, L7: O_NONBLOCK does not stop an open on a FUSE
    // mount from waiting on its server, so a process whose root (or whose
    // `/.flatpak-info`) is on a hung FUSE mount would stall the walk under
    // the sampler lock. The root is pinned with O_PATH and its mount id
    // looked up in the process's mountinfo: only a listed tmpfs clears it
    // (a Flatpak's root is one). Mount points do not matter; the id does. The
    // review of the first fix found a process that chroots below a FUSE
    // mount and puts a tmpfs over its own `/`: mountinfo then shows only
    // that tmpfs at `/`, but its root is still the directory beneath.
    let s = bare_root("fuse");
    let root = Root::new(&s.0);
    // The scratch tree is on one real mount; the fixture's mountinfo
    // names it by its real id, and other mounts by an id that is not it.
    let mnt = root.mount_id("proc").unwrap();
    let other = if mnt == 7 { 8 } else { 7 };
    let info = "[Application]\nname=org.example.Notes\n";
    let rows = [
        // The root's own mount is FUSE.
        (
            9500,
            format!("{mnt} 1 0:40 / / rw - fuse.sshfs example:/ rw\n"),
        ),
        (
            9502,
            format!(
                "{other} 1 254:1 / / rw - ext4 none rw\n{mnt} {other} 0:40 / / rw - fuse example rw\n"
            ),
        ),
        // The review's bypass: a tmpfs over `/` that is not the root's mount.
        (9508, format!("{other} 1 0:44 / / rw - tmpfs tmpfs rw\n")),
        // A chroot below a mount: the root's mount is not listed at all.
        (9506, format!("{other} 1 0:5 / /proc rw - proc proc rw\n")),
        (9507, String::new()),
        // A line for the root's mount without a type is no clearance.
        (9509, format!("{mnt} 1 0:40 / / rw\n")),
        // Nor is any type but tmpfs: an overlay can have a FUSE lower layer.
        (9510, format!("{mnt} 1 0:45 / / rw - overlay overlay rw\n")),
        (9511, format!("{mnt} 1 254:1 / / rw - ext4 none rw\n")),
        // Cleared: the root's mount is listed as a tmpfs, wherever it is
        // mounted, and FUSE elsewhere is no objection.
        (
            9503,
            format!("{mnt} 1 0:44 /newroot / rw - tmpfs tmpfs rw\n"),
        ),
        (
            9504,
            format!(
                "{mnt} 1 0:44 / / rw - tmpfs tmpfs rw\n{other} {mnt} 0:42 / /home rw - fuse.sshfs example:/ rw\n"
            ),
        ),
        (
            9512,
            format!("{mnt} 1 0:44 / /srv\\040x rw shared:3 - tmpfs tmpfs rw\n"),
        ),
    ];
    for (pid, mounts) in &rows {
        let p = own_proc(&s.0, *pid, "notes");
        std::fs::create_dir_all(p.join("root")).unwrap();
        std::fs::write(p.join("root/.flatpak-info"), info).unwrap();
        std::fs::write(p.join("mountinfo"), mounts).unwrap();
    }
    // No mountinfo at all (a fixture without one) still reads the file;
    // on the live machine that would be an objection.
    let p = own_proc(&s.0, 9505, "notes");
    std::fs::create_dir_all(p.join("root")).unwrap();
    std::fs::write(p.join("root/.flatpak-info"), info).unwrap();

    let mut a = Apps::with_pacman(s.0.join("pacman").to_string_lossy());
    a.probe(&root).unwrap();
    let d = data(a.sample(&root, Instant::now()));
    let kind = |pid: u32| {
        d.groups
            .iter()
            .find(|g| g.pids.contains(&pid))
            .unwrap()
            .kind
            .clone()
    };
    for pid in [9500, 9502, 9506, 9507, 9508, 9509, 9510, 9511] {
        assert_ne!(kind(pid), "flatpak", "{pid}");
        let v = inspect(&mut a, &root, pid);
        assert!(v.get("flatpak").is_none(), "{pid}: {v}");
    }
    for pid in [9503, 9504, 9505, 9512] {
        assert_eq!(kind(pid), "flatpak", "{pid}");
        let v = inspect(&mut a, &root, pid);
        assert_eq!(v["flatpak"], "org.example.Notes", "{pid}: {v}");
    }
}

#[test]
fn the_flatpak_info_mount_check_reads_mountinfo_when_asked() {
    // Review of audit 2026-11-21, L7: Linux hands a freed mount id
    // straight back out, so a mountinfo read before the root is pinned can
    // describe another mount with the same id: a process unmounts its
    // tmpfs N, mounts FUSE (which gets N) and chroots into it, and the
    // stale "N tmpfs" line would clear the FUSE root. While a descriptor
    // holds a mount no other live mount can carry its id, so the check
    // reads mountinfo each time it is asked, after the pin.
    let s = bare_root("fuse-late");
    let p = own_proc(&s.0, 9600, "notes");
    let root = Root::new(&s.0);
    std::fs::write(p.join("mountinfo"), "21 1 0:44 / / rw - tmpfs tmpfs rw\n").unwrap();
    let clear = mirqab_core::samplers::apps::procfs::tmpfs_now(&root, "proc/9600");
    std::fs::write(
        p.join("mountinfo"),
        "21 1 0:40 / / rw - fuse.sshfs example:/ rw\n",
    )
    .unwrap();
    assert!(
        !clear(21),
        "a listing from before the pin cleared a FUSE mount"
    );
    std::fs::write(p.join("mountinfo"), "21 1 0:44 / / rw - tmpfs tmpfs rw\n").unwrap();
    assert!(clear(21));
    std::fs::write(p.join("mountinfo"), "").unwrap();
    assert!(!clear(21));
}

fn cargo_cpu(v: Value) -> f64 {
    by_id(&data(v), CARGO).cpu.expect("a rate")
}

#[test]
fn each_get_is_served_its_own_primed_reading_oldest_first() {
    // Review of PR #20, S1: two gets primed 300 ms apart each read their
    // own first reading, half a second old, never one a millisecond old.
    let s = scratch("prime");
    let mut a = probed(&pacman(&s.0));
    let (t0, t1) = (fixture("root-desktop"), fixture("root-desktop.t1"));
    let gap = a
        .prime(&t0, Instant::now())
        .expect("no subscriber baseline: a wait");
    assert_eq!(gap, Duration::from_millis(500));
    std::thread::sleep(Duration::from_millis(300));
    a.prime(&t0, Instant::now()).unwrap();
    assert_eq!(a.primed(), 2);
    std::thread::sleep(Duration::from_millis(200));
    // The twin's ticks are two seconds' worth: 91 % over 2 s is 364 % over
    // the half second between the readings.
    let near = |c: f64| (c - 364.0).abs() < 364.0 * 0.2;
    let first = cargo_cpu(a.snapshot(&t1, Instant::now()));
    assert!(near(first), "{first}");
    std::thread::sleep(Duration::from_millis(300));
    let second = cargo_cpu(a.snapshot(&t1, Instant::now()));
    assert!(near(second), "{second}");
    assert_eq!(a.primed(), 0);
    // A reading 2 ms old is too young: dropped, and the snapshot takes its
    // own two readings half a second apart.
    a.prime(&t0, Instant::now()).unwrap();
    std::thread::sleep(Duration::from_millis(2));
    let t = Instant::now();
    a.snapshot(&t0, Instant::now());
    assert!(
        t.elapsed() >= Duration::from_millis(400),
        "{:?}",
        t.elapsed()
    );
    // idle clears the queue.
    a.prime(&t0, Instant::now()).unwrap();
    a.idle();
    assert_eq!(a.primed(), 0);
    // A young subscriber baseline needs no wait.
    let now = Instant::now();
    a.sample(&t0, now);
    assert!(a.prime(&t0, now + Duration::from_secs(2)).is_none());
}

#[test]
fn the_process_stream_serves_each_get_its_own_primed_reading() {
    let mut p = process_probed();
    let (t0, t1) = (fixture("root-desktop"), fixture("root-desktop.t1"));
    let params = [json!({"pid": 2001})];
    p.wants(&params);
    p.prime(&t0, Instant::now()).expect("a wait");
    std::thread::sleep(Duration::from_millis(300));
    p.wants(&params);
    p.prime(&t0, Instant::now()).expect("a wait");
    std::thread::sleep(Duration::from_millis(200));
    let cpu = |v: Value| {
        process::view(&v, &params[0])["cpu"]
            .as_f64()
            .expect("a rate")
    };
    p.wants(&params);
    let first = cpu(p.snapshot(&t1, Instant::now()));
    std::thread::sleep(Duration::from_millis(300));
    p.wants(&params);
    let second = cpu(p.snapshot(&t1, Instant::now()));
    // The browser's main process: its two-second delta over half a second.
    let two_s = {
        let mut q = process_probed();
        q.wants(&params);
        let t = Instant::now();
        q.sample(&t0, t);
        process::view(&q.sample(&t1, t + Duration::from_secs(2)), &params[0])["cpu"]
            .as_f64()
            .unwrap()
    };
    for c in [first, second] {
        assert!(
            (c - two_s * 4.0).abs() < two_s * 4.0 * 0.2,
            "{c} vs {}",
            two_s * 4.0
        );
    }
    let t = Instant::now();
    p.wants(&params);
    p.snapshot(&t1, Instant::now());
    assert!(
        t.elapsed() >= Duration::from_millis(400),
        "the queue is empty"
    );
}

/// A get of `pid` on `root`, with its `wants` set as the server sets it.
fn process_get(p: &mut Process, root: &Root, pid: u32) -> Value {
    let ask = json!({"pid": pid});
    p.wants(std::slice::from_ref(&ask));
    let got = p.snapshot(root, Instant::now());
    process::view(&got, &ask)
}

#[test]
fn a_primed_reading_left_by_one_get_never_answers_another_pid() {
    // Audit: a get answered from the subscribers' baseline left its primed
    // reading queued, and the next get of another pid took it and answered
    // `cpu: null`.
    let (t0, t1) = (fixture("root-desktop"), fixture("root-desktop.t1"));
    let mut p = process_probed();
    p.wants(&[json!({"pid": 2001})]);
    p.sample(&t0, Instant::now());
    // The baseline is too young for the get, which primes; by its snapshot
    // the baseline serves, and the primed reading goes with the get.
    p.prime(&t0, Instant::now())
        .expect("a young baseline: a wait");
    p.wants(&[json!({"pid": 2001})]);
    let from_base = p.snapshot(&t1, Instant::now() + Duration::from_millis(1200));
    assert!(process::view(&from_base, &json!({"pid": 2001}))["cpu"].is_number());
    assert_eq!(p.primed(), 0, "the get's reading went with it");

    // The process left in the gap: its reading goes too.
    let s = scratch("procleft");
    std::fs::create_dir_all(s.0.join("proc")).unwrap();
    p.wants(&[json!({"pid": 2010})]);
    p.prime(&t0, Instant::now()).expect("a wait");
    assert_eq!(
        process_get(&mut p, &Root::new(&s.0), 2010),
        json!({"pid": 2010, "gone": true})
    );
    assert_eq!(p.primed(), 0);

    // Two gets whose snapshots run in the other order each take their own.
    let mut p = process_probed();
    p.wants(&[json!({"pid": 2001})]);
    p.prime(&t0, Instant::now()).expect("a wait");
    p.wants(&[json!({"pid": 2010})]);
    p.prime(&t0, Instant::now()).expect("a wait");
    std::thread::sleep(Duration::from_millis(500));
    let t = Instant::now();
    assert!(process_get(&mut p, &t1, 2010)["cpu"].is_number());
    assert!(process_get(&mut p, &t1, 2001)["cpu"].is_number());
    assert!(
        t.elapsed() < Duration::from_millis(400),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(p.primed(), 0);
}

#[test]
fn a_baseline_about_to_go_stale_is_not_queued_for_a_get() {
    // Review of PR #26: a baseline 9.7 s old was queued, went stale before
    // `snapshot`, which then waited half a second under the lock.
    let t0 = fixture("root-desktop");
    let ask = [json!({"pid": 2001})];
    let mut p = process_probed();
    p.wants(&ask);
    p.sample(&t0, Instant::now() - Duration::from_millis(9700));
    p.wants(&ask);
    assert_eq!(
        p.prime(&t0, Instant::now()),
        Some(Duration::from_millis(500)),
        "a fresh reading and the wait"
    );
    // One 9 s old still serves at once.
    let mut p = process_probed();
    p.wants(&ask);
    p.sample(&t0, Instant::now() - Duration::from_secs(9));
    p.wants(&ask);
    assert_eq!(p.prime(&t0, Instant::now()), None);
}

#[test]
fn a_get_whose_baseline_a_tick_replaced_does_not_wait_under_the_lock() {
    // Audit: `prime` found the subscribers' baseline usable and queued
    // nothing; a tick between the get's two locked steps replaced it with
    // a fresh one, and `snapshot` slept half a second holding the lock.
    let (t0, t1) = (fixture("root-desktop"), fixture("root-desktop.t1"));
    let ask = [json!({"pid": 2001})];
    let mut p = process_probed();
    let past = Instant::now() - Duration::from_secs(2);
    p.wants(&ask);
    p.sample(&t0, past);
    p.wants(&ask);
    assert!(
        p.prime(&t0, Instant::now()).is_none(),
        "the baseline serves"
    );
    p.wants(&ask);
    p.sample(&t1, Instant::now());
    let t = Instant::now();
    let got = process_get(&mut p, &t1, 2001);
    assert!(
        t.elapsed() < Duration::from_millis(400),
        "{:?}",
        t.elapsed()
    );
    // 80 ticks of 100 over the 2 s since the baseline the get primed with.
    let cpu = got["cpu"].as_f64().expect("a rate");
    assert!((cpu - 40.0).abs() < 4.0, "{cpu}");
    assert_eq!(p.primed(), 0);
}
