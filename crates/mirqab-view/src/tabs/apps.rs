//! The Apps tab's view model (plan 006, part 3): the last `apps` frame in,
//! the rows of the list, the inspector's lines, the confirmation's lines
//! and the `apps.top` card out. The page asks the daemon for every group
//! and process once (`{"limit": 1000, "expand": true}`) and never changes
//! those params; search, sort, flatten and expanding a group are pure
//! functions here, so each answers at once and none costs a subscribe.
//!
//! Rows are keyed so that the page's open groups, its selection and a
//! pending dialog hang on a process's identity, never on a bare pid or a
//! group id alone (plan 006, D5): a process row is `(pid, start)`, and a
//! group keeps its key across frames only while it shares a process with
//! the group of the same id in the previous frame.

use crate::cards::{Card, Size};
use crate::format;
use crate::icons::{self, Glyph};
use mirqab_proto::stream::{AppGroup, AppProcess, AppsData, Inspect};
use mirqab_proto::strings as s;
use serde_json::{Map, Value, json};
use std::cmp::Ordering;
use std::collections::HashSet;

/// The Apps tab's card, pinnable to Home (plan 003).
pub const CARDS: &[Card] = &[Card {
    id: "apps.top",
    tab: "apps",
    streams: &["apps"],
    size: Size::Half,
}];

/// The daemon's `limit` when a subscriber names none.
pub const DEFAULT_LIMIT: u64 = 50;
/// The page's `limit`: every group a desktop has, and then some.
pub const PAGE_LIMIT: u64 = 1000;
/// Rows the `apps.top` card shows.
pub const TOP: usize = 5;
/// The id of the group every other user's processes fold into.
pub const SYSTEM_ID: &str = "system";

/// What the page subscribes with (decision 1).
pub fn page_params() -> Value {
    json!({"limit": PAGE_LIMIT, "expand": true})
}

/// The order of the list, as the daemon sorts (`samplers/apps/mod.rs`,
/// `view`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Cpu,
    Mem,
    Name,
}

impl Sort {
    /// In the order of the page's sort control.
    pub const ALL: [Sort; 3] = [Sort::Cpu, Sort::Mem, Sort::Name];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Cpu => s::APPS_SORT_CPU,
            Sort::Mem => s::APPS_SORT_MEM,
            Sort::Name => s::APPS_SORT_NAME,
        }
    }

    pub fn index(self) -> u32 {
        Sort::ALL.iter().position(|x| *x == self).unwrap_or(0) as u32
    }

    pub fn from_index(i: u32) -> Sort {
        Sort::ALL.get(i as usize).copied().unwrap_or_default()
    }
}

/// A row's identity across frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A number the view model assigns (see [`Keys`]).
    Group(u64),
    /// A process: its pid and start time.
    Process { pid: u32, start: u64 },
}

struct Known {
    id: String,
    key: u64,
    members: Members,
}

/// A group's processes by identity, or by pid alone when the frame did
/// not list them (a frame cut for a consumer that did not expand, which
/// can arrive while the page's subscription replaces it).
struct Members {
    exact: bool,
    set: HashSet<(u32, u64)>,
}

impl Members {
    fn of(g: &AppGroup) -> Members {
        match &g.processes {
            Some(ps) => Members {
                exact: true,
                set: ps.iter().map(|p| (p.pid, p.start)).collect(),
            },
            None => Members {
                exact: false,
                set: g.pids.iter().map(|&p| (p, 0)).collect(),
            },
        }
    }

    /// At least one process in common: by pid and start when both sides
    /// know the starts, by pid otherwise.
    fn share(&self, other: &Members) -> bool {
        if self.exact && other.exact {
            !self.set.is_disjoint(&other.set)
        } else {
            self.set
                .iter()
                .any(|(p, _)| other.set.iter().any(|(q, _)| p == q))
        }
    }
}

/// Group keys across frames (D5). A group keeps the key of the previous
/// frame's group with the same id only while the two share a process
/// (`pid`, `start`); otherwise it is a new row. The System group always
/// keeps its key.
#[derive(Default)]
pub struct Keys {
    next: u64,
    system: Option<u64>,
    prev: Vec<Known>,
}

