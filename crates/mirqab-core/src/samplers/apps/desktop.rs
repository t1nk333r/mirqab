//! Desktop entries (plan 006): the name and icon of a group whose scope
//! names an application id (systemd's XDG convention,
//! https://systemd.io/DESKTOP_ENVIRONMENTS/), or whose main process is an
//! entry's `Exec=` program. Read again only when one of the directories
//! changed.

use super::group::{INTERPRETERS, SHELLS};
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

/// Programs that run something else: an `Exec=` line starting with one says
/// nothing about which app a process is, so it is no key for `by_exec`
/// (review of PR #16: `xdg-open`, `wine` and `java` named unrelated apps).
/// The toolkit's own launchers are caught by `launches` instead: this
/// crate may not name the toolkit (`tools/lint.sh`, architecture §3).
const LAUNCHERS: &[&str] = &[
    "xdg-open", "wine", "java", "electron", "flatpak", "snap", "steam", "sh", "env",
];

/// One parsed entry: what a group shows, the program it runs, its window
/// class, whether it is hidden (deleted, in the specification's words) and
/// whether it is kept out of menus (`NoDisplay`, a handler or a helper).
struct Parsed {
    entry: Entry,
    exec: Option<String>,
    wm_class: Option<String>,
    hidden: bool,
    no_display: bool,
}

/// The `[Desktop Entry]` group of a `.desktop` file. Localised keys
/// (`Name[de]=`) and action groups are skipped.
fn parse(text: &str) -> Option<Parsed> {
    let mut in_main = false;
    let (mut name, mut icon, mut exec, mut wm_class, mut hidden) = (None, None, None, None, false);
    let mut no_display = false;
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
            "NoDisplay" => no_display = val == "true",
            _ => {}
        }
    }
    Some(Parsed {
        entry: Entry { name: name?, icon },
        exec,
        wm_class,
        hidden,
        no_display,
    })
}

/// The program an `Exec=` line runs, by file name: `env VAR=x prog %u` is
/// `prog`. A shell, an interpreter or a launcher (`flatpak run`,
/// `xdg-open`, `java -jar`) names no app of its own, so gives None.
fn exec_program(exec: &str) -> Option<String> {
    let mut words = exec
        .split_whitespace()
        .map(|w| w.trim_matches('"'))
        .skip_while(|w| *w == "env" || w.contains('='));
    let prog = basename(words.next()?);
    let generic = SHELLS.contains(&prog)
        || INTERPRETERS.contains(&prog)
        || LAUNCHERS.contains(&prog)
        || launches(prog, words.next());
    (!generic && !prog.is_empty()).then(|| prog.to_string())
}

/// A program that starts another by name: a `…-launch` program, or one
/// whose first argument is `launch` or `open` (the toolkit's desktop-file
/// launcher and its `launch`/`open` subcommands).
fn launches(prog: &str, first_arg: Option<&str>) -> bool {
    prog.ends_with("-launch") || matches!(first_arg, Some("launch" | "open"))
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
                // A NoDisplay entry (a URL handler, a helper) is found by its
                // id only: its program is usually some visible app's.
                if p.no_display {
                    continue;
                }
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

    #[test]
    fn runners_and_hidden_handlers_are_no_exec_keys() {
        // Review of PR #16: a generic runner's name, or a NoDisplay handler
        // running a visible app's program, must not name a process.
        for exec in [
            "python3 /opt/x/app.py",
            "xdg-open %u",
            "java -jar x.jar",
            "/bin/sh -c 'x'",
            "example-launch org.example.App",
            "exampletool open %u",
            "exampletool launch org.example.App.desktop",
        ] {
            assert_eq!(exec_program(exec), None, "{exec}");
        }
        let d = std::env::temp_dir().join(format!("mirqab-desktop-{}", std::process::id()));
        let apps = d.join("usr/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("a-handler.desktop"),
            "[Desktop Entry]\nName=Open Link\nExec=example-browser %u\nNoDisplay=true\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("example-browser.desktop"),
            "[Desktop Entry]\nName=Example Browser\nExec=example-browser %u\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("py.desktop"),
            "[Desktop Entry]\nName=Py App\nExec=python3 /opt/x/app.py\n",
        )
        .unwrap();
        let mut desk = Desktop::new(None);
        desk.refresh(&Root::new(&d));
        assert_eq!(
            desk.by_exec("example-browser").map(|e| e.name.as_str()),
            Some("Example Browser")
        );
        assert_eq!(
            desk.by_id("a-handler").map(|e| e.name.as_str()),
            Some("Open Link"),
            "still found by its id"
        );
        assert!(desk.by_exec("python3").is_none());
        crate::act::remove_path(&d, false).unwrap();
    }
}
