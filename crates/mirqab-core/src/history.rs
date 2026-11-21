//! Thirty days of per-minute history in one SQLite file (architecture §6).
//! Written by the daemon only; clients ask with `history.query` and
//! `history.events`.

use crate::proto::strings;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "1";
/// Minute samples and events are kept this long; daily totals 90 days.
pub const KEEP_SECS: i64 = 30 * 86_400;
pub const KEEP_DAILY_DAYS: i64 = 90;
/// `top-apps` rows are kept three days, not thirty (plan 006, part 4): a
/// row every minute for 30 days is 10.6 MB with its index, and 64 series
/// already take 38.4 of the 40 MiB budget; three days of the worst case
/// is about 1.1 MB.
pub const KEEP_TOP_APPS_SECS: i64 = 3 * 86_400;
/// The most series history holds (§6.1).
pub const MAX_SERIES: usize = 64;

/// Every series history keeps: key, unit, scale. Plan 001 fixed the first
/// four; plan 002 adds the System tab's (architecture §6.1). Page counts are
/// per minute (`page.*` in KiB, as vmstat counts them; `swap.*` in pages).
pub const SERIES: &[(&str, &str, i64)] = &[
    ("cpu.total", "percent", 100),
    ("mem.used", "bytes", 1),
    ("mem.available", "bytes", 1),
    ("swap.used", "bytes", 1),
    ("cpu.user", "percent", 100),
    ("cpu.system", "percent", 100),
    ("cpu.iowait", "percent", 100),
    ("load.one", "load", 100),
    ("mem.cache", "bytes", 1),
    ("psi.cpu", "percent", 100),
    ("psi.memory", "percent", 100),
    ("psi.io", "percent", 100),
    ("page.in", "kib", 1),
    ("page.out", "kib", 1),
    ("swap.in", "pages", 1),
    ("swap.out", "pages", 1),
    ("temp.cpu", "celsius", 10),
    ("temp.gpu", "celsius", 10),
    ("gpu.0.busy", "percent", 100),
    ("gpu.1.busy", "percent", 100),
    ("gpu.0.vram", "bytes", 1),
    ("gpu.1.vram", "bytes", 1),
    ("power.package", "watts", 10),
    ("power.battery", "watts", 10),
    ("cpu.mhz", "mhz", 1),
];

/// A range of §6.4 that the database answers: (name, span, bucket), seconds.
pub const RANGES: &[(&str, i64, i64)] = &[
    ("1h", 3_600, 60),
    ("24h", 86_400, 240),
    ("7d", 7 * 86_400, 1_800),
    ("30d", 30 * 86_400, 7_200),
];

/// Stored minutes count from 2026-01-01 00:00 UTC, not from 1970: a
/// minute then fits SQLite's 3-byte integer for 15 years instead of taking 4
/// bytes, a byte on each of 2.76 M rows (plan 001 phase C, "as built").
/// The epoch is on a 2-hour boundary, so range buckets align as before.
pub const EPOCH: i64 = 1_767_225_600;

/// The stored minute for a unix time. Saturating: a time a client gives
/// cannot overflow it (plan 001: `i64::MIN` panicked a debug daemon).
pub fn minute(ts: i64) -> i64 {
    ts.saturating_sub(EPOCH).div_euclid(60)
}

/// The unix time a stored minute starts at.
pub fn unix(minute: i64) -> i64 {
    minute * 60 + EPOCH
}

pub fn range(name: &str) -> Option<(i64, i64)> {
    RANGES.iter().find(|r| r.0 == name).map(|r| (r.1, r.2))
}

/// The most points a `from`/`to` query gives a series, as the ranges do
/// (§6.4: 360 at most).
pub const MAX_POINTS: i64 = 360;

/// A `from`/`to` query's span at `now`: clamped to what is kept, from 30
/// days back to a minute ahead, with the smallest range bucket that keeps
/// it within MAX_POINTS (2 hours past 30 days). None when nothing of it is
/// kept. Unclamped, one query over years at 1-minute buckets built
/// hundreds of MB of points (plan 001, deferred to plan 005).
pub fn span(from: i64, to: i64, now: i64) -> Option<(i64, i64, i64)> {
    let from = from.max(now - KEEP_SECS);
    let to = to.min(now + 60);
    if from >= to {
        return None;
    }
    let width = to - from;
    let bucket = RANGES
        .iter()
        .map(|r| r.2)
        .find(|b| (width + b - 1) / b <= MAX_POINTS)
        .unwrap_or(RANGES[RANGES.len() - 1].2);
    Some((from, to, bucket))
}

