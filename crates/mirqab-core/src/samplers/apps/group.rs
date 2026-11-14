//! Which app a process belongs to (plan 006, "Grouping"). A process of the
//! user's belongs to the first of:
//!
//! 1. its systemd app scope or service under the user manager, named from
//!    the desktop entry its application id maps to, else after its main
//!    process (never the launcher: `app-Hyprland-zen.sh-….scope` is named
//!    after the browser, not Hyprland);
//! 2. a Flatpak app id (the `app-flatpak-<id>-<n>.scope`, or the sandbox's
//!    `.flatpak-info`);
//! 3. a container (one group per container);
//! 4. a terminal session: inside a terminal's scope, the processes of each
//!    shell's session are grouped apart and named after its foreground job,
//!    so `vite` in a terminal is not "the terminal";
//! 5. the process tree: the highest ancestor that is not systemd, a shell
//!    or a terminal.
//!
//! A Flatpak or container scope is checked before rule 1: such a scope sits
//! under the user manager too, and is not an app scope in rule 1's sense.
//! Processes of other users make one "System" group; kernel threads are
//! left out.

use super::desktop::{Desktop, Entry};
use super::procfs::{Proc, basename};
use crate::proto::strings;
use std::collections::{BTreeMap, HashMap};

/// Shells, by command name: a shell is never a group's main process, and
/// rule 5 stops below one.
pub const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "ksh", "mksh", "tcsh", "csh", "nu", "elvish", "xonsh",
    "ion", "-sh", "-bash", "-zsh", "-fish",
];

/// Terminal emulators, by command name (cut to 15 bytes, as the kernel
/// keeps it). A terminal's scope is split by session (rule 4).
const TERMINALS: &[&str] = &[
    "foot",
    "footclient",
    "kitty",
    "alacritty",
    "Alacritty",
    "ghostty",
    "wezterm-gui",
    "konsole",
    "gnome-terminal-",
    "kgx",
    "xterm",
    "urxvt",
    "st",
    "terminator",
    "tilix",
    "xfce4-terminal",
    "rio",
    "contour",
    "warp-terminal",
];

/// Interpreters: a job run by one is named after its script
/// (`node node_modules/.bin/vite` is `vite`).
pub const INTERPRETERS: &[&str] = &["node", "python", "python3", "ruby", "perl", "deno"];

pub fn is_shell(comm: &str) -> bool {
    SHELLS.contains(&comm)
}

pub fn is_terminal(comm: &str) -> bool {
    TERMINALS.contains(&comm)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    App,
    Service,
    Flatpak,
    Container,
    Terminal,
    Process,
    System,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::Service => "service",
            Kind::Flatpak => "flatpak",
            Kind::Container => "container",
            Kind::Terminal => "terminal",
            Kind::Process => "process",
            Kind::System => "system",
        }
    }
}

/// One group of the `apps` stream, with its members as indexes into the
/// process list it was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub kind: Kind,
    pub unit: Option<String>,
    pub members: Vec<usize>,
}

/// The deepest systemd unit under the user manager in a cgroup path:
/// `…/user@1000.service/app.slice/app-foo-1.scope` is `app-foo-1.scope`.
/// None outside the user manager.
pub fn user_unit(cgroup: &str) -> Option<&str> {
    let parts: Vec<&str> = cgroup.split('/').collect();
    let manager = parts
        .iter()
        .position(|p| p.starts_with("user@") && p.ends_with(".service"))?;
    parts[manager + 1..]
        .iter()
        .rev()
        .find(|p| p.ends_with(".scope") || p.ends_with(".service"))
        .copied()
}

/// The deepest unit anywhere in a cgroup path, for the inspector.
pub fn any_unit(cgroup: &str) -> Option<&str> {
    cgroup
        .split('/')
        .rev()
        .find(|p| p.ends_with(".scope") || p.ends_with(".service"))
}

