//! Quit and Force quit on fixture roots (plan 006, part 2, "Tests"), with
//! the recording sender: nothing here signals or runs anything. The
//! refusals of architecture §5.4 and decision 5; `wouldDo` lists leaders
//! first with start times; the two-step flow; targets re-computed at the
//! second call; a group with a refused member; a rootless container; a
//! thread id; a user service; the `action` event; and a fixture root never
//! gets the real sender. Part 3: the units a stop takes with it
//! (`alsoStops`, from the fixture's answer), a dependent that holds a
//! refused process, a question that fails, the session bus refused and
//! the sound server's warning. Review of PR #18: a quoted, escaped name in
//! `systemctl show` found and checked; a dependent scope or slice checked;
//! a confirmation refused when the dependents changed. Part 4: a pid
//! asked with its start time is `failed` once it names another process
//! (S4), and a session bus outside any bus unit is refused by its command
//! name (owner, 2026-11-14, Q1). Audit: a confirmation refused when the
//! unit its target is stopped through is not the one the plan showed.

use mirqab_core::Root;
use mirqab_core::act::{self, Recording, Sender, Signal, Target};
use mirqab_core::exec;
use mirqab_core::history::History;
use mirqab_core::proto::Code;
use mirqab_core::samplers::apps::actions::{self, Action, Actions, Failure};
use serde_json::{Value, json};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The fixture's caller: `python3` in a foot terminal, under zsh (3050),
/// foot (3000), Hyprland (1050) and login (900).
const CALLER: u32 = 3060;
const BROWSER: &str = "scope:app-Hyprland-browser.sh-5650fedc";
const SYNC: &str = "app-org.example.Sync@autostart.service";

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn fixture(name: &str) -> Root {
    Root::new(fixture_path(name))
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        let kind = e.file_type().unwrap();
        if kind.is_symlink() {
            // The fixtures' links name paths of an imagined machine:
            // copied as links, never followed.
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dest).unwrap();
        } else if kind.is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A scratch copy of root-desktop.
fn scratch(name: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("mirqab-actions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    copy_tree(&fixture_path("root-desktop"), &d);
    Scratch(d)
}

fn recorded() -> (Actions, Arc<Recording>) {
    let rec = Arc::new(Recording::default());
    (Actions::new(rec.clone()), rec)
}

fn ask(action: Action, params: Value) -> actions::Ask {
    actions::parse(action, &params, Some(CALLER)).expect("a valid request")
}

fn pid(p: u32) -> Value {
    json!({ "pid": p })
}

fn group(id: &str) -> Value {
    json!({ "group": id })
}

/// The first call: needs-confirm, with its data.
fn plan(a: &Actions, root: &Root, action: Action, args: Value) -> Value {
    let f = a
        .handle(root, &ask(action, json!({"args": args})), None)
        .unwrap_err();
    assert_eq!(f.code, Code::NeedsConfirm, "{}", f.message);
    f.data.expect("needs-confirm carries the plan")
}

fn confirm(
    a: &Actions,
    root: &Root,
    action: Action,
    args: Value,
    plan: &str,
    history: Option<&Mutex<History>>,
) -> Result<Value, Failure> {
    a.handle(
        root,
        &ask(action, json!({"args": args, "confirm": true, "plan": plan})),
        history,
    )
}

fn dry(a: &Actions, root: &Root, args: Value) -> Result<Value, Failure> {
    a.handle(
        root,
        &ask(Action::Quit, json!({"args": args, "dryRun": true})),
        None,
    )
}

fn pids(list: &Value) -> Vec<u64> {
    list.as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["pid"].as_u64())
        .collect()
}

#[test]
fn every_refusal_holds_and_nothing_is_recorded() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let cases: &[(u32, &str)] = &[
        (1, "never signalled"),
        (410, "another user"),
        (600, "another user"),
        (2, "kernel thread"),
        (4321, "mirqab daemon"),
        (1100, "user manager"),
        (1101, "user manager"),
        (1050, "compositor"),
        (900, "another user"),
        (6300, "another user's rights"),
        (CALLER, "the program asking"),
        (3050, "the program asking"),
        (3000, "the program asking"),
    ];
    for (p, why) in cases {
        for params in [
            json!({"args": pid(*p)}),
            json!({"args": pid(*p), "dryRun": true}),
        ] {
            let f = a
                .handle(&root, &ask(Action::Quit, params), None)
                .unwrap_err();
            assert_eq!(f.code, Code::Refused, "{p}: {}", f.message);
            assert!(f.message.contains(why), "{p}: {}", f.message);
        }
    }
    // The daemon's own unit is not stopped either.
    let f = a
        .handle(&root, &ask(Action::Kill, json!({"args": pid(4321)})), None)
        .unwrap_err();
    assert!(
        f.message.contains("mirqab.service is not stopped"),
        "{}",
        f.message
    );
    assert!(rec.lines().is_empty());
    assert_eq!(a.live_plans(), 0, "a refusal makes no plan");
}

#[test]
fn a_caller_that_cannot_be_walked_refuses_everything() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    for caller in [None, Some(77)] {
        let ask = actions::parse(Action::Quit, &json!({"args": pid(2001)}), caller).unwrap();
        let f = a.handle(&root, &ask, None).unwrap_err();
        assert_eq!(f.code, Code::Refused, "{caller:?}: {}", f.message);
    }
    assert!(rec.lines().is_empty());
}

