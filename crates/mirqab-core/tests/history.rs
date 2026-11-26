//! History (plan 001 phase C, architecture §6): the schema, counter
//! differences between two fixture moments, gaps, the ranges' bucket counts,
//! pruning, a broken file moved aside, and 30 days × 64 series under 40 MB,
//! with a `top-apps` event every minute (plan 006, part 4: the worst case).
//! Audit fixes: the file and its directory are private (0600, 0700), a
//! `from`/`to` span is clamped and bucketed, and a file found corrupt while
//! open is moved aside and replaced. Plan 024: one corrupt at open that
//! cannot be moved is retried, and one deleted by hand is replaced at once.

use mirqab_core::history::{self, History};
use mirqab_core::root::Root;
use mirqab_core::samplers::system::System;
use rusqlite::params;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scratch(name: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("mirqab-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// A unix time on a 2-hour boundary, so every range's buckets align.
const NOW: i64 = 1_790_006_400;

#[test]
fn the_schema_is_created_with_the_first_series() {
    let d = scratch("schema");
    let h = History::open(&d.0.join("history.db"), NOW).unwrap();
    let schema: String = h
        .conn()
        .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(schema, history::SCHEMA);
    let keys: Vec<String> = h
        .conn()
        .prepare("SELECT key FROM series ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        keys,
        history::SERIES.iter().map(|s| s.0).collect::<Vec<_>>()
    );
    let mode: String = h
        .conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

thread_local! {
    static T0: Instant = Instant::now();
}

#[test]
fn counters_difference_across_a_minute_and_gauges_are_point_samples() {
    let mut s = System::new();
    // The first minute has no baseline for the counter: memory only.
    let first = s.minute_at(&fixture("root-desktop"), NOW as u64, T0.with(|t| *t));
    assert!(first.iter().all(|(k, _)| *k != "cpu.total"));
    let second = s.minute_at(
        &fixture("root-desktop.t1"),
        (NOW + 60) as u64,
        T0.with(|t| *t) + Duration::from_secs(60),
    );
    let get = |k: &str| second.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    // Each of 16 CPUs: +150 user, +50 system, +800 idle → 20.00 % busy.
    assert_eq!(get("cpu.total"), Some(2000));
    assert_eq!(get("mem.available"), Some(48_000_000 * 1024));
    assert_eq!(get("mem.used"), Some((65_536_000 - 48_000_000) * 1024));
    assert_eq!(get("swap.used"), Some((8_388_608 - 8_000_000) * 1024));
}

#[test]
fn a_gap_stores_no_counter() {
    let mut s = System::new();
    s.minute_at(&fixture("root-desktop"), NOW as u64, T0.with(|t| *t));
    let after_sleep = s.minute_at(
        &fixture("root-desktop.t1"),
        (NOW + 3600) as u64,
        T0.with(|t| *t) + Duration::from_secs(3600),
    );
    assert!(
        after_sleep.iter().all(|(k, _)| *k != "cpu.total"),
        "an hour asleep is not one minute"
    );
}

#[test]
fn a_minimal_machine_still_writes_what_it_has() {
    let mut s = System::new();
    let m = s.minute_at(&fixture("root-minimal"), NOW as u64, T0.with(|t| *t));
    assert!(m.iter().any(|(k, _)| *k == "mem.used"));
    assert!(
        m.iter().all(|(k, _)| *k != "swap.used"),
        "no swap line, no swap series"
    );
}

/// Writes `minutes` minutes ending `end_ago` minutes before NOW, minute by
/// minute across every series, as the writer does. Values are realistic in
/// size. Of the series architecture §6.1 lists, about one in four holds byte
/// counts (memory, swap, VRAM, network and disk bytes); the rest are
/// percentages, temperatures, speeds and counts. So every fourth series here
/// holds tens of gigabytes and the others values under 10,000.
fn fill_range(h: &History, keys: &[i64], minutes: i64, end_ago: i64) {
    let tx = h.conn().unchecked_transaction().unwrap();
    {
        let mut stmt = tx
            .prepare("INSERT OR REPLACE INTO sample (series, ts, v) VALUES (?1, ?2, ?3)")
            .unwrap();
        for i in 0..minutes {
            let ts = history::minute(NOW) - end_ago - minutes + i;
            // Values vary with the minute itself, not the loop index, so a
            // fill one minute at a time writes the same values as one long one.
            let m = ts.rem_euclid(1_000_000);
            for id in keys {
                let v = if id % 4 == 0 {
                    17_000_000_000 + m * 4096 + id
                } else {
                    (m % 100) * 100
                };
                stmt.execute(params![id, ts, v]).unwrap();
            }
        }
    }
    tx.commit().unwrap();
}

fn fill(h: &History, keys: &[i64], minutes: i64) {
    fill_range(h, keys, minutes, 0);
}

#[test]
fn each_range_returns_its_bucket_count() {
    let d = scratch("ranges");
    let h = History::open(&d.0.join("history.db"), NOW).unwrap();
    fill(&h, &[1], 30 * 1440);
    for (name, points) in [("1h", 60), ("24h", 360), ("7d", 336), ("30d", 360)] {
        let (span, bucket) = history::range(name).unwrap();
        let v = h.query(&["cpu.total"], NOW - span, NOW, bucket).unwrap();
        assert_eq!(v["bucketSec"], bucket, "{name}");
        assert_eq!(
            v["series"]["cpu.total"]["points"].as_array().unwrap().len(),
            points,
            "{name}"
        );
    }
    // Avg and max per bucket, unscaled: the 1 h range's minutes hold 0..99 %
    // in steps, and a 30 d bucket keeps the spike.
    let v = h.query(&["cpu.total"], NOW - 7200, NOW, 7200).unwrap();
    let p = &v["series"]["cpu.total"]["points"][0];
    assert_eq!(p[2], 99.0);
    assert_eq!(v["series"]["cpu.total"]["unit"], "percent");
    assert_eq!(
        h.query(&["no.such"], NOW - 60, NOW, 60).unwrap()["series"]["no.such"],
        serde_json::Value::Null
    );
}

#[test]
fn a_from_to_span_is_clamped_to_what_is_kept_and_bucketed_to_360_points() {
    // Plan 001, deferred to plan 005: an unclamped span at 1-minute
    // buckets built hundreds of MB, and i64::MIN panicked a debug build.
    assert_eq!(history::minute(i64::MIN), i64::MIN.div_euclid(60));
    // A whole 30 days at 2 hours touches 361 buckets: trimmed at its
    // oldest end to 360 (review of PR #26).
    let (from, to, bucket) = history::span(i64::MIN, i64::MAX, NOW).unwrap();
    assert_eq!(
        (from, to, bucket),
        (NOW - history::KEEP_SECS + 7200, NOW + 60, 7200)
    );
    assert_eq!(history::span(0, NOW, NOW).unwrap().2, 7200);
    // Each range's own span gets its own bucket.
    for (_, span, bucket) in history::RANGES {
        assert_eq!(history::span(NOW - span, NOW, NOW).unwrap().2, *bucket);
    }
    assert_eq!(
        history::span(NOW - 600, NOW, NOW),
        Some((NOW - 600, NOW, 60))
    );
    // Whatever the alignment, a clamped span gives at most 360 points.
    let d = scratch("span");
    let h = History::open(&d.0.join("history.db"), NOW).unwrap();
    fill(&h, &[1], 30 * 1440);
    for skew in [0, 59, 61, 1234, 7199] {
        let now = NOW - skew;
        for (f, t) in [(i64::MIN, i64::MAX), (now - 86_400 - 17, now), (0, now)] {
            let (from, to, bucket) = history::span(f, t, now).unwrap();
            let v = h.query(&["cpu.total"], from, to, bucket).unwrap();
            let n = v["series"]["cpu.total"]["points"].as_array().unwrap().len();
            assert!(n as i64 <= history::MAX_POINTS, "{n} at skew {skew}");
        }
    }
    // Wholly outside the kept window: nothing.
    assert_eq!(history::span(0, NOW - history::KEEP_SECS, NOW), None);
    assert_eq!(history::span(NOW + 120, NOW + 600, NOW), None);
}

#[test]
fn the_minute_just_written_is_in_a_range_that_ends_now() {
    let d = scratch("newest");
    let mut h = History::open(&d.0.join("history.db"), NOW).unwrap();
    // The writer wakes just after a boundary and labels the values with the
    // minute that just ended (wake - 30 s); the daemon queries to now + 1.
    let boundary = NOW + 17 * 60 + 7_200 * 3 + 120;
    h.write_minute(boundary + 1 - 30, &[("cpu.total", 4200)])
        .unwrap();
    for into in [1, 2, 30, 58, 59] {
        let now = boundary + into;
        let v = h.query(&["cpu.total"], now - 3600, now + 1, 60).unwrap();
        assert_eq!(
            v["series"]["cpu.total"]["points"],
            serde_json::json!([[boundary - 60, 42.0, 42.0]]),
            "{into} s into the minute"
        );
    }
    // A wake a little early still labels the same minute.
    h.write_minute(boundary - 1 - 30, &[("cpu.total", 4300)])
        .unwrap();
    let v = h
        .query(&["cpu.total"], boundary - 3600, boundary + 1, 60)
        .unwrap();
    assert_eq!(
        v["series"]["cpu.total"]["points"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn compaction_is_due_a_day_after_the_last_one_even_across_restarts() {
    let d = scratch("due");
    let path = d.0.join("history.db");
    {
        let h = History::open(&path, NOW).unwrap();
        assert!(
            !h.compaction_due(NOW).unwrap(),
            "a new file starts the clock"
        );
        assert!(!h.compaction_due(NOW + 3600).unwrap());
    }
    // Restarted the next day: the clock survived.
    let h = History::open(&path, NOW).unwrap();
    assert!(h.compaction_due(NOW + 86_400).unwrap());
    h.compact(NOW + 86_400).unwrap();
    assert!(!h.compaction_due(NOW + 86_401).unwrap());
    // The clock was set back after that compaction: due, not held off.
    assert!(h.compaction_due(NOW).unwrap());
}

#[test]
fn compaction_leaves_no_large_log() {
    let d = scratch("wal");
    let h = History::open(&d.0.join("history.db"), NOW).unwrap();
    fill(&h, &[1, 2, 3, 4], 7 * 1440);
    h.compact(NOW).unwrap();
    let wal = std::fs::metadata(d.0.join("history.db-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(wal <= 1_048_576, "the log is {wal} bytes after compaction");
}

#[test]
fn writes_and_prunes() {
    let d = scratch("prune");
    let mut h = History::open(&d.0.join("history.db"), NOW).unwrap();
    assert_eq!(
        h.write_minute(NOW - 31 * 86_400, &[("cpu.total", 1), ("bogus", 2)])
            .unwrap(),
        1
    );
    h.write_minute(NOW - 60, &[("cpu.total", 5000)]).unwrap();
    h.add_event(
        NOW - 31 * 86_400,
        "spike",
        &serde_json::json!({"old": true}),
    )
    .unwrap();
    h.add_event(NOW - 60, "spike", &serde_json::json!({"cpu": 99}))
        .unwrap();
    assert_eq!(h.prune(NOW).unwrap(), 2);
    let v = h.query(&["cpu.total"], NOW - 40 * 86_400, NOW, 60).unwrap();
    assert_eq!(
        v["series"]["cpu.total"]["points"],
        serde_json::json!([[NOW - 60, 50.0, 50.0]])
    );
    let ev = h.events(Some("spike"), 0, NOW).unwrap();
    assert_eq!(
        ev,
        serde_json::json!([{"ts": NOW - 60, "kind": "spike", "data": {"cpu": 99}}])
    );
}

#[test]
fn a_broken_file_is_moved_aside() {
    let d = scratch("broken");
    let path = d.0.join("history.db");
    std::fs::write(
        &path,
        b"this is not a database, it is a sentence long enough to be read",
    )
    .unwrap();
    let h = History::open(&path, NOW).unwrap();
    let aside = h.moved_aside.clone().expect("moved aside");
    assert!(aside.ends_with(format!("history.db.broken-{NOW}")));
    assert!(aside.exists());
    assert_eq!(h.list().as_array().unwrap().len(), history::SERIES.len());
}

fn mode(p: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// A field of `proc/self/status` (no syscall needed).
fn status_field(name: &str) -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with(name)).unwrap();
    line[name.len()..].trim().to_string()
}

/// The process's umask.
fn umask() -> u32 {
    u32::from_str_radix(&status_field("Umask:"), 8).unwrap()
}

/// The process's real uid.
fn uid() -> u32 {
    status_field("Uid:")
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn the_file_and_its_directory_are_private() {
    // Plan 001, deferred to the first plan that writes events: the rows say
    // which apps the owner ran and stopped.
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("modes");
    let state = d.0.join("state");
    let path = state.join("mirqab/history.db");
    let h = History::open(&path, NOW).unwrap();
    assert_eq!(mode(&state.join("mirqab")), 0o700);
    for f in ["history.db", "history.db-wal", "history.db-shm"] {
        assert_eq!(mode(&state.join("mirqab").join(f)), 0o600, "{f}");
    }
    // The parent it had to make keeps the default mode (review of PR #6).
    assert_eq!(mode(&state), 0o777 & !umask());
    drop(h);

    // Made before modes were set: the files are tightened at open. The
    // directory is not the default one, so it may be the user's, even named
    // `mirqab` (a checkout beside a socket chosen with `daemon --root`):
    // left alone (review of PR #26).
    let set = |p: &std::path::Path, m: u32| {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap()
    };
    set(&state.join("mirqab"), 0o755);
    set(&path, 0o644);
    let _h = History::open(&path, NOW).unwrap();
    assert_eq!(mode(&state.join("mirqab")), 0o755);
    assert_eq!(mode(&path), 0o600);

    // The default directory, or one the caller owns, is tightened when it
    // is this user's directory; a symlink to one is not followed.
    history::private_dir(&state.join("mirqab"), true).unwrap();
    assert_eq!(mode(&state.join("mirqab")), 0o700);
    set(&state.join("mirqab"), 0o755);
    let link = d.0.join("link");
    std::os::unix::fs::symlink(state.join("mirqab"), &link).unwrap();
    history::private_dir(&link, true).unwrap();
    assert_eq!(mode(&state.join("mirqab")), 0o755);
    // Someone else's directory is never touched, owned or not (skipped as
    // root, where `/` would be ours).
    if uid() != 0 {
        let before = mode(std::path::Path::new("/"));
        history::private_dir(std::path::Path::new("/"), true).unwrap();
        assert_eq!(mode(std::path::Path::new("/")), before);
    }

    // A directory given by path: only the files are made private in it.
    let own = d.0.join("elsewhere");
    std::fs::create_dir_all(&own).unwrap();
    set(&own, 0o755);
    let _h = History::open(&own.join("h.db"), NOW).unwrap();
    assert_eq!(mode(&own), 0o755);
    assert_eq!(mode(&own.join("h.db")), 0o600);

    // A broken file moved aside is private, as is the new one.
    let broken = d.0.join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    let bad = broken.join("history.db");
    std::fs::write(
        &bad,
        b"this is not a database, it is a sentence long enough",
    )
    .unwrap();
    set(&bad, 0o644);
    let h = History::open(&bad, NOW).unwrap();
    assert_eq!(mode(h.moved_aside.as_ref().unwrap()), 0o600);
    assert_eq!(mode(&bad), 0o600);
}

/// Overwrites the root page of `sample` with garbage: the schema, `meta`
/// and `series` still read, so the file opens, and the first use of
/// `sample` finds it corrupt.
fn damage_sample(path: &std::path::Path) {
    use std::io::{Seek, SeekFrom, Write};
    let (root, page): (i64, i64) = {
        let c = rusqlite::Connection::open(path).unwrap();
        let root = c
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE name = 'sample'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        (
            root,
            c.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap(),
        )
    };
    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(((root - 1) * page) as u64)).unwrap();
    f.write_all(&vec![0xa5; page as usize]).unwrap();
}

#[test]
fn a_file_found_corrupt_while_open_is_moved_aside_and_replaced() {
    // Audit: `open` reads only the schema, `meta` and `series`, so damage
    // in `sample` opened without complaint and every write failed after.
    let d = scratch("damaged");
    let path = d.0.join("history.db");
    {
        let h = History::open(&path, NOW).unwrap();
        fill(&h, &[1, 2], 1440);
        h.compact(NOW).unwrap();
    }
    damage_sample(&path);
    let mut h = History::open(&path, NOW).expect("the damage is past what open reads");
    assert!(h.moved_aside.is_none());
    let err = h.write_minute(NOW, &[("cpu.total", 1)]).unwrap_err();
    assert!(err.1, "corrupt: {}", err.0);
    h.recover(NOW + 60).unwrap();
    let aside = h.moved_aside.clone().expect("moved aside");
    assert!(aside.ends_with(format!("history.db.broken-{}", NOW + 60)));
    assert!(aside.exists());
    assert!(h.broken.is_none());
    h.write_minute(NOW + 60, &[("cpu.total", 1)]).unwrap();
    let v = h.query(&["cpu.total"], NOW, NOW + 120, 60).unwrap();
    assert_eq!(
        v["series"]["cpu.total"]["points"].as_array().unwrap().len(),
        1
    );
    assert_eq!(h.list().as_array().unwrap().len(), history::SERIES.len());

    // A file that cannot be moved aside leaves history off with the reason,
    // and the next call, once it can, finishes the job.
    use std::os::unix::fs::PermissionsExt;
    let ro = d.0.join("ro");
    std::fs::create_dir_all(&ro).unwrap();
    let path = ro.join("history.db");
    History::open(&path, NOW).unwrap();
    damage_sample(&path);
    let mut h = History::open(&path, NOW).unwrap();
    assert!(h.write_minute(NOW, &[("cpu.total", 1)]).unwrap_err().1);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
    let refused = h.recover(NOW + 60);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
    if refused.is_ok() {
        // Running as root: the directory's mode is no obstacle.
        return;
    }
    assert!(h.broken.is_some());
    h.recover(NOW + 120).unwrap();
    assert!(h.broken.is_none());
    assert!(h.moved_aside.as_ref().unwrap().exists());
    h.write_minute(NOW + 120, &[("cpu.total", 1)]).unwrap();
}

/// The `.broken-*` files beside `path`.
fn broken_files(path: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".broken-"))
        .collect();
    names.sort();
    names
}

#[test]
fn damage_found_again_soon_after_a_replacement_leaves_history_off() {
    // Review of PR #26: failing storage added a `.broken-*` file a minute.
    let d = scratch("again");
    let path = d.0.join("history.db");
    History::open(&path, NOW).unwrap();
    damage_sample(&path);
    let mut h = History::open(&path, NOW).unwrap();
    assert!(h.write_minute(NOW, &[("cpu.total", 1)]).unwrap_err().1);
    h.recover(NOW).unwrap();
    assert_eq!(broken_files(&path).len(), 1);
    // The new file is damaged too: written out, then the page overwritten,
    // and the connection's cache dropped so it reads the page again.
    h.write_minute(NOW, &[("cpu.total", 1)]).unwrap();
    h.checkpoint().unwrap();
    damage_sample(&path);
    h.conn().execute_batch("PRAGMA shrink_memory").unwrap();
    assert!(h.write_minute(NOW + 60, &[("cpu.total", 1)]).unwrap_err().1);
    let err = h.recover(NOW + 60).unwrap_err();
    assert!(err.0.contains("again"), "{}", err.0);
    assert_eq!(h.broken.as_deref(), Some(err.0.as_str()));
    assert!(h.recover(NOW + 3600).is_err(), "still within the backoff");
    assert_eq!(broken_files(&path).len(), 1, "nothing more moved aside");
    // Once the backoff has passed, the next try replaces it.
    h.recover(NOW + history::RECOVER_BACKOFF_SECS).unwrap();
    assert!(h.broken.is_none());
    assert_eq!(broken_files(&path).len(), 2);
    h.write_minute(NOW + history::RECOVER_BACKOFF_SECS, &[("cpu.total", 1)])
        .unwrap();
}

#[test]
fn a_file_corrupt_at_open_that_cannot_be_moved_is_retried() {
    // Plan 024, C13: `open` returned Err and nothing opened history again
    // until a restart; now it is off with the reason and `recover` retries.
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("openmove");
    let ro = d.0.join("ro");
    std::fs::create_dir_all(&ro).unwrap();
    let path = ro.join("history.db");
    std::fs::write(
        &path,
        b"this is not a database, it is a sentence long enough to be read",
    )
    .unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut h = match History::open(&path, NOW) {
        Ok(h) => h,
        Err(e) => {
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
            panic!("open failed: {e}");
        }
    };
    let blocked = h.broken.clone();
    let again = h.recover(NOW + 60);
    // Writable again before any assertion, so the scratch tree goes.
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).unwrap();
    let Some(why) = blocked else {
        // Running as root: the directory's mode is no obstacle.
        return;
    };
    assert!(why.contains("moving it aside failed"), "{why}");
    assert!(h.moved_aside.is_none());
    assert!(again.is_err(), "still read-only");
    assert!(h.write_minute(NOW, &[("cpu.total", 1)]).is_err());
    // Writable again: the next minute's try moves it and opens a new one.
    h.recover(NOW + 120).unwrap();
    assert!(h.broken.is_none());
    let aside = h.moved_aside.clone().expect("moved aside");
    assert!(aside.ends_with(format!("history.db.broken-{}", NOW + 120)));
    assert!(aside.exists());
    h.write_minute(NOW + 120, &[("cpu.total", 1)]).unwrap();
    assert_eq!(h.list().as_array().unwrap().len(), history::SERIES.len());
}

#[test]
fn a_corrupt_file_deleted_by_hand_is_replaced_at_once() {
    // Plan 024, L2: once the damaged file was gone, its rename failed with
    // ENOENT every minute and history stayed off until a restart.
    let d = scratch("deleted");
    let path = d.0.join("history.db");
    History::open(&path, NOW).unwrap();
    damage_sample(&path);
    let mut h = History::open(&path, NOW).unwrap();
    assert!(h.write_minute(NOW, &[("cpu.total", 1)]).unwrap_err().1);
    h.recover(NOW).unwrap();
    h.write_minute(NOW, &[("cpu.total", 1)]).unwrap();
    h.checkpoint().unwrap();
    damage_sample(&path);
    h.conn().execute_batch("PRAGMA shrink_memory").unwrap();
    assert!(h.write_minute(NOW + 60, &[("cpu.total", 1)]).unwrap_err().1);
    assert!(h.recover(NOW + 60).unwrap_err().0.contains("again"));
    assert_eq!(broken_files(&path).len(), 1);
    // The user starts over within the backoff. A log left behind would be
    // replayed into the new file: it goes aside too.
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(d.0.join("history.db-shm"));
    std::fs::write(d.0.join("history.db-wal"), b"an old log").unwrap();
    h.recover(NOW + 120).unwrap();
    assert!(h.broken.is_none());
    assert_ne!(
        std::fs::read(d.0.join("history.db-wal")).unwrap_or_default(),
        b"an old log"
    );
    assert_eq!(
        std::fs::read(d.0.join(format!("history.db.broken-{}-wal", NOW + 120))).unwrap(),
        b"an old log"
    );
    // Nothing was moved as a file: `movedAside` names the earlier one.
    let named = h.moved_aside.clone().unwrap();
    assert!(named.exists(), "{named:?}");
    assert!(
        !d.0.join(format!("history.db.broken-{}", NOW + 120))
            .exists()
    );
    h.write_minute(NOW + 120, &[("cpu.total", 1)]).unwrap();
    let v = h.query(&["cpu.total"], NOW, NOW + 180, 60).unwrap();
    assert_eq!(
        v["series"]["cpu.total"]["points"].as_array().unwrap().len(),
        1,
        "only the new minute: {v}"
    );

    // Past the backoff, with no log left, the same.
    h.checkpoint().unwrap();
    damage_sample(&path);
    h.conn().execute_batch("PRAGMA shrink_memory").unwrap();
    assert!(
        h.write_minute(NOW + 180, &[("cpu.total", 1)])
            .unwrap_err()
            .1
    );
    let late = NOW + history::RECOVER_BACKOFF_SECS + 60;
    let before = broken_files(&path).len();
    for side in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(d.0.join(format!("history.db{side}")));
    }
    h.recover(late).unwrap();
    assert!(h.broken.is_none());
    assert_eq!(broken_files(&path).len(), before, "nothing to move aside");
    h.write_minute(late, &[("cpu.total", 1)]).unwrap();
}

#[test]
fn a_file_moved_aside_never_overwrites_an_earlier_one() {
    // Review of PR #26: a second move in the same second overwrote the
    // first, and a log left behind could be replayed into the new file.
    let d = scratch("aside");
    let path = d.0.join("history.db");
    let taken = d.0.join(format!("history.db.broken-{NOW}"));
    std::fs::write(&taken, b"the first one").unwrap();
    std::fs::write(
        d.0.join(format!("history.db.broken-{NOW}.1-wal")),
        b"its log",
    )
    .unwrap();
    std::fs::write(
        &path,
        b"this is not a database, it is a sentence long enough",
    )
    .unwrap();
    std::fs::write(d.0.join("history.db-wal"), b"an old log").unwrap();
    let h = History::open(&path, NOW).unwrap();
    let aside = h.moved_aside.clone().unwrap();
    assert!(
        aside.ends_with(format!("history.db.broken-{NOW}.2")),
        "{aside:?}"
    );
    assert_eq!(std::fs::read(&taken).unwrap(), b"the first one");
    assert_eq!(
        std::fs::read(d.0.join(format!("history.db.broken-{NOW}.1-wal"))).unwrap(),
        b"its log"
    );
    // SQLite may drop a log it cannot read while it tries the file; one
    // that is left goes with its file, never to the new one.
    if let Ok(log) = std::fs::read(d.0.join(format!("history.db.broken-{NOW}.2-wal"))) {
        assert_eq!(log, b"an old log");
    }
    assert_ne!(
        std::fs::read(d.0.join("history.db-wal")).unwrap_or_default(),
        b"an old log"
    );
}

/// Set in the child process `the_default_directory_is_tightened` starts.
const DEFAULT_DIR_CHILD: &str = "MIRQAB_TEST_DEFAULT_DIR_CHILD";

#[test]
fn the_default_directory_is_tightened() {
    // Review of PR #26: `private_dir_of`'s own branch, for the real default
    // path. `XDG_STATE_HOME` is the process's, so the check runs in a child
    // of this test binary that runs this test alone with it set: no other
    // test sees the variable, and none is set with `unsafe`.
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os(DEFAULT_DIR_CHILD).is_some() {
        let path = History::default_path().unwrap();
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _h = History::open(&path, NOW).unwrap();
        assert_eq!(mode(dir), 0o700);
        return;
    }
    let d = scratch("default");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "the_default_directory_is_tightened",
            "--exact",
            "--nocapture",
        ])
        .env(DEFAULT_DIR_CHILD, "1")
        .env("XDG_STATE_HOME", &d.0)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1 passed"),
        "the child ran the test"
    );
    assert_eq!(mode(&d.0.join("mirqab")), 0o700);
}

