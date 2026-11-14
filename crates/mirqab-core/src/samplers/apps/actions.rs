//! Quit and Force quit (plan 006, part 2): `apps.quit` sends SIGTERM and
//! `apps.kill` SIGKILL, to one process (`{"pid": N}`) or to an app group
//! (`{"group": "<id>"}`) of the `apps` stream.
//!
//! Two calls, always (architecture §5.4, plan 005 rule 1). The first, or
//! any call without `confirm: true` and a plan, resolves the targets and
//! replies `needs-confirm` with a plan id and the `wouldDo` list; it never
//! acts. The second, with `confirm: true` and the plan, resolves the
//! targets again from fresh reads and acts on the planned ones still there
//! (`{pid, start}`, so a reused pid is another process); one that joined
//! since is skipped. `dryRun: true` lists and plans nothing.
//!
//! Each resolution walks `proc/` afresh with an empty cache, so uid and
//! cgroup are read now, never taken from the sampler's per-process cache,
//! and no sampler lock is held: signals and `systemctl` run while nothing
//! else waits on them. Group ids do not depend on desktop entries, so the
//! walk groups without them.
//!
//! Refused (decision 5, checked here and again inside `act::signal` just
//! before each signal): pid 1, the daemon, another uid on any of the four
//! `Uid:` numbers, kernel threads, anything outside the user's slice, the
//! user manager, the compositor and its parents, the caller and its
//! parents. A group leaves its refused members out and lists them under
//! `skipped`; it is refused only when nothing is left. A user service is
//! stopped with `systemctl --user stop --no-block`, so it does not
//! restart; a unit that holds a refused process is not stopped.

use super::desktop::Desktop;
use super::group::{self, Kind};
use super::procfs::{self, Cache, Proc};
use crate::act::{self, ActError, Facts, Guard, Sender, Signal, Target};
use crate::guard;
use crate::history::History;
use crate::plan::{PlanError, Plans};
use crate::proto::stream::{APPS_KILL, APPS_QUIT};
use crate::proto::{Code, strings};
use crate::root::Root;
use crate::sys;
use serde_json::{Value, json};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long one `systemctl` may take. `--no-block` returns once the job is
/// queued, well inside the CLI's 10 s read timeout.
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(5);

/// Compositors by command name: what a fixture can show, and the fallback
/// on a host where the socket probe finds nothing (decision 6).
pub const COMPOSITORS: &[&str] = &[
    "Hyprland",
    "sway",
    "niri",
    "river",
    "labwc",
    "wayfire",
    "kwin_wayland",
    "gnome-shell",
    "cosmic-comp",
    "weston",
];

/// Quit (SIGTERM) or Force quit (SIGKILL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Kill,
}

impl Action {
    pub fn from_name(name: &str) -> Option<Action> {
        match name {
            APPS_QUIT => Some(Action::Quit),
            APPS_KILL => Some(Action::Kill),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::Quit => APPS_QUIT,
            Action::Kill => APPS_KILL,
        }
    }

    fn signal(self) -> Signal {
        match self {
            Action::Quit => Signal::Term,
            Action::Kill => Signal::Kill,
        }
    }
}

/// What an action is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Pid(u32),
    Group(String),
}

impl Subject {
    /// `{"pid": N}` or `{"group": "<id>"}`, exactly one key; anything else
    /// is a bad request.
    pub fn parse(args: &Value) -> Option<Subject> {
        let o = args.as_object().filter(|o| o.len() == 1)?;
        if let Some(p) = o.get("pid") {
            return p
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .filter(|p| *p > 0)
                .map(Subject::Pid);
        }
        o.get("group")
            .and_then(Value::as_str)
            .filter(|g| !g.is_empty())
            .map(|g| Subject::Group(g.to_string()))
    }

    pub fn to_value(&self) -> Value {
        match self {
            Subject::Pid(p) => json!({ "pid": p }),
            Subject::Group(g) => json!({ "group": g }),
        }
    }
}

/// One request, parsed strictly.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub action: Action,
    pub subject: Subject,
    pub dry_run: bool,
    /// `confirm: true` with this plan id: act.
    pub redeem: Option<String>,
    /// The process that asked (the socket's peer), whose ancestry is
    /// refused; None when it could not be told.
    pub caller: Option<u32>,
}

/// An error reply: its code, message and `data`.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: Code,
    pub message: String,
    pub data: Option<Value>,
}