impl Keys {
    fn fresh(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// One key per group of `groups`, in their order.
    pub fn assign(&mut self, groups: &[AppGroup]) -> Vec<u64> {
        let mut out = Vec::with_capacity(groups.len());
        let mut known = Vec::with_capacity(groups.len());
        let mut taken: HashSet<u64> = HashSet::new();
        for g in groups {
            let m = Members::of(g);
            let key = if g.id == SYSTEM_ID {
                match self.system {
                    Some(k) => k,
                    None => {
                        let k = self.fresh();
                        self.system = Some(k);
                        k
                    }
                }
            } else {
                let kept = self
                    .prev
                    .iter()
                    .find(|k| k.id == g.id && !taken.contains(&k.key) && k.members.share(&m))
                    .map(|k| k.key);
                match kept {
                    Some(k) => k,
                    None => self.fresh(),
                }
            };
            taken.insert(key);
            known.push(Known {
                id: g.id.clone(),
                key,
                members: m,
            });
            out.push(key);
        }
        self.prev = known;
        out
    }
}

/// One `apps` frame with its row keys.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// In the daemon's order, each with its key.
    pub groups: Vec<(u64, AppGroup)>,
    /// Groups the daemon's limit cut.
    pub more: u32,
}

impl Frame {
    pub fn group(&self, key: u64) -> Option<&AppGroup> {
        self.groups.iter().find(|(k, _)| *k == key).map(|(_, g)| g)
    }

    /// A process by identity, with its group's key and the group.
    pub fn process(&self, pid: u32, start: u64) -> Option<(u64, &AppGroup, &AppProcess)> {
        self.groups.iter().find_map(|(k, g)| {
            g.processes
                .iter()
                .flatten()
                .find(|p| p.pid == pid && p.start == start)
                .map(|p| (*k, g, p))
        })
    }

    pub fn has(&self, key: Key) -> bool {
        match key {
            Key::Group(k) => self.group(k).is_some(),
            Key::Process { pid, start } => self.process(pid, start).is_some(),
        }
    }

    /// Whether any of `targets` is still listed.
    pub fn any_listed(&self, targets: &[(u32, u64)]) -> bool {
        targets
            .iter()
            .any(|&(pid, start)| self.process(pid, start).is_some())
    }
}

/// Whether a frame lists processes: one cut for a consumer that did not
/// expand (the card's, in flight while the page's subscription replaces
/// it) does not, and the page skips it rather than lose a process row's
/// selection to it.
pub fn expanded(d: &AppsData) -> bool {
    d.groups.is_empty() || d.groups.iter().any(|g| g.processes.is_some())
}

/// What the page remembers while the interface runs (decision 13): sort,
/// the flatten switch, open groups and the selection. They live on the
/// interface's context, so they survive the page being dropped on hide.
#[derive(Default)]
pub struct AppsState {
    pub sort: Sort,
    pub flatten: bool,
    pub open: HashSet<u64>,
    pub selected: Option<Key>,
    /// A group the `apps.top` card asked to open, by id; resolved on the
    /// page's next frame.
    pub pending_group: Option<String>,
    keys: Keys,
}

impl AppsState {
    /// Takes a frame: keys assigned, open groups and the selection kept
    /// only while their rows exist, a pending group selected. The flag is
    /// true when the selection was lost (its row is gone).
    pub fn take(&mut self, data: AppsData) -> (Frame, bool) {
        let keys = self.keys.assign(&data.groups);
        let more = data.totals.groups.saturating_sub(data.groups.len() as u32);
        let frame = Frame {
            groups: keys.into_iter().zip(data.groups).collect(),
            more,
        };
        let present: HashSet<u64> = frame.groups.iter().map(|(k, _)| *k).collect();
        self.open.retain(|k| present.contains(k));
        if let Some(id) = self.pending_group.take()
            && let Some((k, _)) = frame.groups.iter().find(|(_, g)| g.id == id)
        {
            self.selected = Some(Key::Group(*k));
        }
        let lost = match self.selected {
            Some(k) if !frame.has(k) => {
                self.selected = None;
                true
            }
            _ => false,
        };
        (frame, lost)
    }

    /// Opens or closes a group's processes.
    pub fn toggle_open(&mut self, key: u64) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
    }
}

/// Names compared as the daemon compares them: lower-cased, then as they
/// are.
fn by_name(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b))
}

fn cpu(c: Option<f64>) -> f64 {
    c.unwrap_or(0.0)
}

