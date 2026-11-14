//! Desktop entries (plan 006): the name and icon of a group whose scope
//! names an application id (systemd's XDG convention,
//! https://systemd.io/DESKTOP_ENVIRONMENTS/), or whose main process is an
//! entry's `Exec=` program. Read again only when one of the directories
//! changed.

use super::procfs::basename;
use crate::root::Root;
use std::collections::HashMap;
use std::time::SystemTime;

/// What a group takes from an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub icon: Option<String>,
}

/// The application directories, highest precedence first: the user's own
/// entries override the system's (XDG Base Directory). A fixed list
/// rather than `$XDG_DATA_DIRS`, so a fixture tree reads the same in every
/// environment.
fn dirs(home: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(h) = home
        .map(|h| h.trim_start_matches('/'))
        .filter(|h| !h.is_empty())
    {
        out.push(format!("{h}/.local/share/applications"));
        out.push(format!(
            "{h}/.local/share/flatpak/exports/share/applications"
        ));
    }
    out.extend(
        [
            "var/lib/flatpak/exports/share/applications",
            "usr/local/share/applications",
            "usr/share/applications",
        ]
        .map(String::from),
    );
    out
}

#[derive(Debug, Default)]
pub struct Desktop {
    dirs: Vec<String>,
    stamps: Vec<Option<SystemTime>>,
    entries: Vec<Entry>,
    /// Desktop id (file name without `.desktop`) to entry; None where a
    /// higher directory hides the id (`Hidden=true`).
    by_id: HashMap<String, Option<usize>>,
    by_lower: HashMap<String, usize>,
    /// The `Exec=` program's file name, and `StartupWMClass`, lower case.
    by_exec: HashMap<String, usize>,
    loaded: bool,
}

/// One parsed entry: what a group shows, the program it runs, its window
/// class, and whether it is hidden (deleted, in the specification's words).
struct Parsed {
    entry: Entry,
    exec: Option<String>,
    wm_class: Option<String>,
    hidden: bool,
}

/// The `[Desktop Entry]` group of a `.desktop` file. Localised keys
/// (`Name[de]=`) and action groups are skipped.
fn parse(text: &str) -> Option<Parsed> {
    let mut in_main = false;
    let (mut name, mut icon, mut exec, mut wm_class, mut hidden) = (None, None, None, None, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim() {
            "Name" => name = Some(val.to_string()),
            "Icon" => icon = Some(val.to_string()).filter(|v| !v.is_empty()),
            "Exec" => exec = exec_program(val),
            "StartupWMClass" => wm_class = Some(val.to_lowercase()),
            "Hidden" => hidden = val == "true",
            _ => {}
        }
    }
    Some(Parsed {
        entry: Entry { name: name?, icon },
        exec,
        wm_class,
        hidden,
    })
}

/// The program an `Exec=` line runs, by file name: `env VAR=x prog %u` is
/// `prog`. A `flatpak run` line names no program of the host.
fn exec_program(exec: &str) -> Option<String> {
    let mut words = exec
        .split_whitespace()
        .map(|w| w.trim_matches('"'))
        .skip_while(|w| *w == "env" || w.contains('='));
    let prog = basename(words.next()?);
    (prog != "flatpak" && !prog.is_empty()).then(|| prog.to_string())
}

impl Desktop {
    pub fn new(home: Option<&str>) -> Self {
        let dirs = dirs(home);
        Desktop {
            stamps: vec![None; dirs.len()],
            dirs,
            ..Default::default()
        }
    }

    /// Reads the entries again when a directory appeared, vanished or
    /// changed since the last read; otherwise costs one `stat` per
    /// directory.
    pub fn refresh(&mut self, root: &Root) {
        let stamps: Vec<Option<SystemTime>> = self.dirs.iter().map(|d| root.modified(d)).collect();
        if self.loaded && stamps == self.stamps {
            return;
        }
        self.stamps = stamps;
        self.loaded = true;
        self.entries.clear();
        self.by_id.clear();
        self.by_lower.clear();
        self.by_exec.clear();
        for dir in &self.dirs {
            let Ok(names) = root.read_dir(dir) else {
                continue;
            };
            for file in names {
                let Some(id) = file.strip_suffix(".desktop") else {
                    continue;
                };
                // A higher directory already decided this id.
                if self.by_id.contains_key(id) {
                    continue;
                }
                let Some(p) = root
                    .read(&format!("{dir}/{file}"))
                    .ok()
                    .as_deref()
                    .and_then(parse)
                else {
                    continue;
                };
                if p.hidden {
                    self.by_id.insert(id.to_string(), None);
                    continue;
                }
                let n = self.entries.len();
                self.entries.push(p.entry);
                self.by_id.insert(id.to_string(), Some(n));
                self.by_lower.entry(id.to_lowercase()).or_insert(n);
                for key in [p.exec, p.wm_class].into_iter().flatten() {
                    self.by_exec.entry(key.to_lowercase()).or_insert(n);
                }
            }
        }
    }

    /// The entry whose desktop id is `id`, matched exactly and then
    /// ignoring case.
    pub fn by_id(&self, id: &str) -> Option<&Entry> {
        match self.by_id.get(id) {
            Some(Some(n)) => self.entries.get(*n),
            Some(None) => None,
            None => self
                .by_lower
                .get(&id.to_lowercase())
                .and_then(|n| self.entries.get(*n)),
        }
    }

    /// The entry whose `Exec=` program or window class is `program`.
    pub fn by_exec(&self, program: &str) -> Option<&Entry> {
        self.by_exec
            .get(&program.to_lowercase())
            .and_then(|n| self.entries.get(*n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_main_group_only() {
        let p = parse(
            "[Desktop Entry]\nName[de]=Rechner\nName=Calculator\nExec=env A=b calc %u\nIcon=calc\n\
             [Desktop Action x]\nName=Other\nExec=other\n",
        )
        .unwrap();
        assert_eq!(p.entry.name, "Calculator");
        assert_eq!(p.entry.icon.as_deref(), Some("calc"));
        assert_eq!(p.exec.as_deref(), Some("calc"));
        assert!(!p.hidden);
        assert!(parse("[Desktop Entry]\nExec=x\n").is_none(), "no name");
    }

    #[test]
    fn flatpak_exec_lines_name_no_program() {
        assert_eq!(exec_program("/usr/bin/flatpak run org.example.App"), None);
        assert_eq!(
            exec_program("\"/opt/x/bin/app\" %F").as_deref(),
            Some("app")
        );
    }
}