const DDL: &str = "
CREATE TABLE IF NOT EXISTS series (id INTEGER PRIMARY KEY, key TEXT UNIQUE NOT NULL, unit TEXT NOT NULL, scale INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sample (series INTEGER NOT NULL, ts INTEGER NOT NULL, v INTEGER NOT NULL,
                                   PRIMARY KEY (series, ts)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS daily  (key TEXT NOT NULL, day INTEGER NOT NULL, v INTEGER NOT NULL,
                                   PRIMARY KEY (key, day)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS event  (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS event_kind_ts ON event (kind, ts);
CREATE TABLE IF NOT EXISTS meta   (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

pub struct History {
    conn: Connection,
    path: PathBuf,
    ids: BTreeMap<String, (i64, i64)>,
    /// Set when the file was unreadable and was moved aside, at open or
    /// by `recover`.
    pub moved_aside: Option<PathBuf>,
    /// Why history is off: the file was found corrupt while open and could
    /// not be replaced. `recover` tries again.
    pub broken: Option<String>,
    /// The file at `path` was found corrupt and is not moved aside yet.
    corrupt_in_place: bool,
}

#[derive(Debug)]
pub struct HistoryError(
    pub String,
    /** SQLite says the file is not a database or is corrupt. */ pub bool,
);

impl HistoryError {
    pub fn msg(m: impl Into<String>) -> Self {
        HistoryError(m.into(), false)
    }
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for HistoryError {
    fn from(e: rusqlite::Error) -> Self {
        let corrupt = matches!(
            e.sqlite_error_code(),
            Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
        );
        HistoryError(e.to_string(), corrupt)
    }
}

/// The directory `default_path` keeps history in, which mirqab owns.
const OWN_DIR: &str = "mirqab";

/// Makes the directory history lives in private (plan 001, deferred to the
/// first plan that writes events; the `top-apps` and `action` rows say what
/// the owner ran and stopped). A missing one is made 0700, and parents it
/// has to make (a fresh `~/.local/state`) keep the default mode (review of
/// PR #6). An existing one is tightened to 0700 only when it is mirqab's own
/// (`mirqab`, as `default_path` names it): a directory of another name was
/// given by path and may be the user's. The files are private either way.
pub fn private_dir(dir: &Path) -> Result<(), HistoryError> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    let failed = |e: std::io::Error| HistoryError::msg(format!("{}: {e}", dir.display()));
    if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| HistoryError::msg(format!("{}: {e}", parent.display())))?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            if dir.file_name().is_some_and(|n| n == OWN_DIR) {
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(failed)?;
            }
            Ok(())
        }
        Err(e) => Err(failed(e)),
    }
}

/// Makes `path` 0600: created so when missing, before SQLite opens it (its
/// log and shared memory then take the same mode), and tightened when it
/// exists (a file made before modes were set).
fn private_file(path: &Path) -> Result<(), HistoryError> {
    let failed = |e: std::io::Error| HistoryError::msg(format!("{}: {e}", path.display()));
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {
            if meta.permissions().mode() & 0o777 != 0o600 {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                    .map_err(failed)?;
            }
            Ok(())
        }
        // Not a regular file: SQLite says what it is.
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map(|_| ())
            .map_err(failed),
        Err(e) => Err(failed(e)),
    }
}

/// `open_conn` on a file made private first, its `-wal` and `-shm` after.
fn open_private(path: &Path) -> Result<Connection, HistoryError> {
    private_file(path)?;
    let conn = open_conn(path)?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", path.display()));
        if side.exists() {
            private_file(&side)?;
        }
    }
    Ok(conn)
}