fn group_order(sort: Sort, a: &AppGroup, b: &AppGroup) -> Ordering {
    match sort {
        Sort::Mem => b
            .rss_bytes
            .cmp(&a.rss_bytes)
            .then(by_name(&a.name, &b.name)),
        Sort::Name => by_name(&a.name, &b.name).then(a.id.cmp(&b.id)),
        Sort::Cpu => cpu(b.cpu)
            .total_cmp(&cpu(a.cpu))
            .then(by_name(&a.name, &b.name)),
    }
}

fn process_order(sort: Sort, a: &AppProcess, b: &AppProcess) -> Ordering {
    match sort {
        Sort::Mem => b.rss_bytes.cmp(&a.rss_bytes).then(a.pid.cmp(&b.pid)),
        Sort::Name => by_name(&a.name, &b.name).then(a.pid.cmp(&b.pid)),
        Sort::Cpu => cpu(b.cpu).total_cmp(&cpu(a.cpu)).then(a.pid.cmp(&b.pid)),
    }
}

/// The groups in `sort` order, the System group last.
pub fn sorted(frame: &Frame, sort: Sort) -> Vec<&(u64, AppGroup)> {
    let (mut system, mut groups): (Vec<_>, Vec<_>) =
        frame.groups.iter().partition(|(_, g)| g.id == SYSTEM_ID);
    groups.sort_by(|a, b| group_order(sort, &a.1, &b.1));
    groups.append(&mut system);
    groups
}

/// A group's processes in `sort` order.
pub fn sorted_processes(g: &AppGroup, sort: Sort) -> Vec<&AppProcess> {
    let mut ps: Vec<&AppProcess> = g.processes.iter().flatten().collect();
    ps.sort_by(|a, b| process_order(sort, a, b));
    ps
}

/// The glyph of a group's kind, for a row whose app has no theme icon.
pub fn glyph(kind: &str) -> Glyph {
    match kind {
        "app" => icons::KIND_APP,
        "service" => icons::KIND_SERVICE,
        "terminal" => icons::KIND_TERMINAL,
        "container" => icons::KIND_CONTAINER,
        "flatpak" => icons::KIND_FLATPAK,
        "system" => icons::KIND_SYSTEM,
        _ => icons::KIND_PROCESS,
    }
}

/// The desktop entry's `Icon=` when it is a theme name. A value with a
/// path in it is never used: the interface reads no file the daemon's
/// data names (decision 8).
pub fn icon_name(icon: Option<&str>) -> Option<String> {
    icon.filter(|i| !i.is_empty() && !i.contains('/') && !i.chars().any(char::is_control))
        .map(str::to_string)
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| s::NO_VALUE.to_string(), format::percent)
}

/// The meter's fraction: full at 100 % of one core.
fn fraction(v: Option<f64>) -> f64 {
    (cpu(v) / 100.0).clamp(0.0, 1.0)
}

/// A group's row.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupLine {
    pub key: u64,
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub glyph: Glyph,
    pub cpu: String,
    pub fraction: f64,
    pub memory: String,
    pub procs: String,
    /// More than one process: the expander shows.
    pub expandable: bool,
    pub open: bool,
    pub selected: bool,
    /// For assistive technology.
    pub label: String,
}

/// A process's row, under its group or in the flat list.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessLine {
    pub group: u64,
    pub pid: u32,
    pub start: u64,
    pub name: String,
    pub pid_text: String,
    pub cpu: String,
    pub fraction: f64,
    pub memory: String,
    /// Shown when it is neither running nor sleeping.
    pub state: Option<String>,
    /// Its group's name, in the flat list only.
    pub group_name: Option<String>,
    pub selected: bool,
    pub label: String,
}

/// One row of the list, in display order.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Group(GroupLine),
    Process(ProcessLine),
    /// A line of text: the groups the limit cut, or nothing found.
    Note(String),
}

fn group_line(key: u64, g: &AppGroup, open: bool, selected: bool) -> GroupLine {
    let name = s::plain(&g.name).into_owned();
    let cpu = pct(g.cpu);
    let memory = format::bytes(g.rss_bytes);
    GroupLine {
        key,
        id: g.id.clone(),
        label: s::apps_group_label(&name, &cpu, &memory, g.procs),
        icon: icon_name(g.icon.as_deref()),
        glyph: glyph(&g.kind),
        fraction: fraction(g.cpu),
        procs: s::apps_procs(g.procs),
        expandable: g.procs > 1,
        name,
        cpu,
        memory,
        open,
        selected,
    }
}