#[test]
fn would_do_lists_leaders_first_and_a_dry_run_plans_nothing() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let v = dry(&a, &root, group(BROWSER)).unwrap();
    let would = &v["wouldDo"];
    assert_eq!(pids(would), [2001, 2000, 2010, 2011, 2012]);
    let starts: Vec<u64> = would
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["start"].as_u64().unwrap())
        .collect();
    assert_eq!(starts, [5010, 5000, 5100, 5200, 5300]);
    assert_eq!(would[0]["signal"], "TERM");
    assert_eq!(would[0]["name"], "browser");
    assert!(
        !would.to_string().contains("--new-window"),
        "no command line"
    );
    assert_eq!(a.live_plans(), 0);
    assert!(rec.lines().is_empty());
}

#[test]
fn two_calls_always_and_a_plan_is_used_once_by_its_own_action() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let data = plan(&a, &root, Action::Quit, group(BROWSER));
    assert_eq!(pids(&data["wouldDo"]), [2001, 2000, 2010, 2011, 2012]);
    // confirm: true without a plan is a first call too.
    let f = a
        .handle(
            &root,
            &ask(
                Action::Quit,
                json!({"args": group(BROWSER), "confirm": true}),
            ),
            None,
        )
        .unwrap_err();
    assert_eq!(f.code, Code::NeedsConfirm);
    assert!(rec.lines().is_empty(), "no first call acts");

    let id = data["plan"].as_str().unwrap();
    let done = confirm(&a, &root, Action::Quit, group(BROWSER), id, None).unwrap();
    assert_eq!(pids(&done["done"]), [2001, 2000, 2010, 2011, 2012]);
    assert_eq!(
        rec.lines(),
        [
            "TERM 2001 5010",
            "TERM 2000 5000",
            "TERM 2010 5100",
            "TERM 2011 5200",
            "TERM 2012 5300"
        ]
    );
    let again = confirm(&a, &root, Action::Quit, group(BROWSER), id, None).unwrap_err();
    assert_eq!(again.code, Code::BadRequest, "{}", again.message);

    let quit_plan = plan(&a, &root, Action::Quit, pid(7100));
    let as_kill = confirm(
        &a,
        &root,
        Action::Kill,
        pid(7100),
        quit_plan["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(as_kill.code, Code::Refused, "{}", as_kill.message);
    let other_pid = plan(&a, &root, Action::Quit, pid(7100));
    let f = confirm(
        &a,
        &root,
        Action::Quit,
        pid(8000),
        other_pid["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.code, Code::Refused, "a plan is bound to its subject");
    assert_eq!(rec.lines().len(), 5, "neither was signalled");

    let kill = plan(&a, &root, Action::Kill, pid(7100));
    confirm(
        &a,
        &root,
        Action::Kill,
        pid(7100),
        kill["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(rec.lines().last().unwrap(), "KILL 7100 34000");
}

#[test]
fn targets_are_re_computed_at_the_second_call() {
    let (a, rec) = recorded();
    let data = plan(&a, &fixture("root-desktop"), Action::Quit, pid(7000));
    assert_eq!(data["wouldDo"][0]["start"], 300000);
    // In the .t1 tree pid 7000 is another process (start 360000).
    let v = confirm(
        &a,
        &fixture("root-desktop.t1"),
        Action::Quit,
        pid(7000),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(v["done"], json!([]));
    assert_eq!(v["gone"], json!([{"pid": 7000, "start": 300000}]));
    assert_eq!(pids(&v["skipped"]), [7000]);
    assert_eq!(v["skipped"][0]["reason"], "started after the confirmation");
    assert!(rec.lines().is_empty(), "the new 7000 is not signalled");
}

#[test]
fn a_group_leaves_its_refused_members_out() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    // The session scope's tree: the compositor and the shell it started.
    let v = dry(&a, &root, group("tree:1050")).unwrap();
    assert_eq!(pids(&v["wouldDo"]), [6200]);
    assert_eq!(pids(&v["skipped"]), [1050]);
    assert!(
        v["skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("compositor")
    );
    let data = plan(&a, &root, Action::Quit, group("tree:1050"));
    let done = confirm(
        &a,
        &root,
        Action::Quit,
        group("tree:1050"),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(pids(&done["skipped"]), [1050]);
    assert_eq!(rec.lines(), ["TERM 6200 31000"]);
    // A group with nothing left is refused, and says what was left alone.
    let f = dry(&a, &root, group("system")).unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert!(f.data.unwrap()["skipped"].as_array().unwrap().len() >= 6);
    let f = dry(&a, &root, group("scope:nothing")).unwrap_err();
    assert_eq!(f.code, Code::Failed);
}

#[test]
fn a_rootless_container_on_a_sub_uid_is_refused_with_its_runtime() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let f = dry(&a, &root, group("container:c0ffee00c0ff")).unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert!(
        f.message.contains("podman stop c0ffee00c0ff"),
        "{}",
        f.message
    );
    assert!(rec.lines().is_empty());
}

#[test]
fn a_thread_id_plans_its_thread_group() {
    let s = scratch("thread");
    let t = s.0.join("proc/2015");
    std::fs::create_dir_all(&t).unwrap();
    std::fs::write(
        t.join("status"),
        "Name:\tGecko_IOThread\nTgid:\t2001\nPid:\t2015\nUid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let (a, _) = recorded();
    let v = dry(&a, &root, pid(2015)).unwrap();
    assert_eq!(
        v["wouldDo"],
        json!([{"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser", "via": 2015}])
    );
}

#[test]
fn a_user_service_is_stopped_through_systemctl() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let stop = format!("systemctl --user stop --no-block -- {SYNC}");
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert_eq!(
        v["wouldDo"],
        json!([{"run": stop.split(' ').collect::<Vec<_>>(), "unit": SYNC, "pids": [1300]}])
    );
    assert!(rec.lines().is_empty(), "a dry run runs nothing");

    let group_plan = plan(
        &a,
        &root,
        Action::Quit,
        group("service:app-org.example.Sync@autostart"),
    );
    confirm(
        &a,
        &root,
        Action::Quit,
        group("service:app-org.example.Sync@autostart"),
        group_plan["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(rec.lines(), [format!("RUN {stop}")]);

    let kill = plan(&a, &root, Action::Kill, pid(1300));
    assert_eq!(kill["wouldDo"].as_array().unwrap().len(), 2);
    let v = confirm(
        &a,
        &root,
        Action::Kill,
        pid(1300),
        kill["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(v["done"].as_array().unwrap().len(), 2);
    assert_eq!(
        rec.lines()[1..],
        [
            format!("RUN {stop}"),
            format!("RUN systemctl --user kill --signal=KILL -- {SYNC}")
        ]
    );

    // A unit name mirqab would not pass to systemctl is refused.
    let s = scratch("oddunit");
    std::fs::write(
        s.0.join("proc/1300/cgroup"),
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/odd;name.service\n",
    )
    .unwrap();
    let f = dry(&a, &Root::new(&s.0), pid(1300)).unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert!(f.message.contains("odd;name.service"), "{}", f.message);
    assert_eq!(rec.lines().len(), 3);
}

#[test]
fn each_redeemed_plan_is_one_action_event() {
    let d = std::env::temp_dir().join(format!("mirqab-actions-event-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let s = Scratch(d);
    let history = Mutex::new(History::open(&s.0.join("history.db"), 0).unwrap());
    let root = fixture("root-desktop");
    let (a, _) = recorded();
    let events = |h: &Mutex<History>| -> Vec<Value> {
        h.lock()
            .unwrap()
            .events(Some("action"), 0, i64::MAX)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    };
    a.handle(
        &root,
        &ask(
            Action::Quit,
            json!({"args": group(BROWSER), "dryRun": true}),
        ),
        Some(&history),
    )
    .unwrap();
    let data = a
        .handle(
            &root,
            &ask(Action::Quit, json!({"args": group(BROWSER)})),
            Some(&history),
        )
        .unwrap_err()
        .data
        .unwrap();
    assert!(
        events(&history).is_empty(),
        "a dry run and a plan write nothing"
    );
    confirm(
        &a,
        &root,
        Action::Quit,
        group(BROWSER),
        data["plan"].as_str().unwrap(),
        Some(&history),
    )
    .unwrap();
    let rows = events(&history);
    assert_eq!(rows.len(), 1);
    let e = &rows[0]["data"];
    assert_eq!(e["action"], "apps.quit");
    assert_eq!(e["target"], group(BROWSER));
    assert_eq!(e["caller"], json!({"pid": CALLER, "name": "python3"}));
    assert_eq!(pids(&e["results"]), [2001, 2000, 2010, 2011, 2012]);
    assert_eq!(e["results"][0]["result"], "done");
    let v = confirm(
        &a,
        &root,
        Action::Quit,
        group(BROWSER),
        plan(&a, &root, Action::Quit, group(BROWSER))["plan"]
            .as_str()
            .unwrap(),
        Some(&history),
    )
    .unwrap();
    assert_eq!(v["recorded"], true, "the row was written");
    assert_eq!(e["results"][0]["signal"], "TERM");
    let text = e.to_string();
    for private in ["--new-window", "/usr/lib/example-browser", "contentproc"] {
        assert!(!text.contains(private), "no command line: {text}");
    }
}

#[test]
fn a_fixture_root_never_gets_the_real_sender() {
    // Whatever MIRQAB_TEST says: the choice reads the root only.
    assert!(!act::sender_for(&fixture("root-desktop")).is_real());
    assert!(act::sender_for(&Root::host()).is_real());
}

#[test]
fn an_act_without_a_history_says_it_was_not_recorded() {
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let data = plan(&a, &root, Action::Quit, pid(7100));
    let v = confirm(
        &a,
        &root,
        Action::Quit,
        pid(7100),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(v["recorded"], false);
    assert_eq!(rec.lines(), ["TERM 7100 34000"], "the act still ran");
}

#[test]
fn a_nested_cgroup_never_names_a_host_service() {
    // A rootless container running its own systemd: its dbus-broker.service
    // is not the session's (review of PR #17).
    let s = scratch("nested");
    let cgroup = s.0.join("proc/1300/cgroup");
    std::fs::write(
        &cgroup,
        format!(
            "0::/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-{}.scope/container/system.slice/dbus-broker.service\n",
            "c0ffee00".repeat(8)
        ),
    )
    .unwrap();
    let (a, rec) = recorded();
    let root = Root::new(&s.0);
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert_eq!(
        v["wouldDo"],
        json!([{"signal": "TERM", "pid": 1300, "start": 2600, "name": "example-sync"}])
    );
    assert!(!v.to_string().contains("\"run\""), "no host service: {v}");
    // A service's own sub-cgroup is that service, not the inner name.
    std::fs::write(
        &cgroup,
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/outer.service/payload/inner.service\n",
    )
    .unwrap();
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert_eq!(
        v["wouldDo"][0]["run"],
        json!([
            "systemctl",
            "--user",
            "stop",
            "--no-block",
            "--",
            "outer.service"
        ])
    );
    assert!(rec.lines().is_empty());
}

/// A clock a test moves: seconds since boot, or an error when `u64::MAX`.
fn test_clock(at: &Arc<AtomicU64>) -> actions::Clock {
    let at = Arc::clone(at);
    Arc::new(move || match at.load(Ordering::SeqCst) {
        u64::MAX => Err(std::io::Error::other("no clock")),
        s => Ok(Duration::from_secs(s)),
    })
}

#[test]
fn a_plan_expires_on_the_actions_clock_and_no_clock_acts_on_nothing() {
    let root = fixture("root-desktop");
    let at = Arc::new(AtomicU64::new(1000));
    let rec = Arc::new(Recording::default());
    let a = Actions::with_clock(rec.clone(), test_clock(&at));
    let data = plan(&a, &root, Action::Quit, pid(7100));
    at.store(1000 + 121, Ordering::SeqCst);
    let f = confirm(
        &a,
        &root,
        Action::Quit,
        pid(7100),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.code, Code::BadRequest);
    assert_eq!(f.message, mirqab_core::proto::strings::PLAN_EXPIRED);
    assert!(rec.lines().is_empty());

    let data = plan(&a, &root, Action::Quit, pid(7100));
    at.store(1121 + 119, Ordering::SeqCst);
    let v = confirm(
        &a,
        &root,
        Action::Quit,
        pid(7100),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(pids(&v["done"]), [7100]);

    at.store(u64::MAX, Ordering::SeqCst);
    let fresh = Actions::with_clock(rec.clone(), test_clock(&at));
    let f = fresh
        .handle(&root, &ask(Action::Quit, json!({"args": pid(7100)})), None)
        .unwrap_err();
    assert_eq!(f.code, Code::Failed);
    assert_eq!(fresh.live_plans(), 0);
    assert_eq!(rec.lines().len(), 1, "only the one confirmed above");
}

/// A sender whose commands all fail: `systemctl` exits 1.
#[derive(Default)]
struct Failing {
    runs: Mutex<Vec<String>>,
}

impl Sender for Failing {
    fn is_real(&self) -> bool {
        false
    }
    fn open(&self, _pid: u32) -> std::io::Result<Option<OwnedFd>> {
        Ok(None)
    }
    fn send(&self, _h: Option<&OwnedFd>, _t: Target, _s: Signal) -> std::io::Result<()> {
        Ok(())
    }
    fn run(&self, argv: &[&str], _timeout: Duration) -> Result<exec::Output, String> {
        self.runs.lock().unwrap().push(argv.join(" "));
        Ok(exec::Output {
            status: Some(1),
            stdout: String::new(),
            stderr: "boom".into(),
        })
    }
    fn unit_dependents(&self, _root: &Root, _unit: &str) -> Result<Vec<String>, String> {
        Err("boom".into())
    }
}

#[test]
fn a_failed_stop_skips_the_kill() {
    let root = fixture("root-desktop");
    let sender = Arc::new(Failing::default());
    let a = Actions::new(sender.clone());
    let data = plan(&a, &root, Action::Kill, pid(1300));
    let v = confirm(
        &a,
        &root,
        Action::Kill,
        pid(1300),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(v["done"], json!([]));
    let failed = v["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0]["run"],
        json!(["systemctl", "--user", "stop", "--no-block", "--", SYNC])
    );
    assert_eq!(failed[0]["error"], "boom");
    assert_eq!(
        sender.runs.lock().unwrap().len(),
        1,
        "no kill after a failed stop"
    );
}

#[test]
fn a_stop_names_the_active_units_it_takes_with_it() {
    // The fixture's answer for pipewire.service: wireplumber.service active
    // (pid 1211), pipewire-pulse.service inactive.
    use mirqab_core::proto::strings;
    let root = fixture("root-desktop");
    let (a, rec) = recorded();
    let v = dry(&a, &root, pid(1210)).unwrap();
    assert_eq!(
        v["wouldDo"],
        json!([{
            "run": ["systemctl", "--user", "stop", "--no-block", "--", "pipewire.service"],
            "unit": "pipewire.service",
            "pids": [1210],
            "alsoStops": [{"unit": "wireplumber.service", "pids": [1211]}],
        }])
    );
    // Quit is allowed, with one warning line for each sound unit
    // (owner, 2026-11-14, Q2).
    assert_eq!(
        v["warnings"],
        json!([
            strings::warn_sound("pipewire.service"),
            strings::warn_sound("wireplumber.service")
        ])
    );
    // Force quit: the dependents ride on the stop, not on the kill.
    let data = plan(&a, &root, Action::Kill, pid(1210));
    assert!(data["wouldDo"][0].get("alsoStops").is_some());
    assert!(data["wouldDo"][1].get("alsoStops").is_none());
    assert_eq!(data["warnings"].as_array().unwrap().len(), 2);
    let hist = scratch("dephistory");
    let history = Mutex::new(History::open(&hist.0.join("history.db"), 0).unwrap());
    let done = confirm(
        &a,
        &root,
        Action::Kill,
        pid(1210),
        data["plan"].as_str().unwrap(),
        Some(&history),
    )
    .unwrap();
    assert_eq!(done["done"].as_array().unwrap().len(), 2);
    assert_eq!(
        rec.lines(),
        [
            "RUN systemctl --user stop --no-block -- pipewire.service",
            "RUN systemctl --user kill --signal=KILL -- pipewire.service"
        ]
    );
    let events = history
        .lock()
        .unwrap()
        .events(Some("action"), 0, i64::MAX)
        .unwrap();
    assert_eq!(
        events[0]["data"]["alsoStops"],
        json!(["wireplumber.service"])
    );
    // A service no unit depends on carries neither key nor warning.
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert!(v["wouldDo"][0].get("alsoStops").is_none());
    assert!(v["wouldDo"][0].get("alsoStopsUnknown").is_none());
    assert_eq!(v["warnings"], json!([]));
}

#[test]
fn a_dependent_that_holds_a_refused_process_refuses_the_stop() {
    use mirqab_core::proto::strings;
    let s = scratch("dependent");
    let deps = s.0.join("run/mirqab-test/dependents");
    std::fs::create_dir_all(&deps).unwrap();
    let root = Root::new(&s.0);
    let (a, rec) = recorded();
    // The daemon's own unit (pid 4321) as a dependent of the sync service.
    std::fs::write(deps.join(SYNC), "mirqab.service active\n").unwrap();
    let f = dry(&a, &root, pid(1300)).unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert_eq!(
        f.message,
        strings::refuse_unit_dependent(SYNC, "mirqab.service", &strings::refuse_daemon(4321))
    );
    let f = a
        .handle(&root, &ask(Action::Quit, json!({"args": pid(1300)})), None)
        .unwrap_err();
    assert_eq!(f.code, Code::Refused, "no plan is made either");
    assert_eq!(a.live_plans(), 0);
    // The session bus as a dependent refuses it too.
    std::fs::write(deps.join(SYNC), "dbus-broker.service active\n").unwrap();
    let f = dry(&a, &root, pid(1300)).unwrap_err();
    assert_eq!(
        f.message,
        strings::refuse_unit_dependent(
            SYNC,
            "dbus-broker.service",
            &strings::refuse_session_bus_unit("dbus-broker.service")
        )
    );
    // An inactive dependent is not asked about at all.
    std::fs::write(deps.join(SYNC), "mirqab.service inactive\n").unwrap();
    assert!(dry(&a, &root, pid(1300)).is_ok());
    assert!(rec.lines().is_empty());
}

#[test]
fn a_question_that_fails_still_plans_and_says_so() {
    let s = scratch("unknown");
    // A directory where the answer should be: the read fails.
    std::fs::create_dir_all(s.0.join("run/mirqab-test/dependents").join(SYNC)).unwrap();
    let root = Root::new(&s.0);
    let (a, rec) = recorded();
    let data = plan(&a, &root, Action::Quit, pid(1300));
    assert_eq!(data["wouldDo"][0]["alsoStopsUnknown"], true);
    assert!(data["wouldDo"][0].get("alsoStops").is_none());
    confirm(
        &a,
        &root,
        Action::Quit,
        pid(1300),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        rec.lines(),
        [format!("RUN systemctl --user stop --no-block -- {SYNC}")]
    );
}

#[test]
fn the_session_bus_is_never_stopped() {
    use mirqab_core::proto::strings;
    let s = scratch("bus");
    let (a, rec) = recorded();
    for unit in ["dbus-broker.service", "dbus.service", "dbus-daemon.service"] {
        std::fs::write(
            s.0.join("proc/1300/cgroup"),
            format!("0::/user.slice/user-1000.slice/user@1000.service/session.slice/{unit}\n"),
        )
        .unwrap();
        let root = Root::new(&s.0);
        let f = dry(&a, &root, pid(1300)).unwrap_err();
        assert_eq!(f.code, Code::Refused);
        assert_eq!(f.message, strings::refuse_session_bus_unit(unit));
        let stem = unit.trim_end_matches(".service");
        let f = dry(&a, &root, group(&format!("service:{stem}"))).unwrap_err();
        assert_eq!(f.code, Code::Refused, "{unit} as a group");
        assert_eq!(f.message, strings::refuse_session_bus_unit(unit));
    }
    assert!(rec.lines().is_empty());
}

const USER: &str = "0::/user.slice/user-1000.slice/user@1000.service";
const HELD: &str = r"app-held\x2dterm-1.scope";

/// Moves `pid` of the scratch tree into the host unit path `path` (after
/// the user manager).
fn move_to(s: &Scratch, pid: u32, path: &str) {
    std::fs::write(
        s.0.join(format!("proc/{pid}/cgroup")),
        format!("{USER}/{path}\n"),
    )
    .unwrap();
}

/// Writes the fixture's answer for which units stop with `unit`.
fn dependents(s: &Scratch, unit: &str, text: &str) {
    let d = s.0.join("run/mirqab-test/dependents");
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(unit);
    if f.is_dir() {
        std::fs::remove_dir_all(&f).unwrap();
    }
    std::fs::write(f, text).unwrap();
}

#[test]
fn a_dependent_of_any_unit_type_is_found_and_checked() {
    use mirqab_core::proto::strings;
    let s = scratch("anytype");
    let root = Root::new(&s.0);
    let (a, rec) = recorded();
    // A scope holding the caller (3060): refused, with a backslash in its
    // name end to end.
    move_to(&s, CALLER, &format!("app.slice/{HELD}"));
    dependents(&s, SYNC, &format!("{HELD} active\n"));
    let f = a
        .handle(&root, &ask(Action::Quit, json!({"args": pid(1300)})), None)
        .unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert_eq!(
        f.message,
        strings::refuse_unit_dependent(SYNC, HELD, &strings::refuse_caller(CALLER))
    );
    assert_eq!(a.live_plans(), 0);
    // A scope of ordinary processes: its pids are listed.
    dependents(&s, SYNC, "app-Hyprland-browser.sh-5650fedc.scope active\n");
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert_eq!(
        v["wouldDo"][0]["alsoStops"],
        json!([{"unit": "app-Hyprland-browser.sh-5650fedc.scope", "pids": [2001, 2000, 2010, 2011, 2012]}])
    );
    // A slice with a refused process in a unit under it: the daemon.
    move_to(&s, 4321, "held.slice/held-inner.service");
    dependents(&s, SYNC, "held.slice active\n");
    let f = dry(&a, &root, pid(1300)).unwrap_err();
    assert_eq!(
        f.message,
        strings::refuse_unit_dependent(SYNC, "held.slice", &strings::refuse_daemon(4321))
    );
    assert!(rec.lines().is_empty());
}

/// A sender that answers `unit_dependents` through `act::dependents_via`,
/// as `Real` does, from `systemctl show` text in systemd 262's format.
struct Showing {
    rec: Recording,
    first: String,
    second: String,
}

impl Sender for Showing {
    fn is_real(&self) -> bool {
        false
    }
    fn open(&self, pid: u32) -> std::io::Result<Option<OwnedFd>> {
        self.rec.open(pid)
    }
    fn send(&self, h: Option<&OwnedFd>, t: Target, s: Signal) -> std::io::Result<()> {
        self.rec.send(h, t, s)
    }
    fn run(&self, argv: &[&str], timeout: Duration) -> Result<exec::Output, String> {
        self.rec.run(argv, timeout)
    }
    fn unit_dependents(&self, _root: &Root, unit: &str) -> Result<Vec<String>, String> {
        let show = |argv: &[&str]| -> Result<String, String> {
            Ok(if argv.contains(&"Id") {
                self.second.clone()
            } else {
                self.first.clone()
            })
        };
        act::dependents_via(&show, unit)
    }
}

#[test]
fn quoted_escaped_names_from_systemctl_are_found_and_refused() {
    // systemd quotes a list element that needs it and doubles its
    // backslashes: `"app-held\\x2dterm-1.scope"`. Read word by word, that
    // name failed the unit-name check and the stop became alsoStopsUnknown,
    // with no refusal run on the scope's processes (review of PR #18, B1).
    use mirqab_core::proto::strings;
    let s = scratch("quoted");
    let root = Root::new(&s.0);
    let showing = |a: &str, b: &str| Showing {
        rec: Recording::default(),
        first: format!(
            "RequiredBy=\nRequisiteOf=\nBoundBy={a} \"app-held\\\\x2dterm-1.scope\"\nConsistsOf=\nPropagatesStopTo=\n"
        ),
        second: format!("Id={a}\nActiveState=active\n\nId={HELD}\nActiveState={b}\n"),
    };
    let sender = Arc::new(showing("plain-dep.service", "active"));
    let a = Actions::new(sender.clone());
    let v = dry(&a, &root, pid(1300)).unwrap();
    assert!(v["wouldDo"][0].get("alsoStopsUnknown").is_none(), "{v}");
    assert_eq!(
        v["wouldDo"][0]["alsoStops"],
        json!([{"unit": "plain-dep.service", "pids": []}, {"unit": HELD, "pids": []}])
    );
    move_to(&s, CALLER, &format!("app.slice/{HELD}"));
    let f = dry(&a, &root, pid(1300)).unwrap_err();
    assert_eq!(
        f.message,
        strings::refuse_unit_dependent(SYNC, HELD, &strings::refuse_caller(CALLER))
    );
    assert!(sender.rec.lines().is_empty());
}

#[test]
fn a_confirmation_does_not_stop_a_dependent_it_did_not_show() {
    use mirqab_core::proto::strings;
    let s = scratch("planalso");
    let root = Root::new(&s.0);
    let (a, rec) = recorded();
    let unit = "pipewire.service";
    // A unit that starts depending after the plan: refused, plan spent.
    let data = plan(&a, &root, Action::Quit, pid(1210));
    let id = data["plan"].as_str().unwrap();
    dependents(
        &s,
        unit,
        &format!("wireplumber.service active\n{SYNC} active\n"),
    );
    let f = confirm(&a, &root, Action::Quit, pid(1210), id, None).unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert_eq!(
        f.message,
        strings::plan_dependents_changed(unit, &[SYNC.to_string()])
    );
    assert!(rec.lines().is_empty(), "nothing ran");
    let f = confirm(&a, &root, Action::Quit, pid(1210), id, None).unwrap_err();
    assert_eq!(f.message, strings::PLAN_UNKNOWN, "the plan was spent");
    // The plan listed them, and now they cannot be told.
    dependents(&s, unit, "wireplumber.service active\n");
    let data = plan(&a, &root, Action::Quit, pid(1210));
    let d = s.0.join("run/mirqab-test/dependents").join(unit);
    std::fs::remove_file(&d).unwrap();
    std::fs::create_dir(&d).unwrap();
    let f = confirm(
        &a,
        &root,
        Action::Quit,
        pid(1210),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.message, strings::plan_dependents_unknown(unit));
    assert!(rec.lines().is_empty());
    // A shown dependent that went away is fine: the stop runs.
    dependents(&s, unit, "wireplumber.service active\n");
    let data = plan(&a, &root, Action::Quit, pid(1210));
    dependents(&s, unit, "wireplumber.service inactive\n");
    confirm(
        &a,
        &root,
        Action::Quit,
        pid(1210),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        rec.lines(),
        ["RUN systemctl --user stop --no-block -- pipewire.service"]
    );
}

#[test]
fn a_pid_asked_with_its_start_time_fails_once_it_names_another_process() {
    use mirqab_core::proto::strings;
    let (a, rec) = recorded();
    let (t0, t1) = (fixture("root-desktop"), fixture("root-desktop.t1"));
    // 7000 is `oldjob` (start 300000) in root-desktop, `newjob` in the twin.
    let args = json!({"pid": 7000, "start": 300000});
    let p = plan(&a, &t0, Action::Quit, args.clone());
    assert_eq!(p["wouldDo"][0]["start"], 300000);
    let id = p["plan"].as_str().unwrap().to_string();
    let f = confirm(&a, &t1, Action::Quit, args.clone(), &id, None).unwrap_err();
    assert_eq!(f.code, Code::Failed);
    assert_eq!(f.message, strings::apps_process_gone(7000));
    assert!(rec.lines().is_empty(), "nothing sent");
    // A service's pid with another start: refused the same way, before
    // the unit is looked at.
    let f = a
        .handle(
            &t0,
            &ask(Action::Quit, json!({"args": {"pid": 1300, "start": 1}})),
            None,
        )
        .unwrap_err();
    assert_eq!(f.code, Code::Failed);
    assert_eq!(f.message, strings::apps_process_gone(1300));
    // The right start plans as a bare pid does; a plan is bound to the
    // subject it was made for, start included.
    let p = plan(&a, &t0, Action::Quit, json!({"pid": 7000, "start": 300000}));
    let f = confirm(
        &a,
        &t0,
        Action::Quit,
        pid(7000),
        p["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.code, Code::Refused, "{}", f.message);
    for bad in [
        json!({"args": {"pid": 7000, "start": "x"}}),
        json!({"args": {"pid": 7000, "start": 1, "group": "x"}}),
    ] {
        assert_eq!(
            actions::parse(Action::Quit, &bad, Some(CALLER))
                .unwrap_err()
                .code,
            Code::BadRequest,
            "{bad}"
        );
    }
}

#[test]
fn a_session_bus_outside_its_units_is_refused_by_name() {
    use mirqab_core::proto::strings;
    let s = scratch("busname");
    // A private bus a `dbus-run-session` started, in the login session's
    // scope (no unit of its own): a `tree:` group of the user's.
    let p = s.0.join("proc/9500");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("stat"),
        "9500 (dbus-daemon) S 1 9500 9500 0 -1 4194560 0 0 0 0 1 1 0 0 20 0 1 0 900000 0 0 0",
    )
    .unwrap();
    std::fs::write(
        p.join("status"),
        "Name:\tdbus-daemon\nTgid:\t9500\nUid:\t1000\t1000\t1000\t1000\n",
    )
    .unwrap();
    std::fs::write(
        p.join("cgroup"),
        "0::/user.slice/user-1000.slice/session-2.scope\n",
    )
    .unwrap();
    let root = Root::new(&s.0);
    let (a, rec) = recorded();
    let f = a
        .handle(&root, &ask(Action::Quit, json!({"args": pid(9500)})), None)
        .unwrap_err();
    assert_eq!(f.code, Code::Refused);
    assert_eq!(
        f.message,
        strings::refuse_session_bus_comm(9500, "dbus-daemon")
    );
    let f = a
        .handle(
            &root,
            &ask(Action::Kill, json!({"args": group("tree:9500")})),
            None,
        )
        .unwrap_err();
    assert_eq!(f.code, Code::Refused, "its group too: {}", f.message);
    assert_eq!(
        f.data.unwrap()["skipped"][0]["reason"],
        strings::refuse_session_bus_comm(9500, "dbus-daemon")
    );
    assert!(rec.lines().is_empty());
}

#[test]
fn a_confirmation_does_not_stop_a_unit_the_plan_did_not_show() {
    // The plan keeps the unit it would stop, apart from its dependents: a
    // pid planned for a signal and moved into a user service before the
    // confirmation is not stopped through that service, whose stop would
    // end its other processes and dependents (audit of plan 006).
    use mirqab_core::proto::strings;
    const OTHER: &str = "other.service";
    let events = |h: &Mutex<History>| {
        h.lock()
            .unwrap()
            .events(Some("action"), 0, i64::MAX)
            .unwrap()
            .as_array()
            .map_or(0, Vec::len)
    };
    let refused = |a: &Actions, root: &Root, args: Value, id: &str, h: &Mutex<History>| {
        let f = confirm(a, root, Action::Quit, args.clone(), id, Some(h)).unwrap_err();
        assert_eq!(f.code, Code::Refused, "{}", f.message);
        let again = confirm(a, root, Action::Quit, args, id, Some(h)).unwrap_err();
        assert_eq!(again.message, strings::PLAN_UNKNOWN, "the plan was spent");
        f.message
    };
    let s = scratch("planunit");
    let root = Root::new(&s.0);
    let hist = scratch("planunithistory");
    let history = Mutex::new(History::open(&hist.0.join("history.db"), 0).unwrap());
    let (a, rec) = recorded();

    // A signal planned, a unit now: refused.
    let data = plan(&a, &root, Action::Quit, pid(2001));
    assert_eq!(data["wouldDo"][0]["signal"], "TERM", "{data}");
    move_to(&s, 2001, &format!("app.slice/{OTHER}"));
    let msg = refused(
        &a,
        &root,
        pid(2001),
        data["plan"].as_str().unwrap(),
        &history,
    );
    assert_eq!(msg, strings::plan_unit_changed(None, Some(OTHER)));
    assert!(rec.lines().is_empty(), "nothing ran: {:?}", rec.lines());
    assert_eq!(events(&history), 0, "no event");

    // A unit whose dependents were unknown, another unit now: refused.
    std::fs::create_dir_all(s.0.join("run/mirqab-test/dependents").join(SYNC)).unwrap();
    let data = plan(&a, &root, Action::Quit, pid(1300));
    assert_eq!(data["wouldDo"][0]["alsoStopsUnknown"], true);
    move_to(&s, 1300, &format!("app.slice/{OTHER}"));
    let msg = refused(
        &a,
        &root,
        pid(1300),
        data["plan"].as_str().unwrap(),
        &history,
    );
    assert_eq!(msg, strings::plan_unit_changed(Some(SYNC), Some(OTHER)));
    assert!(rec.lines().is_empty(), "nothing ran: {:?}", rec.lines());

    // A unit planned, signals now: refused too, though narrower.
    move_to(&s, 1300, &format!("app.slice/{SYNC}"));
    dependents(&s, SYNC, "");
    let data = plan(&a, &root, Action::Quit, pid(1300));
    assert_eq!(data["wouldDo"][0]["unit"], SYNC);
    move_to(&s, 1300, "app.slice/app-held-1.scope");
    let msg = refused(
        &a,
        &root,
        pid(1300),
        data["plan"].as_str().unwrap(),
        &history,
    );
    assert_eq!(msg, strings::plan_unit_changed(Some(SYNC), None));
    assert!(rec.lines().is_empty(), "nothing ran: {:?}", rec.lines());
    assert_eq!(events(&history), 0, "no event");

    // The same unit at both calls still stops.
    move_to(&s, 1300, &format!("app.slice/{SYNC}"));
    let data = plan(&a, &root, Action::Quit, pid(1300));
    confirm(
        &a,
        &root,
        Action::Quit,
        pid(1300),
        data["plan"].as_str().unwrap(),
        Some(&history),
    )
    .unwrap();
    assert_eq!(
        rec.lines(),
        [format!("RUN systemctl --user stop --no-block -- {SYNC}")]
    );
    assert_eq!(events(&history), 1);
}

#[test]
fn a_plan_expires_when_the_confirmation_reaches_it_not_when_it_arrived() {
    // Audit 2026-11-21, L15: resolving the targets walks every process and
    // may wait on `systemctl show` twice, so the clock is read again when
    // the plan is redeemed. A confirmation that arrives at 119 s and is
    // resolved by 121 s is past the 120 s a plan lives.
    let root = fixture("root-desktop");
    let ticks = Arc::new(Mutex::new(vec![1000u64, 1119, 1121]));
    let clock: actions::Clock = {
        let ticks = Arc::clone(&ticks);
        Arc::new(move || {
            let mut t = ticks.lock().unwrap();
            let s = if t.len() > 1 { t.remove(0) } else { t[0] };
            Ok(Duration::from_secs(s))
        })
    };
    let rec = Arc::new(Recording::default());
    let a = Actions::with_clock(rec.clone(), clock);
    let data = plan(&a, &root, Action::Quit, pid(7100));
    let f = confirm(
        &a,
        &root,
        Action::Quit,
        pid(7100),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.code, Code::BadRequest);
    assert_eq!(f.message, mirqab_core::proto::strings::PLAN_EXPIRED);
    assert!(rec.lines().is_empty(), "nothing sent");
    assert_eq!(a.live_plans(), 0, "the plan is spent");
}

#[test]
fn a_unit_whose_processes_all_started_since_is_recorded_as_skipped() {
    // Audit 2026-11-21, L27: a unit is not stopped when every process in
    // it started after the plan; each is skipped, and the `action` event
    // says so for each, as it does for a signal.
    let s = scratch("unitlate");
    let root = Root::new(&s.0);
    let hist = scratch("unitlatehistory");
    let history = Mutex::new(History::open(&hist.0.join("history.db"), 0).unwrap());
    let (a, rec) = recorded();
    let data = plan(&a, &root, Action::Quit, pid(1300));
    assert_eq!(data["wouldDo"][0]["unit"], SYNC, "{data}");
    // 1300 is another process now: the same pid, a later start.
    let stat = s.0.join("proc/1300/stat");
    let text = std::fs::read_to_string(&stat).unwrap();
    std::fs::write(&stat, text.replace(" 8 0 2600 ", " 8 0 9999 ")).unwrap();
    let v = confirm(
        &a,
        &root,
        Action::Quit,
        pid(1300),
        data["plan"].as_str().unwrap(),
        Some(&history),
    )
    .unwrap();
    assert!(rec.lines().is_empty(), "nothing ran: {:?}", rec.lines());
    assert_eq!(pids(&v["skipped"]), [1300]);
    let rows = history
        .lock()
        .unwrap()
        .events(Some("action"), 0, i64::MAX)
        .unwrap();
    let results = &rows[0]["data"]["results"];
    let late: Vec<&Value> = results
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["result"] == "skipped")
        .collect();
    assert_eq!(
        late,
        [&json!({"pid": 1300, "name": "example-sync", "signal": "TERM", "result": "skipped"})],
        "{results}"
    );
    assert!(rows[0]["data"].get("unit").is_none(), "no unit was stopped");
}

#[test]
fn a_confirmation_whose_second_clock_read_fails_spends_its_plan() {
    // Review of audit 2026-11-21, L15: a confirmation that cannot read the
    // clock again once its targets are resolved fails as a refused
    // resolve does, and its plan is spent all the same: it was for this
    // subject.
    let root = fixture("root-desktop");
    let reads = Arc::new(AtomicU64::new(0));
    let clock: actions::Clock = {
        let reads = Arc::clone(&reads);
        Arc::new(move || match reads.fetch_add(1, Ordering::SeqCst) {
            0 | 1 => Ok(Duration::from_secs(1000)),
            _ => Err(std::io::Error::other("no clock")),
        })
    };
    let rec = Arc::new(Recording::default());
    let a = Actions::with_clock(rec.clone(), clock);
    let data = plan(&a, &root, Action::Quit, pid(7100));
    let f = confirm(
        &a,
        &root,
        Action::Quit,
        pid(7100),
        data["plan"].as_str().unwrap(),
        None,
    )
    .unwrap_err();
    assert_eq!(f.code, Code::Failed);
    assert_eq!(f.message, mirqab_core::proto::strings::ACT_NO_CLOCK);
    assert_eq!(
        reads.load(Ordering::SeqCst),
        3,
        "read again after resolving"
    );
    assert!(rec.lines().is_empty(), "nothing sent");
    assert_eq!(a.live_plans(), 0, "the plan is spent");
}
