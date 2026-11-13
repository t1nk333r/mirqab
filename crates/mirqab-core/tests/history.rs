//! History (plan 001 phase C, architecture §6): the schema, counter
//! differences between two fixture moments, gaps, the ranges' bucket counts,
//! pruning, a broken file moved aside, and 30 days × 64 series under 40 MB.

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
    // minutes written since.
    fill_range(&h, &ids, 29 * 1440, 1440);
    h.compact(NOW).unwrap();
    // The last day as the writer does it: one transaction a minute, so the
    // write-ahead log behaves as it does in a running daemon.
    for m in 0..1440 {
        fill_range(&h, &ids, 1, 1439 - m);
    }
    // The file and its write-ahead log as they stand, no manual checkpoint:
    // what `status` reports and what the disk holds. The budget is 40 MiB
    // (owner, 2026-11-13).
    let size = h.size_bytes();
    eprintln!("history: 64 series x 30 days = {size} bytes");
    assert!(size < 40 * 1024 * 1024, "{size} bytes");
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