/// A container's id, cut to the 12 characters `docker ps` shows: a
/// `docker-<id>.scope`, `libpod-<id>.scope` (Podman, also rootless),
/// `crio-<id>.scope` or `cri-containerd-<id>.scope` unit, or a `docker/<id>`
/// pair of path parts (cgroupfs driver).
pub fn container_id(cgroup: &str) -> Option<String> {
    let hex = |s: &str| s.len() >= 12 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let parts: Vec<&str> = cgroup.split('/').collect();
    for (i, p) in parts.iter().enumerate() {
        if let Some(stem) = p.strip_suffix(".scope") {
            for prefix in ["docker-", "libpod-", "crio-", "cri-containerd-"] {
                if let Some(id) = stem.strip_prefix(prefix)
                    && hex(id)
                {
                    return Some(id[..12].to_string());
                }
            }
        }
        if *p == "docker"
            && let Some(id) = parts.get(i + 1)
            && hex(id)
        {
            return Some(id[..12].to_string());
        }
    }
    None
}

/// A unit name's `\xNN` escapes decoded (systemd escapes `-` inside an
/// application id as `\x2d`, a space as `\x20`, systemd.unit(5)). Any other
/// backslash is left as written. Applied to the ids taken from a unit name,
/// never to the unit name itself, so group ids stay what systemd shows.
pub fn unit_unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && b.get(i + 1) == Some(&b'x')
            && let Some(hex) = b.get(i + 2..i + 4)
            // from_str_radix would take a leading `+`; only two hex digits do.
            && hex.iter().all(u8::is_ascii_hexdigit)
            && let Ok(h) = std::str::from_utf8(hex)
            && let Ok(byte) = u8::from_str_radix(h, 16)
        {
            out.push(byte);
            i += 4;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `app-flatpak-<app id>-<n>.scope` → the app id.
pub fn flatpak_scope(unit: &str) -> Option<String> {
    let rest = unit.strip_prefix("app-flatpak-")?.strip_suffix(".scope")?;
    let id = match rest.rsplit_once('-') {
        Some((id, n)) if n.bytes().all(|b| b.is_ascii_digit()) => id,
        _ => rest,
    };
    (!id.is_empty()).then(|| unit_unescape(id))
}

/// The application ids a unit name may carry, most likely first. systemd's
/// convention is `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope` and
/// `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service`; an id may hold
/// `-` itself, so with and without a leading launcher are both tried
/// against the desktop entries. A unit outside the convention offers its
/// own stem (`pipewire.service` → `pipewire`).
pub fn unit_app_ids(unit: &str) -> Vec<String> {
    let (stem, scope) = match unit.strip_suffix(".scope") {
        Some(s) => (s, true),
        None => (unit.strip_suffix(".service").unwrap_or(unit), false),
    };
    let Some(rest) = stem.strip_prefix("app-") else {
        return vec![stem.split('@').next().unwrap_or(stem).to_string()];
    };
    let id = if scope {
        match rest.rsplit_once('-') {
            Some((id, random)) if random.bytes().all(|b| b.is_ascii_alphanumeric()) => id,
            _ => rest,
        }
    } else {
        rest.split('@').next().unwrap_or(rest)
    };
    // Split on the unit name's own dashes first: an escaped `\x2d` belongs
    // to the id.
    let mut out = vec![unit_unescape(id)];
    if let Some((_, without_launcher)) = id.split_once('-') {
        out.push(unit_unescape(without_launcher));
    }
    out
}

/// What a process is grouped by before names are given.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Base {
    System,
    Unit(String),
    Flatpak(String),
    Container(String),
    Tree,
}

fn base(p: &Proc, own_uid: u32) -> Base {
    let cg = p.info.cgroup.as_deref().unwrap_or("");
    // A rootless container under the user's own manager is the user's,
    // whatever uid its processes run as: an image that drops root runs on
    // one of the user's sub-uids (review of PR #16).
    let manager = format!("user@{own_uid}.service");
    if let Some(id) = container_id(cg)
        && cg.split('/').any(|part| part == manager)
    {
        return Base::Container(id);
    }
    if p.info.uid != Some(own_uid) {
        return Base::System;
    }
    if let Some(id) = container_id(cg) {
        return Base::Container(id);
    }
    if let Some(unit) = user_unit(cg) {
        return match flatpak_scope(unit) {
            Some(id) => Base::Flatpak(id),
            None => Base::Unit(unit.to_string()),
        };
    }
    match &p.info.flatpak {
        Some(id) => Base::Flatpak(id.clone()),
        None => Base::Tree,
    }
}