impl Failure {
    fn new(code: Code, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Parses an `action` request's params for `apps.quit` or `apps.kill`
/// (decision 3): `args` exactly one of `pid` and `group`; `dryRun` and
/// `confirm` booleans or absent; `confirm: false` with a plan, or a plan
/// that is not a string, is a bad request.
pub fn parse(action: Action, params: &Value, caller: Option<u32>) -> Result<Ask, Failure> {
    let bad = |m: &str| Failure::new(Code::BadRequest, m);
    let subject = params
        .get("args")
        .and_then(Subject::parse)
        .ok_or_else(|| bad(strings::ACT_ARGS))?;
    let flag = |key: &str| match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(()),
    };
    let dry_run = flag("dryRun")
        .map_err(|()| bad(strings::ACT_DRY_RUN_BOOL))?
        .unwrap_or(false);
    let confirm = flag("confirm").map_err(|()| bad(strings::ACT_CONFIRM_BOOL))?;
    let plan = match params.get("plan") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(bad(strings::ACT_PLAN_STRING)),
    };
    if confirm == Some(false) && plan.is_some() {
        return Err(bad(strings::ACT_CONFIRM_FALSE));
    }
    Ok(Ask {
        action,
        subject,
        dry_run,
        redeem: plan.filter(|_| confirm == Some(true)),
        caller,
    })
}

/// One thing to do.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Signal {
        target: Target,
        name: String,
        /// The thread id asked for, when it was not the process's (D3).
        via: Option<u32>,
    },
    /// A user service: `systemctl --user stop` (and for Force quit `kill`).
    Unit {
        unit: String,
        procs: Vec<(Target, String)>,
    },
}

/// A process left alone, and why.
#[derive(Debug, Clone, PartialEq)]
struct Skip {
    pid: u32,
    name: String,
    reason: String,
}

impl Skip {
    fn to_value(&self) -> Value {
        json!({"pid": self.pid, "name": self.name, "reason": self.reason})
    }
}

/// What a subject resolves to now.
struct Resolution {
    steps: Vec<Step>,
    skipped: Vec<Skip>,
    guard: Guard,
}

impl Resolution {
    fn targets(&self) -> Vec<Target> {
        let mut out = Vec::new();
        for s in &self.steps {
            match s {
                Step::Signal { target, .. } => out.push(*target),
                Step::Unit { procs, .. } => out.extend(procs.iter().map(|(t, _)| *t)),
            }
        }
        out
    }

    fn would_do(&self, action: Action) -> Vec<Value> {
        let mut out = Vec::new();
        for s in &self.steps {
            match s {
                Step::Signal { target, name, via } => {
                    let mut e = json!({
                        "signal": action.signal().name(),
                        "pid": target.pid,
                        "start": target.start,
                        "name": name,
                    });
                    if let Some(t) = via {
                        e["via"] = json!(t);
                    }
                    out.push(e);
                }
                Step::Unit { unit, procs } => {
                    let pids: Vec<u32> = procs.iter().map(|(t, _)| t.pid).collect();
                    for argv in unit_commands(unit, action) {
                        out.push(json!({"run": argv, "unit": unit, "pids": pids}));
                    }
                }
            }
        }
        out
    }

    fn skipped(&self) -> Vec<Value> {
        self.skipped.iter().map(Skip::to_value).collect()
    }
}

/// The commands that stop a user unit: `stop --no-block`, so a unit with
/// `Restart=` does not come back and the call does not wait out
/// `TimeoutStopSec`; for Force quit, then `kill --signal=KILL` on what is
/// left (decision 9).
fn unit_commands(unit: &str, action: Action) -> Vec<Vec<String>> {
    let argv = |words: &[&str]| -> Vec<String> {
        words
            .iter()
            .map(|w| w.to_string())
            .chain([unit.to_string()])
            .collect()
    };
    let mut out = vec![argv(&["systemctl", "--user", "stop", "--no-block", "--"])];
    if action == Action::Kill {
        out.push(argv(&[
            "systemctl",
            "--user",
            "kill",
            "--signal=KILL",
            "--",
        ]));
    }
    out
}

/// A unit name mirqab passes to systemctl: `[A-Za-z0-9:_.@\-]+\.service`.
fn unit_name_ok(unit: &str) -> bool {
    unit.strip_suffix(".service").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ":_.@\\-".contains(c))
    })
}

fn facts_of(p: &Proc) -> Facts {
    Facts {
        comm: p.stat.comm.clone(),
        start: p.stat.start,
        flags: p.stat.flags,
        sid: p.stat.sid,
        pgrp: p.stat.pgrp,
    }
}

