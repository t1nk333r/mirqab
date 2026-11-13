//! `Root` keeps reads inside the tree (plan 001, "Tests"): a path with `..`
//! or an absolute path cannot name a file outside it.

use mirqab_core::root::Root;
use std::path::PathBuf;

fn fixture() -> Root {
    Root::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/root-desktop"))
}

#[test]
fn reads_inside_the_tree() {
    let r = fixture();
    assert!(r.read("proc/meminfo").unwrap().starts_with("MemTotal:"));
    assert!(r.exists("proc/stat"));
    assert!(r.read_dir("proc").unwrap().contains(&"self".to_string()));
}

#[test]
fn parent_components_cannot_escape() {
    let r = fixture();
    for p in [
        "../root-laptop/proc/stat",
        "proc/../../root-laptop/proc/stat",
        "proc/self/../../../..",
    ] {
        assert!(r.read(p).is_err(), "{p} escaped");
        assert!(!r.exists(p), "{p} escaped");
    }
}

#[test]
fn absolute_paths_cannot_escape() {
    let r = fixture();
    for p in ["/etc/hostname", "/proc/self/status", "//etc/passwd"] {
        assert!(r.path(p).is_err(), "{p} escaped");
        assert!(r.read(p).is_err(), "{p} escaped");
    }
}