/// The oldest member that is not a shell (a launcher script's shell is
/// older than the app it starts), else the oldest.
fn main_process(procs: &[Proc], members: &[usize]) -> usize {
    let oldest = |it: &mut dyn Iterator<Item = &usize>| {
        it.min_by_key(|&&i| (procs[i].stat.start, procs[i].pid()))
            .copied()
    };
    oldest(&mut members.iter().filter(|&&i| !is_shell(&procs[i].stat.comm)))
        .or_else(|| oldest(&mut members.iter()))
        .unwrap_or(members[0])
}

/// A job's name: the script an interpreter runs, else the process's name.
fn job_name(p: &Proc) -> String {
    if INTERPRETERS.contains(&p.stat.comm.as_str())
        && let Some(script) = p.info.cmdline.iter().skip(1).find(|a| !a.starts_with('-'))
    {
        return basename(script).to_string();
    }
    p.name()
}

fn named(entry: Option<&Entry>, fallback: String) -> (String, Option<String>) {
    match entry {
        Some(e) => (e.name.clone(), e.icon.clone()),
        None => (fallback, None),
    }
}

/// Every visible process of `procs` in exactly one group, the groups in
/// order of their ids.
pub fn group(procs: &[Proc], own_uid: u32, desktop: &Desktop) -> Vec<Group> {
    let by_pid: HashMap<u32, usize> = procs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.pid(), i))
        .collect();
    let bases: Vec<Option<Base>> = procs
        .iter()
        .map(|p| (!p.kernel()).then(|| base(p, own_uid)))
        .collect();
    let mut by_base: BTreeMap<Base, Vec<usize>> = BTreeMap::new();
    for (i, b) in bases.iter().enumerate() {
        if let Some(b) = b {
            by_base.entry(b.clone()).or_default().push(i);
        }
    }

    let mut out: Vec<Group> = Vec::new();
    for (b, members) in by_base {
        match b {
            Base::System => out.push(Group {
                id: "system".to_string(),
                name: strings::APPS_SYSTEM.to_string(),
                icon: None,
                kind: Kind::System,
                unit: None,
                members,
            }),
            Base::Container(id) => {
                let main = &procs[main_process(procs, &members)];
                out.push(Group {
                    id: format!("container:{id}"),
                    name: strings::apps_container(&main.name(), &id),
                    icon: None,
                    kind: Kind::Container,
                    unit: None,
                    members,
                });
            }
            Base::Flatpak(id) => {
                let (name, icon) = named(desktop.by_id(&id), id.clone());
                out.push(Group {
                    id: format!("flatpak:{id}"),
                    name,
                    icon,
                    kind: Kind::Flatpak,
                    unit: None,
                    members,
                });
            }
            Base::Unit(unit) => unit_groups(procs, &by_pid, desktop, unit, members, &mut out),
            Base::Tree => tree_groups(procs, &by_pid, &bases, desktop, members, &mut out),
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Rule 1, and rule 4 inside a terminal's scope.
fn unit_groups(
    procs: &[Proc],
    by_pid: &HashMap<u32, usize>,
    desktop: &Desktop,
    unit: String,
    members: Vec<usize>,
    out: &mut Vec<Group>,
) {
    let main_i = main_process(procs, &members);
    let main = &procs[main_i];
    let mut own = members.clone();
    if is_terminal(&main.stat.comm) {
        // Each session led by a member of this scope, other than the
        // terminal's own, is a job of its own unless only shells are in it.
        let mut sessions: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &i in &members {
            let sid = procs[i].stat.sid;
            let led_here = by_pid.get(&sid).is_some_and(|l| members.contains(l));
            if sid != main.stat.sid && led_here {
                sessions.entry(sid).or_default().push(i);
            }
        }
        for (sid, job) in sessions {
            let Some(name) = session_job(procs, by_pid, sid, &job) else {
                continue;
            };
            own.retain(|i| !job.contains(i));
            out.push(Group {
                id: format!("session:{sid}"),
                name,
                icon: None,
                kind: Kind::Terminal,
                unit: None,
                members: job,
            });
        }
    }
    let stem = unit
        .strip_suffix(".scope")
        .or_else(|| unit.strip_suffix(".service"))
        .unwrap_or(&unit);
    let service = unit.ends_with(".service") || unit == "init.scope";
    let entry = unit_app_ids(&unit)
        .iter()
        .find_map(|id| desktop.by_id(id))
        .or_else(|| desktop.by_exec(&main.name()));
    // A plain service is known by its unit's name; a scope or an app
    // service after its main process (the launcher is in the unit name,
    // the app is not always).
    let fallback = if unit.ends_with(".service") && !stem.starts_with("app-") {
        stem.split('@').next().unwrap_or(stem).to_string()
    } else {
        main.name()
    };
    let (name, icon) = named(entry, fallback);
    out.push(Group {
        id: format!("{}:{stem}", if service { "service" } else { "scope" }),
        name,
        icon,
        kind: if service { Kind::Service } else { Kind::App },
        unit: Some(unit),
        members: own,
    });
}

/// A session's job name: its foreground process group's leader when that
/// is not a shell, else its oldest process that is not a shell. None when
/// the session holds only shells (an idle prompt stays with the terminal).
fn session_job(
    procs: &[Proc],
    by_pid: &HashMap<u32, usize>,
    sid: u32,
    job: &[usize],
) -> Option<String> {
    let leader = by_pid.get(&sid).map(|&i| &procs[i])?;
    let fg = u32::try_from(leader.stat.tpgid)
        .ok()
        .and_then(|pg| by_pid.get(&pg))
        .filter(|i| job.contains(i))
        .map(|&i| &procs[i])
        .filter(|p| !is_shell(&p.stat.comm));
    let pick = fg.or_else(|| {
        job.iter()
            .map(|&i| &procs[i])
            .filter(|p| !is_shell(&p.stat.comm))
            .min_by_key(|p| (p.stat.start, p.pid()))
    })?;
    Some(job_name(pick))
}

/// Rule 5: each process joins its highest ancestor that is not systemd, a
/// shell or a terminal, climbing only through other rule-5 processes of
/// the same user.
fn tree_groups(
    procs: &[Proc],
    by_pid: &HashMap<u32, usize>,
    bases: &[Option<Base>],
    desktop: &Desktop,
    members: Vec<usize>,
    out: &mut Vec<Group>,
) {
    let mut trees: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in members {
        let mut top = i;
        // A loop in the parent links (pids reused mid-walk) cannot hang it.
        for _ in 0..64 {
            let Some(&parent) = by_pid.get(&procs[top].stat.ppid) else {
                break;
            };
            let pp = &procs[parent];
            let stop = pp.pid() <= 1
                || bases[parent] != Some(Base::Tree)
                || pp.stat.comm == "systemd"
                || is_shell(&pp.stat.comm)
                || is_terminal(&pp.stat.comm);
            if stop {
                break;
            }
            top = parent;
        }
        trees.entry(top).or_default().push(i);
    }
    for (top, members) in trees {
        let root = &procs[top];
        let (name, icon) = named(desktop.by_exec(&root.name()), root.name());
        out.push(Group {
            id: format!("tree:{}", root.pid()),
            name,
            icon,
            kind: Kind::Process,
            unit: None,
            members,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_under_the_user_manager() {
        let u = "/user.slice/user-1000.slice/user@1000.service";
        assert_eq!(
            user_unit(&format!("{u}/app.slice/app-Hyprland-zen.sh-5650fedc.scope")),
            Some("app-Hyprland-zen.sh-5650fedc.scope")
        );
        assert_eq!(user_unit(&format!("{u}/init.scope")), Some("init.scope"));
        assert_eq!(
            user_unit(&format!("{u}/user.slice/libpod-abc.scope/container")),
            Some("libpod-abc.scope")
        );
        assert_eq!(
            user_unit("/user.slice/user-1000.slice/session-2.scope"),
            None
        );
        assert_eq!(user_unit("/system.slice/sshd.service"), None);
        assert_eq!(any_unit("/system.slice/sshd.service"), Some("sshd.service"));
        // The unit name stays escaped; only ids taken from it are decoded.
        let escaped = format!("{u}/app.slice/app-flatpak-org.example.Two\\x2dWords-123.scope");
        let unit = user_unit(&escaped).unwrap();
        assert_eq!(unit, "app-flatpak-org.example.Two\\x2dWords-123.scope");
        assert_eq!(
            flatpak_scope(unit).as_deref(),
            Some("org.example.Two-Words")
        );
    }

    #[test]
    fn unit_escapes_are_decoded_only_when_well_formed() {
        assert_eq!(unit_unescape("two\\x2dwords"), "two-words");
        assert_eq!(unit_unescape("a\\x20b"), "a b");
        assert_eq!(unit_unescape("bad\\xZZ"), "bad\\xZZ");
        assert_eq!(unit_unescape("short\\x2"), "short\\x2");
        assert_eq!(unit_unescape("plus\\x+f"), "plus\\x+f");
        assert_eq!(unit_unescape("other\\n"), "other\\n");
    }

    #[test]
    fn application_ids_from_unit_names() {
        assert_eq!(
            unit_app_ids("app-Hyprland-zen.sh-5650fedc.scope"),
            ["Hyprland-zen.sh", "zen.sh"]
        );
        assert_eq!(
            unit_app_ids("app-com.example.App-12345.scope"),
            ["com.example.App"]
        );
        assert_eq!(
            unit_app_ids("app-org.example.Sync@autostart.service"),
            ["org.example.Sync"]
        );
        assert_eq!(unit_app_ids("pipewire.service"), ["pipewire"]);
        assert_eq!(unit_app_ids("tool-xyz-0a1b.scope"), ["tool-xyz-0a1b"]);
        assert_eq!(
            flatpak_scope("app-flatpak-org.example.Notes-48213.scope").as_deref(),
            Some("org.example.Notes")
        );
        // An escaped dash belongs to the id (review of PR #16).
        assert_eq!(
            flatpak_scope("app-flatpak-org.example.Two\\x2dWords-123.scope").as_deref(),
            Some("org.example.Two-Words")
        );
        assert_eq!(
            unit_app_ids("app-Hyprland-two\\x2dwords-0f0f0f0f.scope"),
            ["Hyprland-two-words", "two-words"]
        );
        assert_eq!(
            unit_app_ids("app-Hyprland-odd\\xZZname-0f0f0f0f.scope"),
            ["Hyprland-odd\\xZZname", "odd\\xZZname"],
            "a malformed escape stays as written"
        );
    }

    #[test]
    fn container_ids() {
        let id = "c0ffee00".repeat(8);
        assert_eq!(
            container_id(&format!("/system.slice/docker-{id}.scope")).as_deref(),
            Some("c0ffee00c0ff")
        );
        assert_eq!(
            container_id(&format!("/docker/{id}")).as_deref(),
            Some("c0ffee00c0ff")
        );
        assert_eq!(
            container_id("/user.slice/x/libpod-conmon-zz.scope"),
            None,
            "conmon is not the container"
        );
        assert_eq!(container_id("/system.slice/docker.service"), None);

        // A sub-uid process stays in a rootless container under the user's
        // manager; a foreign uid in a container under system.slice stays
        // System (review of PR #16).
        let proc_in = |uid: u32, cgroup: String| Proc {
            stat: super::super::procfs::Stat {
                pid: 10,
                comm: "db".into(),
                state: 'S',
                ppid: 1,
                pgrp: 10,
                sid: 10,
                tpgid: -1,
                flags: 0,
                ticks: 0,
                threads: 1,
                start: 1,
            },
            mem: None,
            io: None,
            info: std::sync::Arc::new(super::super::procfs::Info {
                uid: Some(uid),
                cgroup: Some(cgroup),
                ..Default::default()
            }),
        };
        let rootless =
            format!("/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-{id}.scope");
        assert_eq!(
            base(&proc_in(100_998, rootless), 1000),
            Base::Container("c0ffee00c0ff".into())
        );
        let rootful = format!("/system.slice/docker-{id}.scope");
        assert_eq!(base(&proc_in(999, rootful.clone()), 1000), Base::System);
        assert_eq!(
            base(&proc_in(1000, rootful), 1000),
            Base::Container("c0ffee00c0ff".into())
        );
    }
}