fn open_conn(path: &Path) -> Result<Connection, HistoryError> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Keep the write-ahead log small: checkpoint every 256 pages (1 MiB)
    // instead of 1000, and cut the file back to 1 MiB after a checkpoint, so
    // the log adds at most about a megabyte to the file's 30-day size.
    conn.pragma_update(None, "wal_autocheckpoint", 256)?;
    conn.pragma_update(None, "journal_size_limit", 1_048_576)?;
    conn.execute_batch(DDL)?;
    let schema: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
            r.get(0)
        })
        .optional()?;
    match schema.as_deref() {
        None => {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema', ?1)",
                [SCHEMA],
            )?;
        }
        Some(SCHEMA) => {}
        // A later schema written by a newer mirqab: do not guess at it.
        Some(other) => {
            return Err(HistoryError::msg(strings::history_schema_newer(
                other, SCHEMA,
            )));
        }
    }
    Ok(conn)
}

/// Renames a file SQLite called corrupt, with its log and shared memory,
/// to `<name>.broken-<now>`, and returns the new name.
fn move_aside(path: &Path, now_unix: i64) -> std::io::Result<PathBuf> {
    let aside = path.with_file_name(format!(
        "{}.broken-{now_unix}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    std::fs::rename(path, &aside)?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", path.display()));
        if side.exists() {
            let _ = std::fs::rename(&side, PathBuf::from(format!("{}{suffix}", aside.display())));
        }
    }
    Ok(aside)
}