#[test]
fn a_newer_schema_is_left_alone_and_refused() {
    let d = scratch("newer");
    let path = d.0.join("history.db");
    {
        let mut h = History::open(&path, NOW).unwrap();
        h.write_minute(NOW, &[("cpu.total", 1)]).unwrap();
        h.conn()
            .execute("UPDATE meta SET value = '2' WHERE key = 'schema'", [])
            .unwrap();
    }
    let err = History::open(&path, NOW).err().expect("refused");
    assert!(err.0.contains("newer"), "{}", err.0);
    let names: Vec<String> = std::fs::read_dir(&d.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains("broken")), "{names:?}");
}

/// A `top-apps` row as the `apps` sampler writes it: three names by CPU
/// and three by memory, the process count (about 150 bytes).
fn top_apps(m: i64) -> serde_json::Value {
    serde_json::json!({
        "cpu": [["Example Browser", 41.2 + (m % 7) as f64], ["cargo", 30.0], ["System", 4.1]],
        "mem": [["Example Browser", 2_147_483_648_i64 + m], ["code", 1_073_741_824_i64], ["System", 536_870_912_i64]],
        "procs": 412 + m % 9,
    })
}

#[test]
fn sixty_four_series_for_thirty_days_fit_in_forty_megabytes() {
    let d = scratch("size");
    let mut h = History::open(&d.0.join("history.db"), NOW).unwrap();
    let mut ids = Vec::new();
    for i in 0..history::MAX_SERIES {
        ids.push(
            h.register(&format!("test.series{i}"), "bytes", 1)
                .or_else(|_| h.register(history::SERIES[i % history::SERIES.len()].0, "", 1))
                .unwrap(),
        );
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), history::MAX_SERIES);
    assert!(h.register("one.too.many", "bytes", 1).is_err());
    // The steady state: 29 days packed by the daily rewrite, then a day of
    // minutes written since. Every minute also has a `top-apps` row at its
    // real size, the worst case (a machine busy with changing leaders all
    // the time; plan 006, part 4, decision 7); they are kept three days
    // (`KEEP_TOP_APPS_SECS`), so two days of them are packed too.
    fill_range(&h, &ids, 29 * 1440, 1440);
    for m in 0..29 * 1440 {
        h.add_event(NOW - (29 * 1440 + 1440 - m) * 60, "top-apps", &top_apps(m))
            .unwrap();
    }
    h.prune(NOW - 1440 * 60).unwrap();
    h.compact(NOW).unwrap();
    // The last day as the writer does it: one transaction a minute, so the
    // write-ahead log behaves as it does in a running daemon.
    for m in 0..1440 {
        fill_range(&h, &ids, 1, 1439 - m);
        h.add_event(NOW - (1440 - m) * 60, "top-apps", &top_apps(m))
            .unwrap();
    }
    // The file and its write-ahead log as they stand, no manual checkpoint:
    // what `status` reports and what the disk holds. The budget is 40 MiB
    // (owner, 2026-11-13).
    let size = h.size_bytes();
    eprintln!("history: 64 series and a top-apps row a minute x 30 days = {size} bytes");
    assert!(size < 40 * 1024 * 1024, "{size} bytes");
}

