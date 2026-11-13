//! Thirty days of per-minute history in one SQLite file (architecture §6).
//! Written by the daemon only; clients ask with `history.query` and
//! `history.events`.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "1";
/// Minute samples and events are kept this long; daily totals 90 days.
pub const KEEP_SECS: i64 = 30 * 86_400;
pub const KEEP_DAILY_DAYS: i64 = 90;
/// The most series history holds (§6.1).
pub const MAX_SERIES: usize = 64;

/// The series plan 001 fixes: key, unit, scale. Plan 002 adds the rest.
pub const SERIES: &[(&str, &str, i64)] = &[
    ("cpu.total", "percent", 100),
    ("mem.used", "bytes", 1),
    ("mem.available", "bytes", 1),
    ("swap.used", "bytes", 1),
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

/// The stored minute for a unix time.
pub fn minute(ts: i64) -> i64 {
    (ts - EPOCH).div_euclid(60)
}

/// The unix time a stored minute starts at.
pub fn unix(minute: i64) -> i64 {
    minute * 60 + EPOCH
}

pub fn range(name: &str) -> Option<(i64, i64)> {
    RANGES.iter().find(|r| r.0 == name).map(|r| (r.1, r.2))
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
    /// Set when the file was unreadable and was moved aside at open.
    pub moved_aside: Option<PathBuf>,
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
            return Err(HistoryError::msg(format!(
                "history schema {other} is newer than {SCHEMA}"
            )));
        }
    }
    Ok(conn)
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
            std::fs::create_dir_all(dir)
                .map_err(|e| HistoryError::msg(format!("{}: {e}", dir.display())))?;
        }
        let (conn, moved_aside) = match open_conn(path) {
            Ok(c) => (c, None),
            // Only a file SQLite calls not a database, or corrupt, is moved
            // aside. A newer schema, a permission error, a full disk or a
            // lock leaves the file alone and history off (review of phase C:
            // each of those hid thirty days of data).
            Err(first) if first.1 => {
                let aside = path.with_file_name(format!(
                    "{}.broken-{now_unix}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                std::fs::rename(path, &aside).map_err(|e| {
                    HistoryError::msg(format!("{first}; and moving it aside failed: {e}"))
                })?;
                for suffix in ["-wal", "-shm"] {
                    let side = PathBuf::from(format!("{}{suffix}", path.display()));
                    if side.exists() {
                        let _ = std::fs::rename(
                            &side,
                            PathBuf::from(format!("{}{suffix}", aside.display())),
                        );
                    }
                }
                (open_conn(path)?, Some(aside))
            }
            Err(other) => return Err(other),
        };
        let mut h = History {
            conn,
            path: path.to_path_buf(),
            ids: BTreeMap::new(),
            moved_aside,
        };
        for (key, unit, scale) in SERIES {
            h.register(key, unit, *scale)?;
        }
        Ok(h)
    }

    pub fn path(&self) -> &Path {
        &self.path
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
                    return Err(HistoryError::msg(format!(
                        "history holds at most {MAX_SERIES} series"
                    )));
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
        let mut n = self.conn.execute(
            "DELETE FROM sample WHERE ts < ?1",
            [minute(now - KEEP_SECS)],
        )?;
        n += self
            .conn
            .execute("DELETE FROM event WHERE ts < ?1", [now - KEEP_SECS])?;
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
            Some(t) => now - t >= 86_400,
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