impl History {
    /// `$XDG_STATE_HOME/mirqab/history.db`, else `~/.local/state/mirqab/history.db`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from(".local/state"));
        base.join("mirqab/history.db")
    }

    /// Opens or creates the file. One that cannot be opened is renamed
    /// `history.db.broken-<unix time>` and a new one is made; `moved_aside`
    /// says so for `status`.
    pub fn open(path: &Path, now_unix: i64) -> Result<History, HistoryError> {
        if let Some(dir) = path.parent() {
            private_dir(dir)?;
        }
        let (conn, moved_aside) = match open_private(path) {
            Ok(c) => (c, None),
            // Only a file SQLite calls not a database, or corrupt, is moved
            // aside. A newer schema, a permission error, a full disk or a
            // lock leaves the file alone and history off (review of phase C:
            // each of those hid thirty days of data).
            Err(first) if first.1 => {
                let aside = move_aside(path, now_unix).map_err(|e| {
                    HistoryError::msg(format!("{first}; and moving it aside failed: {e}"))
                })?;
                (open_private(path)?, Some(aside))
            }
            Err(other) => return Err(other),
        };
        let mut h = History {
            conn,
            path: path.to_path_buf(),
            ids: BTreeMap::new(),
            moved_aside,
            broken: None,
            corrupt_in_place: false,
        };
        // Every series the file already has, so a series a later plan
        // registers (net.rx) is queryable after a restart before it is
        // registered again (review of plan 001 phase C).
        let stored: Vec<(String, i64, i64)> = h
            .conn
            .prepare("SELECT key, id, scale FROM series")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (key, id, scale) in stored {
            h.ids.insert(key, (id, scale));
        }
        for (key, unit, scale) in SERIES {
            h.register(key, unit, *scale)?;
        }
        Ok(h)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replaces a file SQLite found corrupt while it was open (a write or a
    /// query said so: `open` reads only the schema, `meta` and `series`, so
    /// a damaged `sample` page opens without complaint). The connection is
    /// closed, the file moved aside as at open (`moved_aside` says where)
    /// and a new one made. On failure `broken` says why, and the next call
    /// tries again from where this one stopped (audit: history stayed on
    /// while every write and query failed).
    pub fn recover(&mut self, now_unix: i64) -> Result<(), HistoryError> {
        if self.broken.is_none() {
            self.corrupt_in_place = true;
        }
        let result = self.replace(now_unix);
        if let Err(e) = &result {
            self.broken = Some(e.to_string());
        }
        result
    }

    fn replace(&mut self, now_unix: i64) -> Result<(), HistoryError> {
        // Closed first, its log and shared memory with it, so nothing writes
        // to the file once it is moved.
        drop(std::mem::replace(
            &mut self.conn,
            Connection::open_in_memory()?,
        ));
        if self.corrupt_in_place {
            let aside = move_aside(&self.path, now_unix)
                .map_err(|e| HistoryError::msg(format!("{}: {e}", self.path.display())))?;
            self.corrupt_in_place = false;
            self.moved_aside = Some(aside);
        }
        let fresh = History::open(&self.path, now_unix)?;
        *self = History {
            moved_aside: fresh.moved_aside.clone().or(self.moved_aside.take()),
            ..fresh
        };
        Ok(())
    }

    /// Adds a series, or confirms it. Fails past MAX_SERIES.
    pub fn register(&mut self, key: &str, unit: &str, scale: i64) -> Result<i64, HistoryError> {
        if let Some((id, _)) = self.ids.get(key) {
            return Ok(*id);
        }
        let existing: Option<(i64, i64)> = self
            .conn
            .query_row("SELECT id, scale FROM series WHERE key = ?1", [key], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let (id, scale) = match existing {
            Some(found) => found,
            None => {
                let count: i64 = self
                    .conn
                    .query_row("SELECT count(*) FROM series", [], |r| r.get(0))?;
                if count as usize >= MAX_SERIES {
                    return Err(HistoryError::msg(strings::history_series_full(MAX_SERIES)));
                }
                self.conn.execute(
                    "INSERT INTO series (key, unit, scale) VALUES (?1, ?2, ?3)",
                    params![key, unit, scale],
                )?;
                (self.conn.last_insert_rowid(), scale)
            }
        };
        self.ids.insert(key.to_string(), (id, scale));
        Ok(id)
    }

    /// One minute's values in one transaction. `ts` is any second inside the
    /// minute the values cover; the row is labelled with that minute. Unknown keys are skipped.
    pub fn write_minute(&mut self, ts: i64, values: &[(&str, i64)]) -> Result<usize, HistoryError> {
        let minute = minute(ts);
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO sample (series, ts, v) VALUES (?1, ?2, ?3)",
            )?;
            for (key, v) in values {
                if let Some((id, _)) = self.ids.get(*key) {
                    stmt.execute(params![id, minute, v])?;
                    n += 1;
                }
            }
        }
        tx.commit()?;
        Ok(n)
    }

    pub fn add_event(&mut self, ts: i64, kind: &str, data: &Value) -> Result<(), HistoryError> {
        self.conn.execute(
            "INSERT INTO event (ts, kind, data) VALUES (?1, ?2, ?3)",
            params![ts, kind, data.to_string()],
        )?;
        Ok(())
    }

    /// Avg and max per bucket for each series over `[from, to)`, unscaled.
    /// Buckets without data are absent: a graph draws a break there.
    pub fn query(
        &self,
        series: &[&str],
        from: i64,
        to: i64,
        bucket: i64,
    ) -> Result<Value, HistoryError> {
        let bucket_min = (bucket / 60).max(1);
        let mut out = serde_json::Map::new();
        let mut stmt = self.conn.prepare_cached(
            // Floor division, so a minute before the epoch (a clock set
            // wrong) lands in its own bucket, not the one after it.
            "SELECT ts - ((ts % ?2) + ?2) % ?2 AS b, avg(v), max(v) FROM sample
             WHERE series = ?1 AND ts >= ?3 AND ts < ?4 GROUP BY b ORDER BY b",
        )?;
        for key in series {
            let Some((id, scale)) = self.ids.get(*key).copied() else {
                out.insert(key.to_string(), Value::Null);
                continue;
            };
            let unit: String =
                self.conn
                    .query_row("SELECT unit FROM series WHERE id = ?1", [id], |r| r.get(0))?;
            let s = scale.max(1) as f64;
            let points: Vec<Value> = stmt
                // Rows are labelled with the minute their values cover, so
                // the minute running at `to` holds nothing yet.
                .query_map(params![id, bucket_min, minute(from), minute(to)], |r| {
                    let b: i64 = r.get(0)?;
                    let avg: f64 = r.get(1)?;
                    let max: i64 = r.get(2)?;
                    Ok(json!([unix(b), avg / s, max as f64 / s]))
                })?
                .collect::<Result<_, _>>()?;
            out.insert(key.to_string(), json!({"unit": unit, "points": points}));
        }
        Ok(json!({"from": from, "to": to, "bucketSec": bucket_min * 60, "series": out}))
    }

    pub fn events(&self, kind: Option<&str>, from: i64, to: i64) -> Result<Value, HistoryError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT ts, kind, data FROM event WHERE (?1 IS NULL OR kind = ?1) AND ts >= ?2 AND ts < ?3 ORDER BY ts",
        )?;
        let rows: Vec<Value> = stmt
            .query_map(params![kind, from, to], |r| {
                let data: String = r.get(2)?;
                Ok(
                    json!({"ts": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?,
                          "data": serde_json::from_str::<Value>(&data).unwrap_or(Value::Null)}),
                )
            })?
            .collect::<Result<_, _>>()?;
        Ok(Value::Array(rows))
    }

    /// Deletes what is past its keep time. Returns the rows removed.
    pub fn prune(&mut self, now: i64) -> Result<usize, HistoryError> {
        // One range delete per series: the key is (series, ts), so a bare
        // `ts < ?` scans the whole table (37 ms an hour on a full file;
        // review of plan 001 phase C).
        let cutoff = minute(now - KEEP_SECS);
        let mut n = 0;
        let ids: Vec<i64> = self.ids.values().map(|(id, _)| *id).collect();
        {
            let mut stmt = self
                .conn
                .prepare_cached("DELETE FROM sample WHERE series = ?1 AND ts < ?2")?;
            for id in ids {
                n += stmt.execute(rusqlite::params![id, cutoff])?;
            }
        }
        n += self
            .conn
            .execute("DELETE FROM event WHERE ts < ?1", [now - KEEP_SECS])?;
        // By the (kind, ts) index.
        n += self.conn.execute(
            "DELETE FROM event WHERE kind = 'top-apps' AND ts < ?1",
            [now - KEEP_TOP_APPS_SECS],
        )?;
        n += self.conn.execute(
            "DELETE FROM daily WHERE day < ?1",
            [now.div_euclid(86_400) - KEEP_DAILY_DAYS],
        )?;
        Ok(n)
    }

    /// The series' keys, units and scales.
    pub fn list(&self) -> Value {
        Value::Array(
            SERIES
                .iter()
                .filter(|(k, _, _)| self.ids.contains_key(*k))
                .map(|(k, u, s)| json!({"key": k, "unit": u, "scale": s}))
                .chain(
                    self.ids
                        .keys()
                        .filter(|k| !SERIES.iter().any(|s| s.0 == k.as_str()))
                        .map(|k| json!({"key": k})),
                )
                .collect(),
        )
    }

    /// The file's size with its write-ahead log.
    pub fn size_bytes(&self) -> u64 {
        ["", "-wal"]
            .iter()
            .filter_map(|s| std::fs::metadata(format!("{}{s}", self.path.display())).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Rewrites the file in key order. Minutes arrive series by series, so
    /// each series' run of rows grows in the middle of the table and its pages
    /// split half full; a daily rewrite packs them again (plan 001 phase C:
    /// 30 days × 64 series measured 43 MB unpacked, over the 40 MB budget).
    /// Whether a day has passed since the last compaction. A new file counts
    /// from its creation.
    pub fn compaction_due(&self, now: i64) -> Result<bool, HistoryError> {
        let last: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'compacted'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(match last.and_then(|v| v.parse::<i64>().ok()) {
            // A stamp from the future (the clock jumped ahead, then was set
            // right) would hold compaction off until the clock caught up.
            Some(t) => t > now || now - t >= 86_400,
            None => {
                self.conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('compacted', ?1)",
                    [now.to_string()],
                )?;
                false
            }
        })
    }

    pub fn compact(&self, now: i64) -> Result<(), HistoryError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('compacted', ?1)",
            [now.to_string()],
        )?;
        // In WAL mode VACUUM writes the whole new file through the log; empty
        // it at once, or the log stays as large as the database (review of
        // phase C: 36 MB of database left a 36 MB log).
        self.conn
            .execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Makes the write-ahead log's pages part of the file (for size tests).
    pub fn checkpoint(&self) -> Result<(), HistoryError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