#[test]
fn top_apps_rows_are_kept_three_days_and_other_events_thirty() {
    let d = scratch("topkeep");
    let mut h = History::open(&d.0.join("history.db"), NOW).unwrap();
    for (ago_days, kind) in [
        (2, "top-apps"),
        (4, "top-apps"),
        (4, "action"),
        (31, "action"),
    ] {
        h.add_event(NOW - ago_days * 86_400, kind, &serde_json::json!({}))
            .unwrap();
    }
    assert_eq!(h.prune(NOW).unwrap(), 2);
    let left = h.events(None, NOW - 40 * 86_400, NOW + 1).unwrap();
    let kinds: Vec<(i64, &str)> = left
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                (NOW - e["ts"].as_i64().unwrap()) / 86_400,
                e["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(kinds, [(4, "action"), (2, "top-apps")]);
}

#[test]
fn a_series_registered_later_survives_a_reopen() {
    let d = scratch("reopen");
    let path = d.0.join("history.db");
    {
        let mut h = History::open(&path, NOW).unwrap();
        h.register("net.rx", "bytes", 1).unwrap();
        h.write_minute(NOW - 30, &[("net.rx", 42)]).unwrap();
    }
    // Reopened, before anything registers net.rx again.
    let h = History::open(&path, NOW).unwrap();
    assert!(
        h.list()
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["key"] == "net.rx"),
        "{}",
        h.list()
    );
    let q = h.query(&["net.rx"], NOW - 3600, NOW + 1, 60).unwrap();
    assert_eq!(q["series"]["net.rx"]["points"][0][1], 42.0);
}