fn process_line(
    group: u64,
    p: &AppProcess,
    group_name: Option<&str>,
    selected: Option<Key>,
) -> ProcessLine {
    let name = s::plain(&p.name).into_owned();
    let cpu = pct(p.cpu);
    let memory = format::bytes(p.rss_bytes);
    ProcessLine {
        group,
        pid: p.pid,
        start: p.start,
        label: s::apps_process_label(&name, p.pid, &cpu, &memory),
        pid_text: s::apps_pid(p.pid),
        fraction: fraction(p.cpu),
        state: (!matches!(p.state.as_str(), "S" | "R" | "")).then(|| s::apps_state(&p.state)),
        group_name: group_name.map(s::apps_in_group),
        selected: selected
            == Some(Key::Process {
                pid: p.pid,
                start: p.start,
            }),
        name,
        cpu,
        memory,
    }
}

fn hit(q: &str, text: &str) -> bool {
    text.to_lowercase().contains(q)
}

fn process_hit(q: &str, p: &AppProcess) -> bool {
    hit(q, &p.name) || p.pid.to_string().starts_with(q)
}

/// The rows of the list for `search`: groups in the state's order, an
/// open group's processes after it; with the flatten switch, process rows
/// only, each with its group's name. A group whose name does not match
/// but some of whose processes do is listed with those processes under
/// it. The System group and its processes come last.
pub fn rows(frame: &Frame, state: &AppsState, search: &str) -> Vec<Row> {
    let q = search.trim().to_lowercase();
    let mut out = Vec::new();
    let groups = sorted(frame, state.sort);
    if state.flatten {
        let (mut system, mut rest): (Vec<_>, Vec<_>) = (Vec::new(), Vec::new());
        for (k, g) in &groups {
            let group_hit = q.is_empty() || hit(&q, &g.name);
            for p in g.processes.iter().flatten() {
                if group_hit || process_hit(&q, p) {
                    let to = if g.id == SYSTEM_ID {
                        &mut system
                    } else {
                        &mut rest
                    };
                    to.push((*k, g, p));
                }
            }
        }
        for list in [&mut rest, &mut system] {
            list.sort_by(|a, b| process_order(state.sort, a.2, b.2));
            out.extend(
                list.iter().map(|(k, g, p)| {
                    Row::Process(process_line(*k, p, Some(&g.name), state.selected))
                }),
            );
        }
    } else {
        for (k, g) in &groups {
            let group_hit = q.is_empty() || hit(&q, &g.name);
            let procs: Vec<&AppProcess> = sorted_processes(g, state.sort)
                .into_iter()
                .filter(|p| group_hit || process_hit(&q, p))
                .collect();
            if !group_hit && procs.is_empty() {
                continue;
            }
            let open = state.open.contains(k) || !group_hit;
            out.push(Row::Group(group_line(
                *k,
                g,
                open,
                state.selected == Some(Key::Group(*k)),
            )));
            if (open && g.procs > 1) || !group_hit {
                out.extend(
                    procs
                        .into_iter()
                        .map(|p| Row::Process(process_line(*k, p, None, state.selected))),
                );
            }
        }
    }
    if frame.more > 0 && q.is_empty() {
        out.push(Row::Note(s::apps_more(frame.more)));
    }
    if out.is_empty() && !q.is_empty() {
        out.push(Row::Note(s::APPS_EMPTY.to_string()));
    }
    out
}

/// A group's main process: the member whose parent is not a member, the
/// lowest pid on a tie (decision 10). The inspector explains it.
pub fn main_pid(g: &AppGroup) -> Option<u32> {
    let ps = g.processes.as_deref().unwrap_or_default();
    let pids: HashSet<u32> = ps.iter().map(|p| p.pid).collect();
    ps.iter()
        .filter(|p| !pids.contains(&p.ppid))
        .map(|p| p.pid)
        .min()
        .or_else(|| ps.iter().map(|p| p.pid).min())
        .or_else(|| g.pids.iter().copied().min())
}

/// One row of the `apps.top` card.
#[derive(Debug, Clone, PartialEq)]
pub struct TopLine {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub glyph: Glyph,
    pub cpu: String,
    pub fraction: f64,
    pub label: String,
}