/// The user's uid as the root says (`proc/self/status`), so a fixture
/// says whose processes are the user's, as in the sampler.
fn own_uid(root: &Root) -> u32 {
    procfs::read_text(root, "proc/self/status")
        .and_then(|t| procfs::status(&t).uid)
        .unwrap_or_else(sys::uid)
}

/// The compositor's pids on the live machine: the listener of each
/// `wayland-*` socket in the runtime directory, by `SO_PEERCRED`. Read at
/// plan time and again at redeem; never polled.
fn compositor_sockets() -> Vec<u32> {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("wayland-") || name.ends_with(".lock") {
            continue;
        }
        if !e.file_type().is_ok_and(|t| t.is_socket()) {
            continue;
        }
        if let Some(pid) = UnixStream::connect(e.path())
            .ok()
            .and_then(|s| sys::peer_pid(&s).ok())
            && !out.contains(&pid)
        {
            out.push(pid);
        }
    }
    out
}

/// The guard for one resolution: the daemon, the compositor and its
/// parents, the caller and its parents. A walk that cannot finish refuses
/// the action (decisions 6, 7).
fn make_guard(
    root: &Root,
    procs: &[Proc],
    own_uid: u32,
    caller: Option<u32>,
) -> Result<Guard, Failure> {
    let refused_tree =
        |e: guard::WalkError| Failure::new(Code::Refused, strings::refuse_tree(&e.to_string()));
    let mut daemon = vec![std::process::id()];
    if let Some(st) = procfs::read_text(root, "proc/self/stat")
        .as_deref()
        .and_then(procfs::stat)
    {
        daemon.push(st.pid);
    }
    let mut compositors: Vec<u32> = procs
        .iter()
        .filter(|p| p.info.uid == Some(own_uid) && COMPOSITORS.contains(&p.stat.comm.as_str()))
        .map(Proc::pid)
        .collect();
    if root.is_host() {
        for pid in compositor_sockets() {
            if !compositors.contains(&pid) {
                compositors.push(pid);
            }
        }
    }
    let mut refused: Vec<(u32, String)> = Vec::new();
    for c in compositors {
        for (i, pid) in guard::ancestry_strict(root, c)
            .map_err(refused_tree)?
            .into_iter()
            .enumerate()
        {
            let why = if i == 0 {
                strings::refuse_compositor(pid)
            } else {
                strings::refuse_compositor_parent(pid)
            };
            refused.push((pid, why));
        }
    }
    let caller = caller.ok_or_else(|| Failure::new(Code::Refused, strings::REFUSE_NO_CALLER))?;
    for pid in guard::ancestry_strict(root, caller).map_err(refused_tree)? {
        refused.push((pid, strings::refuse_caller(pid)));
    }
    Ok(Guard {
        own_uid,
        daemon,
        refused,
    })
}

/// The `Tgid:` of `proc/<pid>/status`: the process a thread id belongs to.
fn tgid(root: &Root, pid: u32) -> Option<u32> {
    procfs::read_text(root, &format!("proc/{pid}/status"))?
        .lines()
        .find_map(|l| l.strip_prefix("Tgid:"))
        .and_then(|v| v.trim().parse().ok())
}

/// The members of a group in the order they are signalled (decision 8):
/// the main process, then the other session and process-group leaders,
/// then the rest, oldest first inside each class.
fn ordered(procs: &[Proc], members: &[usize]) -> Vec<usize> {
    let main = group::main_process(procs, members);
    let class = |i: usize| {
        let s = &procs[i].stat;
        if i == main {
            0
        } else if s.pid == s.sid || s.pid == s.pgrp {
            1
        } else {
            2
        }
    };
    let mut out = members.to_vec();
    out.sort_by_key(|&i| (class(i), procs[i].stat.start, procs[i].pid()));
    out
}

/// A user service as one step: refused whole when its name is odd or any
/// of its processes is refused.
fn unit_step(root: &Root, guard: &Guard, procs: &[Proc], unit: &str) -> Result<Step, Failure> {
    if !unit_name_ok(unit) {
        return Err(Failure::new(Code::Refused, strings::refuse_unit_name(unit)));
    }
    let mut members: Vec<usize> = procs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.info.cgroup.as_deref().and_then(group::user_unit) == Some(unit))
        .map(|(i, _)| i)
        .collect();
    members = ordered(procs, &members);
    let mut out = Vec::new();
    for i in members {
        let p = &procs[i];
        if let Some(why) = act::refusal(root, guard, p.pid(), &facts_of(p)) {
            return Err(Failure::new(
                Code::Refused,
                strings::refuse_unit_holds(unit, &why),
            ));
        }
        out.push((
            Target {
                pid: p.pid(),
                start: p.stat.start,
            },
            p.stat.comm.clone(),
        ));
    }
    Ok(Step::Unit {
        unit: unit.to_string(),
        procs: out,
    })
}

