//! The `apps` sampler and the process inspector on fixture roots (plan 006,
//! Tests): each grouping rule picks the expected group; the System group;
//! kernel threads hidden; CPU and storage rates from the `.t1` twin;
//! sorting, `limit` and `expand`; a pid reused with a new start time is a
//! new process; the inspector's golden output for five processes, the
//! verdict rules, `pacman` asked once per path, and unreadable files left
//! out rather than failing.

use mirqab_core::Root;
use mirqab_core::proto::stream::{AppGroup, AppsData, Inspect};
use mirqab_core::sampler::Sampler;
use mirqab_core::samplers::apps::{self, Apps};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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
    // Rule 3: a rootless container under the user manager.
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
    assert_eq!(d.totals.procs, 37);
    assert_eq!(d.totals.cores, Some(16));
    assert_eq!(d.groups.iter().map(|g| g.procs).sum::<u32>(), 37);
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
    assert_eq!(second.totals.cpu, Some(198.0));
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
    let name = apps::view(&full, &json!({"sort": "name", "limit": 3}));
    assert_eq!(names(&name), ["Calculator", "cargo", "devserver", "System"]);
    let default = apps::view(&full, &Value::Null);
    assert_eq!(default["groups"].as_array().unwrap().len(), 18);
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
    let got = apps::view(&serde_json::to_value(&second).unwrap(), &json!({}));
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
