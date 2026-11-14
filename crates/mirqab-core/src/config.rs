//! `~/.config/mirqab/config.json` (architecture §5.5). The daemon is its only
//! writer; clients change it through `config.set`. A file the daemon cannot
//! parse is left untouched, reported in `status`, and defaults are used: the
//! app never rewrites a file the user broke by hand.
//!
//! The keys and their checks below mirror `share/mirqab/config.schema.json`;
//! a test below checks that the two name the same keys and defaults.

use crate::act;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Every key with its default (plan 001, "Config keys added here").
pub fn defaults() -> Value {
    json!({
        "window": {"mode": "window", "anchor": "top-right", "marginEm": 0.5, "widthEm": 58, "heightEm": 40, "rememberTab": false, "lastTab": null},
        "live": {"intervalMs": 1000},
        "theme": {"follow": null, "accent": null, "scheme": "dark"},
        "units": {"temperature": "c", "clock": "auto"},
        "home": {"pinned": [], "seeded": false},
        "tabs": {"hidden": []},
        "bar": {"combo": ["cpu", "mem", "temp"]},
        "tray": {"enabled": true}
    })
}

/// Why a value is refused, or None when it is acceptable for `key`.
fn check(key: &str, v: &Value) -> Option<String> {
    let one_of = |allowed: &[&str]| match v.as_str() {
        Some(s) if allowed.contains(&s) => None,
        _ => Some(format!("{key} must be one of {}", allowed.join(", "))),
    };
    let number = |lo: f64, hi: f64| match v.as_f64() {
        Some(n) if (lo..=hi).contains(&n) => None,
        _ => Some(format!("{key} must be a number from {lo} to {hi}")),
    };
    let string_or_null = || match v {
        Value::Null | Value::String(_) => None,
        _ => Some(format!("{key} must be a string or null")),
    };
    let strings = || match v.as_array() {
        Some(a) if a.iter().all(Value::is_string) => None,
        _ => Some(format!("{key} must be a list of strings")),
    };
    match key {
        "window.mode" => one_of(&["popover", "window"]),
        "window.anchor" => one_of(&[
            "top-left",
            "top",
            "top-right",
            "bottom-left",
            "bottom",
            "bottom-right",
        ]),
        "window.marginEm" => number(0.0, 10.0),
        "window.widthEm" => number(30.0, 120.0),
        "window.heightEm" => number(20.0, 80.0),
        "window.rememberTab" | "home.seeded" | "tray.enabled" => match v {
            Value::Bool(_) => None,
            _ => Some(format!("{key} must be true or false")),
        },
        "window.lastTab" => match v {
            Value::Null => None,
            Value::String(t)
                if (1..=32).contains(&t.len())
                    && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') =>
            {
                None
            }
            _ => Some(format!("{key} must be a tab id or null")),
        },
        "live.intervalMs" => match v.as_u64() {
            Some(n) if (1000..=5000).contains(&n) => None,
            _ => Some(format!(
                "{key} must be a whole number of milliseconds from 1000 to 5000"
            )),
        },
        "theme.follow" | "theme.accent" => string_or_null(),
        "theme.scheme" => one_of(&["dark", "light", "system"]),
        "units.temperature" => one_of(&["c", "f"]),
        "units.clock" => one_of(&["auto", "12", "24"]),
        "home.pinned" | "tabs.hidden" => strings(),
        "bar.combo" => match v.as_array() {
            Some(a)
                if a.iter().all(|x| {
                    x.as_str().is_some_and(|n| {
                        n != "combo"
                            && crate::proto::strings::BAR_ITEMS
                                .iter()
                                .any(|(i, _)| *i == n)
                    })
                }) =>
            {
                None
            }
            _ => Some(format!(
                "{key} must be a list of bar items other than combo (mirqab bar --list)"
            )),
        },
        _ => Some(format!("unknown setting: {key}")),
    }
}

/// The keys `check` knows, as dotted paths.
pub fn keys() -> Vec<String> {
    let mut out = Vec::new();
    if let Value::Object(groups) = defaults() {
        for (g, v) in groups {
            if let Value::Object(fields) = v {
                for f in fields.keys() {
                    out.push(format!("{g}.{f}"));
                }
            }
        }
    }
    out
}

fn lookup<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(v, |cur, part| cur.get(part))
}

fn set_path(v: &mut Value, key: &str, new: Value) {
    let mut cur = v;
    let parts: Vec<&str> = key.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        if !cur.get(*part).is_some_and(Value::is_object) {
            cur[*part] = Value::Object(Map::new());
        }
        cur = &mut cur[*part];
    }
    cur[parts[parts.len() - 1]] = new;
}

