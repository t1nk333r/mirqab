//! Thirty days of per-minute history in one SQLite file (architecture §6).
//! Written by the daemon only; clients ask with `history.query` and
//! `history.events`.

use crate::proto::strings;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
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
/// it within MAX_POINTS (2 hours past 30 days). Buckets align to the
/// epoch, so a span is then trimmed at its oldest end to the MAX_POINTS
/// buckets that end it (a whole 30 days at 2 hours otherwise touches 361
/// or 362; review of PR #26). None when nothing of it is kept. Unclamped,
/// one query over years at 1-minute buckets built hundreds of MB of
/// points (plan 001, deferred to plan 005).
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
    // The bucket holding the last minute before `to`, and the start of the
    // one MAX_POINTS - 1 before it, in the query's own floor arithmetic.
    let per = bucket / 60;
    let last = (minute(to) - 1).div_euclid(per);
    let from = from.max(unix((last - (MAX_POINTS - 1)) * per));
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
    /// When a file was last moved aside, for `RECOVER_BACKOFF_SECS`.
    last_replaced: Option<i64>,
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

/// Makes the directory history lives in private (plan 001, deferred to the
/// first plan that writes events; the `top-apps` and `action` rows say what
/// the owner ran and stopped). A missing one is made 0700, and parents it
/// has to make (a fresh `~/.local/state`) keep the default mode (review of
/// PR #6). An existing one is tightened to 0700 only when the caller says
/// mirqab owns it (`own`), and only when it is a directory of this user's,
/// not a symlink: one reached through a path the user chose (`mirqab daemon
/// --root` keeps history beside a chosen socket) may be theirs, and someone
/// else's is never touched (review of PR #26). The files are private
/// either way.
pub fn private_dir(dir: &Path, own: bool) -> Result<(), HistoryError> {
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
            let ours = std::fs::symlink_metadata(dir)
                .is_ok_and(|m| m.is_dir() && m.uid() == crate::sys::uid());
            if own && ours {
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(failed)?;
            }
            Ok(())
        }
        Err(e) => Err(failed(e)),
    }
}

/// `private_dir` for the directory of the history file at `path`, which
/// mirqab owns only when it is the default one (`History::default_path`).
pub fn private_dir_of(path: &Path) -> Result<(), HistoryError> {
    match path.parent() {
        Some(dir) => private_dir(dir, History::default_path().parent() == Some(dir)),
        None => Ok(()),
    }
}

/// After `recover` replaced a file, damage found again within this long is
/// left in place and history is off (`broken`) until it has passed:
/// failing storage then adds a `.broken-*` file at most every six hours,
/// not every minute (review of PR #26).
pub const RECOVER_BACKOFF_SECS: i64 = 6 * 3600;

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

/// `path` with `suffix` added to its file name (`-wal`, `-shm`).
fn beside(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

/// Renames a file SQLite called corrupt, with its log and shared memory,
/// to `<name>.broken-<now>`, or `.broken-<now>.1` and on when that or its
/// log is there already: an earlier one is never overwritten. Returns the
/// new name. A log or shared memory that cannot be moved puts back what
/// moved and fails: left beside a new file, an old log would be replayed
/// into it (review of PR #26).
fn move_aside(path: &Path, now_unix: i64) -> std::io::Result<PathBuf> {
    const SIDES: [&str; 2] = ["-wal", "-shm"];
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let taken = |p: &Path| {
        ["", SIDES[0], SIDES[1]]
            .iter()
            .any(|s| std::fs::symlink_metadata(beside(p, s)).is_ok())
    };
    let aside = (0..1000)
        .map(|n| match n {
            0 => path.with_file_name(format!("{name}.broken-{now_unix}")),
            n => path.with_file_name(format!("{name}.broken-{now_unix}.{n}")),
        })
        .find(|p| !taken(p))
        .ok_or_else(|| {
            std::io::Error::new(ErrorKind::AlreadyExists, strings::HISTORY_ASIDE_TAKEN)
        })?;
    std::fs::rename(path, &aside)?;
    let mut moved: Vec<&str> = Vec::new();
    for s in SIDES {
        let side = beside(path, s);
        if std::fs::symlink_metadata(&side).is_err() {
            continue;
        }
        if let Err(e) = std::fs::rename(&side, beside(&aside, s)) {
            for m in moved.iter().rev() {
                let _ = std::fs::rename(beside(&aside, m), beside(path, m));
            }
            let _ = std::fs::rename(&aside, path);
            return Err(e);
        }
        moved.push(s);
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
        private_dir_of(path)?;
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
            last_replaced: moved_aside.as_ref().map(|_| now_unix),
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
    /// while every write and query failed). Within `RECOVER_BACKOFF_SECS`
    /// of the last replacement the file is left in place and `broken`.
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
            // A clock set back counts as past the backoff.
            if let Some(t) = self.last_replaced
                && (0..RECOVER_BACKOFF_SECS).contains(&(now_unix - t))
            {
                return Err(HistoryError::msg(strings::history_damaged_again(
                    RECOVER_BACKOFF_SECS / 3600,
                )));
            }
            let aside = move_aside(&self.path, now_unix)
                .map_err(|e| HistoryError::msg(format!("{}: {e}", self.path.display())))?;
            self.corrupt_in_place = false;
            self.moved_aside = Some(aside);
            self.last_replaced = Some(now_unix);
        }
        let fresh = History::open(&self.path, now_unix)?;
        *self = History {
            moved_aside: fresh.moved_aside.clone().or(self.moved_aside.take()),
            last_replaced: fresh.last_replaced.or(self.last_replaced),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_moves_aside_with_its_log_to_a_name_not_taken() {
        let dir = std::env::temp_dir().join(format!("mirqab-aside-{}", std::process::id()));
        let _ = crate::act::remove_path(&dir, false);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.db");
        for (name, body) in [
            ("history.db", "db"),
            ("history.db-wal", "wal"),
            ("history.db-shm", "shm"),
            ("history.db.broken-100", "first"),
            ("history.db.broken-100.1-shm", "second's shm"),
        ] {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let aside = move_aside(&path, 100).unwrap();
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).ok();
        assert_eq!(aside, dir.join("history.db.broken-100.2"));
        assert_eq!(read("history.db.broken-100.2").as_deref(), Some("db"));
        assert_eq!(read("history.db.broken-100.2-wal").as_deref(), Some("wal"));
        assert_eq!(read("history.db.broken-100.2-shm").as_deref(), Some("shm"));
        assert_eq!(read("history.db.broken-100").as_deref(), Some("first"));
        assert_eq!(
            read("history.db.broken-100.1-shm").as_deref(),
            Some("second's shm")
        );
        for gone in ["history.db", "history.db-wal", "history.db-shm"] {
            assert!(read(gone).is_none(), "{gone}");
        }
        let _ = crate::act::remove_path(&dir, false);
    }
}