/// The service unit a process is stopped through, when it is in one.
fn service_of(p: &Proc) -> Option<&str> {
    p.info
        .cgroup
        .as_deref()
        .and_then(group::user_unit)
        .filter(|u| u.ends_with(".service"))
}

/// What `subject` resolves to now, from a fresh walk.
fn resolve(root: &Root, subject: &Subject, caller: Option<u32>) -> Result<Resolution, Failure> {
    if let Subject::Pid(p) = subject
        && *p <= 1
    {
        return Err(Failure::new(Code::Refused, strings::refuse_init(*p)));
    }
    let uid = own_uid(root);
    let procs = procfs::walk(root, &mut Cache::default(), uid);
    let guard = make_guard(root, &procs, uid, caller)?;
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    match subject {
        Subject::Pid(asked) => {
            let asked = *asked;
            let no_process = || Failure::new(Code::Failed, strings::apps_no_process(asked));
            // D3: a thread id stands for its thread group.
            let pid = tgid(root, asked).unwrap_or(asked);
            if pid <= 1 {
                return Err(Failure::new(Code::Refused, strings::refuse_init(pid)));
            }
            let via = (pid != asked).then_some(asked);
            let p = procs
                .iter()
                .find(|p| p.pid() == pid)
                .ok_or_else(no_process)?;
            match service_of(p) {
                Some(unit) => steps.push(unit_step(root, &guard, &procs, unit)?),
                None => {
                    if let Some(why) = act::refusal(root, &guard, pid, &facts_of(p)) {
                        return Err(Failure::new(Code::Refused, why));
                    }
                    steps.push(Step::Signal {
                        target: Target {
                            pid,
                            start: p.stat.start,
                        },
                        name: p.stat.comm.clone(),
                        via,
                    });
                }
            }
        }
        Subject::Group(id) => {
            let groups = group::group(&procs, uid, &Desktop::default());
            let g = groups
                .iter()
                .find(|g| g.id == *id)
                .ok_or_else(|| Failure::new(Code::Failed, strings::apps_no_group(id)))?;
            if g.kind == Kind::Container
                && g.members.iter().any(|&i| procs[i].info.uid != Some(uid))
            {
                // D4: the user cannot signal a rootless container's
                // sub-uid processes; its runtime can stop it.
                let short = id.strip_prefix("container:").unwrap_or(id);
                let runtime = g.members.iter().find_map(|&i| {
                    procs[i]
                        .info
                        .cgroup
                        .as_deref()
                        .and_then(group::container_runtime)
                });
                return Err(Failure::new(
                    Code::Refused,
                    strings::refuse_container(short, runtime),
                ));
            }
            let unit = g.unit.as_deref().filter(|u| u.ends_with(".service"));
            if let (Kind::Service, Some(unit)) = (g.kind, unit) {
                steps.push(unit_step(root, &guard, &procs, unit)?);
            } else {
                for i in ordered(&procs, &g.members) {
                    let p = &procs[i];
                    match act::refusal(root, &guard, p.pid(), &facts_of(p)) {
                        Some(reason) => skipped.push(Skip {
                            pid: p.pid(),
                            name: p.stat.comm.clone(),
                            reason,
                        }),
                        None => steps.push(Step::Signal {
                            target: Target {
                                pid: p.pid(),
                                start: p.stat.start,
                            },
                            name: p.stat.comm.clone(),
                            via: None,
                        }),
                    }
                }
                if steps.is_empty() {
                    let first = skipped
                        .first()
                        .map(|s| s.reason.clone())
                        .unwrap_or_default();
                    let data =
                        json!({"skipped": skipped.iter().map(Skip::to_value).collect::<Vec<_>>()});
                    return Err(Failure {
                        code: Code::Refused,
                        message: strings::refuse_nothing_left(&first),
                        data: Some(data),
                    });
                }
            }
        }
    }
    Ok(Resolution {
        steps,
        skipped,
        guard,
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The daemon's actions on apps: the plans and the sender.
pub struct Actions {
    plans: Mutex<Plans>,
    sender: Arc<dyn Sender>,
}

impl Actions {
    pub fn new(sender: Arc<dyn Sender>) -> Self {
        Actions {
            plans: Mutex::new(Plans::new()),
            sender,
        }
    }

    /// How many plans wait for their confirmation.
    pub fn live_plans(&self) -> usize {
        lock(&self.plans).len()
    }

    /// Answers one parsed request: the `wouldDo` of a dry run, a
    /// `needs-confirm` failure with a new plan, or what was done. Each
    /// redeemed plan is written to `history` as one `action` event.
    pub fn handle(
        &self,
        root: &Root,
        ask: &Ask,
        history: Option<&Mutex<History>>,
    ) -> Result<Value, Failure> {
        let res = match resolve(root, &ask.subject, ask.caller) {
            Ok(r) => r,
            Err(f) => {
                // A plan offered with a request that is refused now is
                // spent all the same: it was for this subject.
                if let Some(id) = &ask.redeem {
                    let _ = lock(&self.plans).redeem(
                        id,
                        ask.action.name(),
                        &ask.subject.to_value(),
                        &[],
                        sys::boottime(),
                    );
                }
                return Err(f);
            }
        };
        if ask.dry_run {
            return Ok(json!({"wouldDo": res.would_do(ask.action), "skipped": res.skipped()}));
        }
        let Some(id) = &ask.redeem else {
            let plan = lock(&self.plans).make(
                ask.action.name(),
                ask.subject.to_value(),
                res.targets(),
                sys::boottime(),
            );
            return Err(Failure {
                code: Code::NeedsConfirm,
                message: strings::ACT_NEEDS_CONFIRM.to_string(),
                data: Some(json!({
                    "plan": plan,
                    "wouldDo": res.would_do(ask.action),
                    "skipped": res.skipped(),
                })),
            });
        };
        self.redeem(root, ask, id, res, history)
    }

    fn redeem(
        &self,
        root: &Root,
        ask: &Ask,
        id: &str,
        res: Resolution,
        history: Option<&Mutex<History>>,
    ) -> Result<Value, Failure> {
        let current = res.targets();
        let redeemed = lock(&self.plans)
            .redeem(
                id,
                ask.action.name(),
                &ask.subject.to_value(),
                &current,
                sys::boottime(),
            )
            .map_err(|e| match e {
                PlanError::Unknown => Failure::new(Code::BadRequest, strings::PLAN_UNKNOWN),
                PlanError::Expired => Failure::new(Code::BadRequest, strings::PLAN_EXPIRED),
                PlanError::OtherTool => Failure::new(Code::Refused, strings::PLAN_OTHER),
            })?;
        let sig = ask.action.signal();
        let mut done: Vec<Value> = Vec::new();
        let mut gone: Vec<Value> = redeemed
            .gone
            .iter()
            .map(|t| json!({"pid": t.pid, "start": t.start}))
            .collect();
        let mut failed: Vec<Value> = Vec::new();
        let mut skipped = res.skipped.clone();
        let mut results: Vec<Value> = Vec::new();
        let mut unit_name: Option<String> = None;
        for t in &redeemed.gone {
            results.push(json!({"pid": t.pid, "signal": sig.name(), "result": "gone"}));
        }
        for s in &res.skipped {
            results.push(
                json!({"pid": s.pid, "name": s.name, "signal": sig.name(), "result": "skipped"}),
            );
        }
        // The lock on the plans is released; nothing else is held while
        // signals go out and systemctl runs.
        for step in &res.steps {
            match step {
                Step::Signal { target, name, .. } => {
                    if !redeemed.kept.contains(target) {
                        let late = Skip {
                            pid: target.pid,
                            name: name.clone(),
                            reason: strings::SKIP_LATE.to_string(),
                        };
                        results.push(json!({"pid": target.pid, "name": name, "signal": sig.name(), "result": "skipped"}));
                        skipped.push(late);
                        continue;
                    }
                    let result =
                        act::signal(root, self.sender.as_ref(), &res.guard, *target, sig, false);
                    let word = match result {
                        Ok(entry) => {
                            done.push(entry);
                            "done"
                        }
                        Err(ActError::Gone(_)) => {
                            gone.push(
                                json!({"pid": target.pid, "start": target.start, "name": name}),
                            );
                            "gone"
                        }
                        Err(ActError::Refused(reason)) => {
                            skipped.push(Skip {
                                pid: target.pid,
                                name: name.clone(),
                                reason,
                            });
                            "skipped"
                        }
                        Err(ActError::Failed(why)) => {
                            failed.push(json!({"pid": target.pid, "name": name, "error": why}));
                            "failed"
                        }
                    };
                    results.push(json!({"pid": target.pid, "name": name, "signal": sig.name(), "result": word}));
                }
                Step::Unit { unit, procs } => {
                    // A unit is stopped whole once any process planned in it
                    // is still there; systemd ends what joined it since too.
                    if !procs.iter().any(|(t, _)| redeemed.kept.contains(t)) {
                        for (t, name) in procs {
                            skipped.push(Skip {
                                pid: t.pid,
                                name: name.clone(),
                                reason: strings::SKIP_LATE.to_string(),
                            });
                        }
                        continue;
                    }
                    unit_name = Some(unit.clone());
                    let mut ok = true;
                    for argv in unit_commands(unit, ask.action) {
                        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
                        let why = match self.sender.run(&words, SYSTEMCTL_TIMEOUT) {
                            Ok(out) if out.success() => None,
                            Ok(out) => Some(out.stderr.trim().to_string()),
                            Err(e) => Some(e),
                        };
                        match why {
                            None => done.push(json!({"run": argv, "unit": unit})),
                            Some(why) => {
                                failed.push(json!({"run": argv, "unit": unit, "error": why}));
                                ok = false;
                                break;
                            }
                        }
                    }
                    let word = if ok { "done" } else { "failed" };
                    for (t, name) in procs {
                        results.push(json!({"pid": t.pid, "name": name, "signal": sig.name(), "result": word}));
                    }
                }
            }
        }
        if let Some(h) = history {
            let caller = ask.caller.map(|pid| {
                let name = act::facts(root, pid).map(|f| f.comm);
                json!({"pid": pid, "name": name})
            });
            let mut data = json!({
                "action": ask.action.name(),
                "target": ask.subject.to_value(),
                "caller": caller,
                "results": results,
            });
            if let Some(u) = unit_name {
                data["unit"] = json!(u);
            }
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Err(e) = lock(h).add_event(ts, "action", &data) {
                crate::server::log(&format!("history: the action was not recorded: {e}"));
            }
        }
        Ok(json!({
            "done": done,
            "gone": gone,
            "failed": failed,
            "skipped": skipped.iter().map(Skip::to_value).collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_are_strict() {
        let p = |v: Value| parse(Action::Quit, &v, Some(1));
        assert_eq!(
            p(json!({"args": {"pid": 7}})).unwrap().subject,
            Subject::Pid(7)
        );
        assert_eq!(
            p(json!({"args": {"group": "scope:x"}})).unwrap().subject,
            Subject::Group("scope:x".into())
        );
        for bad in [
            json!({}),
            json!({"args": {"pid": 7, "group": "x"}}),
            json!({"args": {"pid": "7"}}),
            json!({"args": {"pid": 0}}),
            json!({"args": {"pid": 7, "force": true}}),
            json!({"args": {"group": ""}}),
            json!({"args": {"pid": 7}, "confirm": "true"}),
            json!({"args": {"pid": 7}, "confirm": 1}),
            json!({"args": {"pid": 7}, "confirm": false, "plan": "plan-1"}),
            json!({"args": {"pid": 7}, "plan": 3}),
            json!({"args": {"pid": 7}, "dryRun": "yes"}),
        ] {
            assert_eq!(p(bad.clone()).unwrap_err().code, Code::BadRequest, "{bad}");
        }
        let a = p(json!({"args": {"pid": 7}, "confirm": true, "plan": "plan-1"})).unwrap();
        assert_eq!(a.redeem.as_deref(), Some("plan-1"));
        let a = p(json!({"args": {"pid": 7}, "plan": "plan-1"})).unwrap();
        assert_eq!(a.redeem, None, "a plan without confirm: true does not act");
    }

    #[test]
    fn unit_names_are_checked() {
        assert!(unit_name_ok("app-org.example.Sync@autostart.service"));
        assert!(unit_name_ok("app-two\\x2dwords@1.service"));
        assert!(!unit_name_ok("odd name.service"));
        assert!(!unit_name_ok("x;y.service"));
        assert!(!unit_name_ok(".service"));
        assert!(!unit_name_ok("app-x.scope"));
        assert_eq!(
            unit_commands("x.service", Action::Kill)[1],
            [
                "systemctl",
                "--user",
                "kill",
                "--signal=KILL",
                "--",
                "x.service"
            ]
        );
    }
}