/// Overlays the user's file on the defaults, key by key. A key with a bad
/// value keeps its default; the reason is collected.
fn merge(user: &Value) -> (Value, Vec<String>) {
    let mut out = defaults();
    let mut problems = Vec::new();
    for key in keys() {
        if let Some(v) = lookup(user, &key) {
            match check(&key, v) {
                None => set_path(&mut out, &key, v.clone()),
                Some(why) => problems.push(why),
            }
        }
    }
    (out, problems)
}

pub struct Config {
    path: PathBuf,
    /// Defaults with the user's valid values on top.
    effective: Value,
    /// What the file held, kept so a set changes one key and nothing else.
    file: Value,
    /// Set when the file exists but is not a JSON object.
    broken: Option<String>,
    problems: Vec<String>,
    /// The file's modification time and length when it was last read, so a
    /// hand edit while the daemon runs is noticed (review of phase B).
    stamp: Option<(std::time::SystemTime, u64)>,
}

fn stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

#[derive(Debug, PartialEq)]
pub enum SetError {
    Invalid(String),
    /// The file is broken; the daemon does not overwrite it.
    Broken(String),
    Write(String),
}

impl Config {
    /// `$XDG_CONFIG_HOME/mirqab/config.json`, else `~/.config/mirqab/config.json`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        base.join("mirqab/config.json")
    }

    pub fn load(path: &Path) -> Config {
        let mut c = Config {
            path: path.to_path_buf(),
            effective: defaults(),
            file: Value::Object(Map::new()),
            broken: None,
            problems: Vec::new(),
            stamp: None,
        };
        c.read();
        c
    }

    /// Reads the file afresh: what is on disk now is the truth, whatever an
    /// earlier read found.
    fn read(&mut self) {
        self.effective = defaults();
        self.file = Value::Object(Map::new());
        self.broken = None;
        self.problems = Vec::new();
        self.stamp = stamp(&self.path);
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.broken = Some(e.to_string()),
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(v @ Value::Object(_)) => {
                    let (effective, problems) = merge(&v);
                    self.effective = effective;
                    self.problems = problems;
                    self.file = v;
                }
                Ok(_) => self.broken = Some("not a JSON object".into()),
                Err(e) => self.broken = Some(e.to_string()),
            },
        }
    }

    /// Re-reads the file when it changed on disk since the last read. Cheap:
    /// one stat. Callers run it before answering from the config.
    pub fn refresh(&mut self) {
        if stamp(&self.path) != self.stamp {
            self.read();
        }
    }

    pub fn effective(&self) -> &Value {
        &self.effective
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        lookup(&self.effective, key)
    }

    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The live interval floor, `live.intervalMs`.
    pub fn live_interval_ms(&self) -> u64 {
        self.get("live.intervalMs")
            .and_then(Value::as_u64)
            .unwrap_or(1000)
    }

    /// Changes one key and writes the file atomically. Under `dry_run` the
    /// change is checked and described but nothing is written.
    pub fn set(&mut self, key: &str, value: Value, dry_run: bool) -> Result<Value, SetError> {
        if let Some(why) = check(key, &value) {
            return Err(SetError::Invalid(why));
        }
        // Always from the file as it is now: a hand edit or a breakage since
        // the last read must be kept or refused, never written over. (A stat
        // can miss an edit within the same timestamp tick; a read cannot.)
        self.read();
        if let Some(why) = &self.broken {
            return Err(SetError::Broken(why.clone()));
        }
        let mut file = self.file.clone();
        set_path(&mut file, key, value.clone());
        let mut text = serde_json::to_vec_pretty(&file).unwrap_or_default();
        text.push(b'\n');
        let done = act::write_file(&self.path, &text, dry_run)
            .map_err(|e| SetError::Write(e.to_string()))?;
        if !dry_run {
            self.file = file;
            set_path(&mut self.effective, key, value);
            // A key the user had set to a bad value may be the one just fixed.
            self.problems = merge(&self.file).1;
            self.stamp = stamp(&self.path);
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mirqab-config-{name}-{}", std::process::id()));
        let _ = act::remove_path(&d, false);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let d = scratch("missing");
        let c = Config::load(&d.join("config.json"));
        assert_eq!(c.effective(), &defaults());
        assert_eq!(c.live_interval_ms(), 1000);
        assert!(c.broken().is_none());
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_bad_value_keeps_its_default_and_is_reported() {
        let d = scratch("bad");
        let p = d.join("config.json");
        std::fs::write(
            &p,
            r#"{"live":{"intervalMs":200},"units":{"temperature":"f"},"mine":1}"#,
        )
        .unwrap();
        let mut c = Config::load(&p);
        assert_eq!(c.live_interval_ms(), 1000);
        assert_eq!(c.get("units.temperature"), Some(&json!("f")));
        assert_eq!(c.problems().len(), 1);
        // Setting that key to a good value clears the problem.
        c.set("live.intervalMs", json!(1500), false).unwrap();
        assert!(c.problems().is_empty());
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_broken_file_is_never_rewritten() {
        let d = scratch("broken");
        let p = d.join("config.json");
        std::fs::write(&p, "{not json").unwrap();
        let mut c = Config::load(&p);
        assert!(c.broken().is_some());
        assert!(matches!(
            c.set("units.temperature", json!("f"), false),
            Err(SetError::Broken(_))
        ));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{not json");
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn a_file_broken_or_edited_after_load_is_kept() {
        let d = scratch("later");
        let p = d.join("config.json");
        std::fs::write(&p, r#"{"units":{"temperature":"f"}}"#).unwrap();
        let mut c = Config::load(&p);
        // Broken by hand while the daemon runs: refused, the file kept.
        std::fs::write(&p, "{oops").unwrap();
        assert!(matches!(
            c.set("units.clock", json!("24"), false),
            Err(SetError::Broken(_))
        ));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{oops");
        // Fixed and extended by hand: the edit is kept and the set applies.
        std::fs::write(
            &p,
            r#"{"units":{"temperature":"c"},"tabs":{"hidden":["fans"]}}"#,
        )
        .unwrap();
        c.set("units.clock", json!("12"), false).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            written,
            json!({"units": {"temperature": "c", "clock": "12"}, "tabs": {"hidden": ["fans"]}})
        );
        assert_eq!(c.get("tabs.hidden"), Some(&json!(["fans"])));
        // A later edit is picked up by refresh.
        std::fs::write(
            &p,
            r#"{"units":{"temperature":"f","clock":"24"},"live":{"intervalMs":3000}}"#,
        )
        .unwrap();
        c.refresh();
        assert_eq!(c.live_interval_ms(), 3000);
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn set_checks_writes_and_keeps_unknown_keys() {
        let d = scratch("set");
        let p = d.join("config.json");
        std::fs::write(&p, r#"{"mine":{"keep":true}}"#).unwrap();
        let mut c = Config::load(&p);
        assert!(matches!(
            c.set("live.intervalMs", json!(500), false),
            Err(SetError::Invalid(_))
        ));
        assert!(matches!(
            c.set("no.such", json!(1), false),
            Err(SetError::Invalid(_))
        ));
        c.set("live.intervalMs", json!(2000), true).unwrap();
        assert_eq!(c.live_interval_ms(), 1000, "a dry run changes nothing");
        c.set("live.intervalMs", json!(2000), false).unwrap();
        assert_eq!(c.live_interval_ms(), 2000);
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            written,
            json!({"mine": {"keep": true}, "live": {"intervalMs": 2000}})
        );
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn home_and_tab_keys_validate_and_keep_order() {
        let d = scratch("home");
        let p = d.join("config.json");
        let mut c = Config::load(&p);
        let order = json!(["system.memory", "system.cpu", "gone.card"]);
        c.set("home.pinned", order.clone(), false).unwrap();
        for bad in [json!(["system.cpu", 3]), json!("system.cpu"), json!(null)] {
            assert!(matches!(
                c.set("home.pinned", bad, false),
                Err(SetError::Invalid(_))
            ));
        }
        c.set("home.seeded", json!(true), false).unwrap();
        assert!(c.set("home.seeded", json!("yes"), false).is_err());
        c.set("bar.combo", json!(["load", "cpu"]), false).unwrap();
        for bad in [json!(["combo"]), json!(["cpu", "nope"]), json!("cpu")] {
            assert!(c.set("bar.combo", bad, false).is_err());
        }
        c.set("window.rememberTab", json!(true), false).unwrap();
        c.set("window.lastTab", json!("system"), false).unwrap();
        assert!(c.set("window.lastTab", json!("../x"), false).is_err());
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(written["home"], json!({"pinned": order, "seeded": true}));
        assert_eq!(written["window"]["lastTab"], "system");
        // Written atomically: no temporary file is left beside it.
        let names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["config.json"]);
        act::remove_path(&d, false).unwrap();
    }

    #[test]
    fn the_window_opens_as_a_normal_window_by_default() {
        assert_eq!(defaults()["window"]["mode"], "window");
    }

    #[test]
    fn the_schema_names_the_same_keys_and_defaults() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../share/mirqab/config.schema.json")).unwrap();
        let mut from_schema = Vec::new();
        for (g, group) in schema["properties"].as_object().unwrap() {
            for (f, field) in group["properties"].as_object().unwrap() {
                let key = format!("{g}.{f}");
                assert_eq!(
                    Some(&field["default"]),
                    lookup(&defaults(), &key),
                    "default of {key}"
                );
                from_schema.push(key);
            }
        }
        let mut ours = keys();
        ours.sort();
        from_schema.sort();
        assert_eq!(ours, from_schema);
    }

    #[test]
    fn every_default_passes_its_own_check() {
        let d = defaults();
        for key in keys() {
            assert_eq!(check(&key, lookup(&d, &key).unwrap()), None, "{key}");
        }
    }
}