/// The five groups using the most CPU, System included when it is among
/// them: a root daemon eating a core must not be hidden (decision 12).
pub fn top_five(groups: &[AppGroup]) -> Vec<TopLine> {
    let mut all: Vec<&AppGroup> = groups.iter().collect();
    all.sort_by(|a, b| group_order(Sort::Cpu, a, b));
    all.into_iter()
        .take(TOP)
        .map(|g| {
            let line = group_line(0, g, false, false);
            TopLine {
                id: g.id.clone(),
                name: line.name,
                icon: line.icon,
                glyph: line.glyph,
                cpu: line.cpu,
                fraction: line.fraction,
                label: line.label,
            }
        })
        .collect()
}

/// The `apps` params of several consumers merged into one subscription
/// (decision 2): the largest `limit` (absent is the daemon's 50); `expand`
/// is `true` if any asks `true`, otherwise the union of the id lists;
/// `sort` is never sent, as each consumer sorts for itself.
pub fn merge_params(all: &[Value]) -> Value {
    let limit = all
        .iter()
        .map(|p| {
            p.get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_LIMIT)
        })
        .max()
        .unwrap_or(DEFAULT_LIMIT);
    let mut out = Map::new();
    out.insert("limit".into(), json!(limit));
    if all
        .iter()
        .any(|p| p.get("expand").and_then(Value::as_bool) == Some(true))
    {
        out.insert("expand".into(), json!(true));
    } else {
        let mut ids: Vec<&str> = Vec::new();
        for id in all
            .iter()
            .filter_map(|p| p.get("expand").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if !ids.is_empty() {
            out.insert("expand".into(), json!(ids));
        }
    }
    Value::Object(out)
}

/// What the inspector shows of the selected row, from the frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub key: Key,
    pub name: String,
    pub kind: &'static str,
    pub icon: Option<String>,
    pub glyph: Glyph,
    pub cpu: Option<f64>,
    pub cpu_text: String,
    pub memory: String,
    pub threads: String,
    pub procs: String,
    /// The pid `apps.inspect` explains: the process, or a group's main
    /// process.
    pub inspect_pid: Option<u32>,
    /// The `args` of `apps.quit` and `apps.kill`.
    pub args: Value,
    /// A process row's pid and start time: the live graph's params, and
    /// what a plan's entry must match (D5).
    pub process: Option<(u32, u64)>,
    /// The processes a Quit would end, by identity.
    pub targets: Vec<(u32, u64)>,
    /// Another user's: no buttons (decision 11).
    pub not_yours: bool,
    /// The interface itself is among the targets.
    pub holds_self: bool,
}

/// The selection's view, or None when its row is gone.
pub fn selection(frame: &Frame, key: Key, own_pid: u32) -> Option<Selection> {
    match key {
        Key::Group(k) => {
            let g = frame.group(k)?;
            let targets: Vec<(u32, u64)> = match &g.processes {
                Some(ps) => ps.iter().map(|p| (p.pid, p.start)).collect(),
                None => g.pids.iter().map(|&p| (p, 0)).collect(),
            };
            Some(Selection {
                key,
                name: s::plain(&g.name).into_owned(),
                kind: s::apps_kind(&g.kind),
                icon: icon_name(g.icon.as_deref()),
                glyph: glyph(&g.kind),
                cpu: g.cpu,
                cpu_text: pct(g.cpu),
                memory: format::bytes(g.rss_bytes),
                threads: g.threads.to_string(),
                procs: s::apps_procs(g.procs),
                inspect_pid: main_pid(g),
                args: json!({"group": g.id}),
                process: None,
                holds_self: targets.iter().any(|&(p, _)| p == own_pid),
                targets,
                not_yours: g.id == SYSTEM_ID,
            })
        }
        Key::Process { pid, start } => {
            let (_, g, p) = frame.process(pid, start)?;
            Some(Selection {
                key,
                name: s::plain(&p.name).into_owned(),
                kind: s::apps_kind("process"),
                icon: None,
                glyph: icons::KIND_PROCESS,
                cpu: p.cpu,
                cpu_text: pct(p.cpu),
                memory: format::bytes(p.rss_bytes),
                threads: p.threads.to_string(),
                procs: s::apps_procs(1),
                inspect_pid: Some(pid),
                args: json!({"pid": pid}),
                process: Some((pid, start)),
                targets: vec![(pid, start)],
                not_yours: g.id == SYSTEM_ID,
                holds_self: pid == own_pid,
            })
        }
    }
}

