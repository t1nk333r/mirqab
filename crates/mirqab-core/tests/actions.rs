//! Quit and Force quit on fixture roots (plan 006, part 2, "Tests"), with
//! the recording sender: nothing here signals or runs anything. The
//! refusals of architecture §5.4 and decision 5; `wouldDo` lists leaders
//! first with start times; the two-step flow; targets re-computed at the
//! second call; a group with a refused member; a rootless container; a
//! thread id; a user service; the `action` event; and a fixture root never
//! gets the real sender.

use mirqab_core::Root;
use mirqab_core::act::{self, Recording};
use mirqab_core::history::History;
use mirqab_core::proto::Code;
use mirqab_core::samplers::apps::actions::{self, Action, Actions, Failure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