/// A dependent unit that is no service, scope or slice (a target) takes
/// its own dependents with it, unlisted (review of PR #18, S3).
pub fn deeper(unit: &str) -> bool {
    ![".service", ".scope", ".slice"]
        .iter()
        .any(|t| unit.ends_with(t))
}

fn pids_of(v: &Value) -> Vec<u32> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_u64().map(|p| p as u32))
        .collect()
}

/// The confirmation's lines for a `needs-confirm` reply's data (decision
/// 11): one per process signalled, a service's stop in words with its
/// pids and what it takes with it, the processes left alone with their
/// reasons, then the daemon's warnings.
pub fn confirm_lines(data: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for w in data["wouldDo"].as_array().into_iter().flatten() {
        if let Some(argv) = w["run"].as_array() {
            let unit = w["unit"].as_str().unwrap_or("");
            let kill = argv.iter().any(|a| a.as_str() == Some("kill"));
            if kill {
                out.push(s::apps_confirm_unit_kill(unit));
                continue;
            }
            let pids = pids_of(&w["pids"]);
            out.push(s::apps_confirm_stop(unit, pids.len()));
            if !pids.is_empty() {
                out.push(s::apps_confirm_pids(&pids));
            }
            for d in w["alsoStops"].as_array().into_iter().flatten() {
                let du = d["unit"].as_str().unwrap_or("");
                let dp = pids_of(&d["pids"]);
                out.push(s::apps_confirm_also_stops(du, dp.len(), deeper(du)));
                if !dp.is_empty() {
                    out.push(format!("  {}", s::apps_confirm_pids(&dp)));
                }
            }
            if w["alsoStopsUnknown"] == Value::Bool(true) {
                out.push(format!("  {}", s::APPS_ALSO_STOPS_UNKNOWN));
            }
        } else {
            out.push(s::apps_confirm_signal(
                w["name"].as_str().unwrap_or(""),
                w["pid"].as_u64().unwrap_or(0) as u32,
                w["via"].as_u64().map(|v| v as u32),
            ));
        }
    }
    out.extend(left_alone(&data["skipped"]));
    for w in data["warnings"].as_array().into_iter().flatten() {
        out.push(s::apps_warning(w.as_str().unwrap_or("")));
    }
    out
}

fn left_alone(skipped: &Value) -> Vec<String> {
    let list = skipped.as_array().map_or(&[][..], Vec::as_slice);
    if list.is_empty() {
        return Vec::new();
    }
    let mut out = vec![s::APPS_LEFT_ALONE.to_string()];
    out.extend(list.iter().map(|e| {
        s::apps_left_alone_line(
            e["pid"].as_u64().unwrap_or(0) as u32,
            e["name"].as_str().unwrap_or(""),
            e["reason"].as_str().unwrap_or(""),
        )
    }));
    out
}

/// Whether a plan names the row's pid with another start time: the row's
/// process exited and the pid was reused (D5). Then nothing is asked.
pub fn start_differs(data: &Value, pid: u32, start: u64) -> bool {
    data["wouldDo"].as_array().into_iter().flatten().any(|w| {
        w["pid"].as_u64() == Some(u64::from(pid))
            && w["start"].as_u64().is_some_and(|st| st != start)
    })
}

/// The lines of a confirmed reply (decision 11): done, already gone, or
/// each failure with its error; then anything left alone.
pub fn result_lines(v: &Value) -> Vec<String> {
    let count = |k: &str| v[k].as_array().map_or(0, Vec::len);
    let mut out = Vec::new();
    let failed = v["failed"].as_array().map_or(&[][..], Vec::as_slice);
    if count("done") > 0 && failed.is_empty() {
        out.push(s::APPS_DONE.to_string());
    } else if count("done") == 0 && count("gone") > 0 && failed.is_empty() {
        out.push(s::APPS_ALREADY_GONE.to_string());
    }
    for f in failed {
        let what = match f["run"].as_array() {
            Some(argv) => argv
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
            None => s::apps_confirm_signal(
                f["name"].as_str().unwrap_or(""),
                f["pid"].as_u64().unwrap_or(0) as u32,
                None,
            ),
        };
        out.push(s::quit_failed_line(
            &what,
            f["error"].as_str().unwrap_or(""),
        ));
    }
    out.extend(left_alone(&v["skipped"]));
    out
}

/// The act ran but its row in the history could not be written.
pub fn not_recorded(v: &Value) -> bool {
    v["recorded"] == Value::Bool(false)
}

/// The inspector's labelled lines, each only when the daemon could read
/// it; `private` adds the working directory, the command line and the
/// open files' paths, which the MCP tool leaves out. Every value is
/// made `plain`, as a process names itself. `mirqab what` prints the same
/// lines.
pub fn inspect_rows(i: &Inspect, private: bool) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let mut line = |label: &'static str, value: String| {
        out.push((label, s::plain(&value).into_owned()));
    };
    line(s::WHAT_PROCESS, s::proc_ref(&i.name, i.pid));
    if !i.parents.is_empty() {
        let chain: Vec<String> = i
            .parents
            .iter()
            .map(|p| s::proc_ref(&p.name, p.pid))
            .collect();
        line(s::WHAT_PARENTS, chain.join(", "));
    }
    match (&i.user, i.uid) {
        (Some(u), Some(uid)) => line(s::WHAT_USER, s::proc_ref(u, uid)),
        (None, Some(uid)) => line(s::WHAT_USER, uid.to_string()),
        _ => {}
    }
    if let Some(age) = i.age_sec {
        line(s::WHAT_STARTED, s::started_ago(&format::duration(age)));
    }
    if let Some(t) = i.cpu_time_sec {
        line(s::WHAT_CPU_TIME, format::duration(t as u64));
    }
    if let Some(t) = i.threads {
        line(s::WHAT_THREADS, t.to_string());
    }
    if let Some(st) = &i.state {
        line(s::WHAT_STATE, s::state_word(st).to_string());
    }
    if let Some(e) = &i.exe {
        let shown = if i.exe_deleted {
            s::exe_replaced(e)
        } else {
            e.clone()
        };
        line(s::WHAT_EXE, shown);
    }
    if let Some(p) = &i.package {
        line(s::WHAT_PACKAGE, s::package_line(&p.name, p.repo.as_deref()));
    }
    if let Some(a) = &i.arch {
        line(s::WHAT_ARCH, s::arch_line(&a.machine, a.bits, a.foreign));
    }
    if let Some(u) = &i.unit {
        line(s::WHAT_UNIT, u.clone());
    }
    if let Some(g) = &i.group {
        line(s::WHAT_GROUP, g.name.clone());
    }
    if let Some(f) = &i.flatpak {
        line(s::WHAT_FLATPAK, f.clone());
    }
    if let Some(c) = &i.container {
        line(s::WHAT_CONTAINER, c.clone());
    }
    if let Some(sb) = &i.sandbox {
        line(
            s::WHAT_SANDBOX,
            s::sandbox_line(
                sb.seccomp.as_deref(),
                sb.no_new_privs,
                &sb.capabilities,
                &sb.namespaces,
            ),
        );
    }
    if let Some(m) = &i.memory {
        let pss = m.pss_bytes.map(format::bytes);
        let swap = m.swap_bytes.map(format::bytes);
        line(
            s::WHAT_MEMORY,
            s::memory_line(&format::bytes(m.rss_bytes), pss.as_deref(), swap.as_deref()),
        );
    }
    if let Some(f) = &i.files {
        line(s::WHAT_FILES, s::files_line(f.count, f.sockets));
    }
    if let Some(g) = &i.gpu {
        let (v, t, c) = (
            g.vram_bytes.map(format::bytes),
            g.gtt_bytes.map(format::bytes),
            g.cpu_bytes.map(format::bytes),
        );
        line(
            s::WHAT_GPU,
            s::gpu_memory_line(v.as_deref(), t.as_deref(), c.as_deref()),
        );
    }
    if private {
        if let Some(d) = &i.cwd {
            line(s::WHAT_CWD, d.clone());
        }
        if !i.cmdline.is_empty() {
            line(s::WHAT_COMMAND, i.cmdline.join(" "));
        }
        if let Some(f) = &i.files {
            for p in &f.paths {
                line(s::WHAT_OPEN, p.clone());
            }
        }
    }
    out
}
