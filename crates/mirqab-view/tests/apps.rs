//! The Apps tab's view model (plan 006, part 3, Tests) on
//! `tests/golden/apps.json`, the daemon's view with `expand: true`: each
//! sort matches the daemon's own, search, flatten, open and closed groups,
//! the "more" line, the main process, the card's five, the merge rule, row
//! keys across frames (D5), and the confirmation's and the result's lines.

use mirqab_core::samplers::apps::view;
use mirqab_proto::stream::{AppGroup, AppProcess, AppsData};
use mirqab_proto::strings as s;
use mirqab_view::tabs::apps::{self, AppsState, Key, Row, Sort};
use serde_json::{Value, json};

fn golden() -> Value {
    serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap()
}

fn data(v: &Value) -> AppsData {
    serde_json::from_value(v.clone()).unwrap()
}

fn state() -> AppsState {
    AppsState::default()
}

/// Group ids of the rows, and pids of process rows, in order.
fn ids(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Row::Group(g) => g.id.clone(),
            Row::Process(p) => p.pid.to_string(),
            Row::Note(n) => format!("note:{n}"),
        })
        .collect()
}

#[test]
fn each_sort_is_the_daemons_order() {
    for (sort, word) in [(Sort::Cpu, "cpu"), (Sort::Mem, "mem"), (Sort::Name, "name")] {
        let daemon = data(&view(
            &golden(),
            &json!({"sort": word, "limit": 1000, "expand": true}),
        ));
        let mut st = state();
        let (frame, _) = st.take(data(&golden()));
        let ours: Vec<&str> = apps::sorted(&frame, sort)
            .iter()
            .map(|(_, g)| g.id.as_str())
            .collect();
        let theirs: Vec<&str> = daemon.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ours, theirs, "groups by {word}");
        assert_eq!(*ours.last().unwrap(), "system", "System last by {word}");
        for g in &daemon.groups {
            let (_, mine) = frame.groups.iter().find(|(_, m)| m.id == g.id).unwrap();
            let ours: Vec<u32> = apps::sorted_processes(mine, sort)
                .iter()
                .map(|p| p.pid)
                .collect();
            let theirs: Vec<u32> = g.processes.iter().flatten().map(|p| p.pid).collect();
            assert_eq!(ours, theirs, "processes of {} by {word}", g.id);
        }
    }
}

#[test]
fn search_finds_a_group_by_its_name_a_member_or_a_pid() {
    let mut st = state();
    let (frame, _) = st.take(data(&golden()));
    let all = apps::rows(&frame, &st, "");
    assert_eq!(
        all.iter().filter(|r| matches!(r, Row::Group(_))).count(),
        24
    );
    assert!(all.iter().all(|r| !matches!(r, Row::Process(_))), "closed");
    assert!(matches!(all.last(), Some(Row::Group(g)) if g.id == "system"));

    let by_name = apps::rows(&frame, &st, "Browser");
    assert_eq!(ids(&by_name), ["scope:app-Hyprland-browser.sh-5650fedc"]);

    // A member's name: the group is listed with that member under it.
    let by_member = apps::rows(&frame, &st, "rustc");
    assert_eq!(ids(&by_member), ["session:3030", "3041"]);
    let Row::Group(g) = &by_member[0] else {
        panic!()
    };
    assert!(g.open, "shown open, so the match is visible");

    // A pid, by its first digits.
    let by_pid = apps::rows(&frame, &st, "2011");
    assert_eq!(
        ids(&by_pid),
        ["scope:app-Hyprland-browser.sh-5650fedc", "2011"]
    );

    let none = apps::rows(&frame, &st, "no such thing");
    assert_eq!(ids(&none), [format!("note:{}", s::APPS_EMPTY)]);
}

#[test]
fn an_open_group_lists_its_processes_and_flatten_lists_only_processes() {
    let mut st = state();
    let (frame, _) = st.take(data(&golden()));
    let browser = frame
        .groups
        .iter()
        .find(|(_, g)| g.name == "Example Browser")
        .unwrap()
        .0;
    st.toggle_open(browser);
    let rows = apps::rows(&frame, &st, "");
    let i = rows
        .iter()
        .position(|r| matches!(r, Row::Group(g) if g.key == browser))
        .unwrap();
    assert_eq!(
        ids(&rows[i..i + 6]),
        [
            "scope:app-Hyprland-browser.sh-5650fedc",
            "2001",
            "2010",
            "2011",
            "2012",
            "2000"
        ]
    );
    let Row::Group(g) = &rows[i] else { panic!() };
    assert!(g.open && g.expandable);
    assert_eq!(g.procs, "5 processes");
    assert_eq!(g.icon.as_deref(), Some("example-browser"));
    assert_eq!(g.glyph, mirqab_view::icons::KIND_APP);
    assert_eq!(g.cpu, "66%");
    assert!((g.fraction - 0.66).abs() < 1e-9);
    st.toggle_open(browser);
    assert!(
        !apps::rows(&frame, &st, "")
            .iter()
            .any(|r| matches!(r, Row::Process(_)))
    );

    // A single-process group has no expander.
    let rows = apps::rows(&frame, &st, "pipewire");
    let Row::Group(g) = &rows[0] else { panic!() };
    assert!(!g.expandable);

    st.flatten = true;
    let flat = apps::rows(&frame, &st, "");
    assert_eq!(flat.len(), 44, "every process, no group rows");
    let Row::Process(first) = &flat[0] else {
        panic!()
    };
    assert_eq!((first.pid, first.name.as_str()), (3041, "rustc"));
    assert_eq!(first.group_name.as_deref(), Some("in cargo"));
    let Row::Process(last) = flat.last().unwrap() else {
        panic!()
    };
    assert_eq!(last.group_name.as_deref(), Some("in System"), "System last");
    // Searching the flat list by a group's name keeps its processes.
    assert_eq!(apps::rows(&frame, &st, "example browser").len(), 5);
}

#[test]
fn a_cut_list_ends_with_the_more_line() {
    let cut = view(&golden(), &json!({"limit": 3, "expand": true}));
    let mut st = state();
    let (frame, _) = st.take(data(&cut));
    // Three groups and System, which comes outside the limit.
    assert_eq!(frame.more, 20);
    let rows = apps::rows(&frame, &st, "");
    assert_eq!(rows.last(), Some(&Row::Note(s::apps_more(20))));
}

#[test]
fn a_groups_main_process_is_the_member_whose_parent_is_not_one() {
    let d = data(&golden());
    let main = |id: &str| apps::main_pid(d.groups.iter().find(|g| g.id == id).unwrap());
    assert_eq!(main("scope:app-Hyprland-browser.sh-5650fedc"), Some(2000));
    assert_eq!(main("session:3030"), Some(3030));
    // Several members whose parents are outside: the lowest pid.
    assert_eq!(main("system"), Some(1));
}

#[test]
fn the_card_shows_the_five_heaviest_system_included_when_heavy() {
    let mut d = data(&golden());
    let top = apps::top_five(&d.groups);
    let names: Vec<&str> = top.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["cargo", "Example Browser", "vite", "devserver", "Hyprland"]
    );
    let sys = d.groups.iter_mut().find(|g| g.id == "system").unwrap();
    sys.cpu = Some(80.0);
    let top = apps::top_five(&d.groups);
    assert_eq!(top[1].id, "system", "a root daemon eating a core shows");
    assert_eq!(top.len(), 5);
}

#[test]
fn params_merge_to_the_largest_view() {
    let page = apps::page_params();
    assert_eq!(apps::merge_params(&[Value::Null]), json!({"limit": 50}));
    assert_eq!(
        apps::merge_params(&[Value::Null, page.clone()]),
        json!({"limit": 1000, "expand": true})
    );
    assert_eq!(
        apps::merge_params(&[json!({"expand": ["a"]}), json!({"expand": ["b", "a"]})]),
        json!({"limit": 50, "expand": ["a", "b"]})
    );
    assert_eq!(
        apps::merge_params(&[json!({"expand": ["a"]}), json!({"expand": true})]),
        json!({"limit": 50, "expand": true}),
        "true wins"
    );
    assert_eq!(
        apps::merge_params(&[json!({"limit": 10, "sort": "name"}), Value::Null]),
        json!({"limit": 50}),
        "sort is never sent; absent counts as the daemon's 50"
    );
}

fn group(id: &str, procs: &[(u32, u64)]) -> AppGroup {
    AppGroup {
        id: id.into(),
        name: id.into(),
        kind: "terminal".into(),
        procs: procs.len() as u32,
        pids: procs.iter().map(|p| p.0).collect(),
        processes: Some(
            procs
                .iter()
                .map(|&(pid, start)| AppProcess {
                    pid,
                    start,
                    name: format!("p{pid}"),
                    ..Default::default()
                })
                .collect(),
        ),
        ..Default::default()
    }
}

fn frame_of(groups: Vec<AppGroup>) -> AppsData {
    AppsData {
        totals: mirqab_proto::stream::AppsTotals {
            groups: groups.len() as u32,
            ..Default::default()
        },
        groups,
        ..Default::default()
    }
}

#[test]
fn keys_follow_the_processes_not_the_ids() {
    let mut st = state();
    let (f1, _) = st.take(frame_of(vec![
        group("session:10", &[(100, 1), (101, 2)]),
        group("system", &[(1, 1)]),
    ]));
    let (k_session, k_system) = (f1.groups[0].0, f1.groups[1].0);
    st.toggle_open(k_session);
    st.selected = Some(Key::Group(k_session));

    // One member left, one joined: the same row.
    let (f2, lost) = st.take(frame_of(vec![
        group("session:10", &[(101, 2), (102, 3)]),
        group("system", &[(2, 5)]),
    ]));
    assert!(!lost);
    assert_eq!(f2.groups[0].0, k_session);
    assert_eq!(f2.groups[1].0, k_system, "System always keeps its key");
    assert!(st.open.contains(&k_session));

    // The same id, no shared member (a new session reused the sid): a new
    // row, closed and unselected.
    let (f3, lost) = st.take(frame_of(vec![group("session:10", &[(300, 9)])]));
    assert!(lost, "the selection's row is gone");
    assert_ne!(f3.groups[0].0, k_session);
    assert!(st.open.is_empty());
    assert_eq!(st.selected, None);

    // A frame cut for another consumer (no processes listed, pids only)
    // keeps the rows: it can arrive while the page's subscription replaces
    // the card's.
    let (f4, lost) = st.take(frame_of(vec![group("session:10", &[(300, 9)])]));
    let k = f4.groups[0].0;
    let mut cut = group("session:10", &[(300, 9)]);
    cut.processes = None;
    let (f5, _) = st.take(frame_of(vec![cut]));
    assert_eq!(f5.groups[0].0, k);
    let (f6, _) = st.take(frame_of(vec![group("session:10", &[(300, 9)])]));
    assert_eq!(f6.groups[0].0, k);
    assert!(!lost);
    let mut cut = frame_of(vec![group("session:10", &[(300, 9)])]);
    assert!(apps::expanded(&cut));
    cut.groups[0].processes = None;
    assert!(!apps::expanded(&cut), "the page skips such a frame");

    // A pid with a new start is another process row.
    st.selected = Some(Key::Process { pid: 300, start: 9 });
    let (_, lost) = st.take(frame_of(vec![group("session:10", &[(300, 10)])]));
    assert!(lost);
}

#[test]
fn a_pending_group_from_the_card_is_selected_on_the_next_frame() {
    let mut st = state();
    st.pending_group = Some("session:10".into());
    let (f, lost) = st.take(frame_of(vec![group("session:10", &[(100, 1)])]));
    assert!(!lost);
    assert_eq!(st.selected, Some(Key::Group(f.groups[0].0)));
    assert_eq!(st.pending_group, None);
}

#[test]
fn a_selection_says_what_to_inspect_and_whether_to_offer_quit() {
    let mut st = state();
    let (frame, _) = st.take(data(&golden()));
    let browser = frame
        .groups
        .iter()
        .find(|(_, g)| g.name == "Example Browser")
        .unwrap()
        .0;
    let sel = apps::selection(&frame, Key::Group(browser), 2011).unwrap();
    assert_eq!(sel.inspect_pid, Some(2000));
    assert_eq!(
        sel.args,
        json!({"group": "scope:app-Hyprland-browser.sh-5650fedc"})
    );
    assert_eq!(sel.targets.len(), 5);
    assert!(sel.holds_self, "the interface's own pid is in it");
    assert!(!sel.not_yours);

    let p = apps::selection(
        &frame,
        Key::Process {
            pid: 2001,
            start: 5010,
        },
        1,
    )
    .unwrap();
    assert_eq!(p.args, json!({"pid": 2001}));
    assert_eq!(p.process, Some((2001, 5010)));
    assert!(!p.holds_self);
    assert!(
        apps::selection(
            &frame,
            Key::Process {
                pid: 2001,
                start: 1
            },
            1
        )
        .is_none()
    );

    let sys = frame
        .groups
        .iter()
        .find(|(_, g)| g.id == "system")
        .unwrap()
        .0;
    assert!(
        apps::selection(&frame, Key::Group(sys), 1)
            .unwrap()
            .not_yours
    );
    let root = apps::selection(
        &frame,
        Key::Process {
            pid: 410,
            start: 300,
        },
        1,
    )
    .unwrap();
    assert!(root.not_yours, "a process of the System group");
}

#[test]
fn the_confirmation_lists_every_target_in_words() {
    let data = json!({
        "plan": "p1",
        "wouldDo": [
            {"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser"},
            {"signal": "TERM", "pid": 2010, "start": 5100, "name": "x\u{1b}[2J", "via": 2011},
            {"run": ["systemctl", "--user", "stop", "--no-block", "--", "pipewire.service"],
             "unit": "pipewire.service", "pids": [1210],
             "alsoStops": [{"unit": "wireplumber.service", "pids": [1211]},
                           {"unit": "sound.target", "pids": []}]},
            {"run": ["systemctl", "--user", "stop", "--no-block", "--", "a.service"],
             "unit": "a.service", "pids": [1, 2], "alsoStopsUnknown": true},
            {"run": ["systemctl", "--user", "kill", "--signal=KILL", "--", "a.service"],
             "unit": "a.service", "pids": [1, 2]},
        ],
        "skipped": [{"pid": 1050, "name": "Hyprland", "reason": "pid 1050 is the session's compositor"}],
        "warnings": ["pipewire.service is part of your sound"],
    });
    let lines = apps::confirm_lines(&data);
    assert_eq!(
        lines,
        [
            "browser (2001)",
            "x\u{fffd}[2J (2010), asked for its thread 2011",
            "Stops pipewire.service with systemctl (1 process)",
            "  pids 1210",
            "  Also stops wireplumber.service (1 process)",
            "    pids 1211",
            "  Also stops sound.target (0 processes) and what depends on it",
            "Stops a.service with systemctl (2 processes)",
            "  pids 1 2",
            "  Other services that depend on it may stop too",
            "Then ends whatever is left of a.service at once (SIGKILL)",
            "Left alone:",
            "  Hyprland (1050): pid 1050 is the session's compositor",
            "Warning: pipewire.service is part of your sound",
        ]
    );
    for l in &lines {
        assert!(!l.chars().any(char::is_control), "{l:?}");
    }
    assert!(!apps::start_differs(&data, 2001, 5010));
    assert!(apps::start_differs(&data, 2001, 4000), "a reused pid");
    assert!(!apps::start_differs(&data, 7, 1), "a pid it does not name");
}

#[test]
fn the_result_is_one_line_in_words() {
    let done = json!({"done": [{"signal": "TERM", "pid": 1, "name": "a"}], "gone": [],
                      "failed": [], "skipped": [], "recorded": true});
    assert_eq!(apps::result_lines(&done), [s::APPS_DONE]);
    assert!(!apps::not_recorded(&done));
    let gone = json!({"done": [], "gone": [{"pid": 1, "start": 2}], "failed": [], "skipped": []});
    assert_eq!(apps::result_lines(&gone), [s::APPS_ALREADY_GONE]);
    let failed = json!({"done": [], "gone": [], "skipped": [], "recorded": false,
        "failed": [{"pid": 7, "name": "b\nc", "error": "no"},
                   {"run": ["systemctl", "--user", "stop", "--", "x.service"], "error": "boom"}]});
    let lines = apps::result_lines(&failed);
    assert_eq!(
        lines,
        [
            "failed b\u{fffd}c (7): no",
            "failed systemctl --user stop -- x.service: boom"
        ]
    );
    assert!(apps::not_recorded(&failed));
}

#[test]
fn an_icon_is_a_theme_name_never_a_path() {
    assert_eq!(apps::icon_name(Some("firefox")).as_deref(), Some("firefox"));
    assert_eq!(apps::icon_name(Some("/usr/share/pixmaps/x.png")), None);
    assert_eq!(apps::icon_name(Some("")), None);
    assert_eq!(apps::icon_name(None), None);
    assert!(apps::deeper("sound.target"));
    assert!(!apps::deeper("a.service") && !apps::deeper("b.scope") && !apps::deeper("c.slice"));
}

#[test]
fn an_entry_of_unknown_shape_is_shown_as_sent() {
    let lines = apps::confirm_lines(&json!({"wouldDo": [{"foo": "bar"}]}));
    assert_eq!(lines, ["foo bar"]);
    assert!(!lines[0].contains("(0)"), "never a process with pid 0");
}

#[test]
fn an_outcome_outlives_its_row_and_says_when_unrecorded() {
    let key = Key::Group(7);
    let reply = Ok(
        json!({"done": [{"signal": "TERM", "pid": 1, "start": 2, "name": "a"}],
                          "gone": [], "failed": [], "skipped": [], "recorded": false}),
    );
    let o = apps::outcome(key, "Example\nBrowser", &reply);
    assert_eq!(o.key, key);
    assert_eq!(o.lines, [s::APPS_DONE]);
    assert!(o.warn);
    assert_eq!(
        apps::outcome_notice(&o),
        format!("Example\u{fffd}Browser: Done. {}", s::APPS_NOT_RECORDED)
    );
    let err = Err(mirqab_proto::ErrorBody {
        code: mirqab_proto::Code::Refused,
        message: "pid 1 is never signalled".into(),
        data: None,
    });
    let o = apps::outcome(key, "x", &err);
    assert!(!o.warn);
    assert_eq!(apps::outcome_notice(&o), "x: pid 1 is never signalled");
}

#[test]
fn a_quit_watches_only_what_was_signalled() {
    let reply = json!({
        "done": [{"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser"},
                 {"signal": "TERM", "pid": 2010, "start": 5100, "name": "Web Content"}],
        "gone": [{"pid": 2011, "start": 5200}],
        "skipped": [{"pid": 2000, "name": "bash", "reason": "the program asking"}],
        "failed": [],
    });
    let all = [(2000, 5000), (2001, 5010), (2010, 5100), (2011, 5200)];
    assert_eq!(apps::watched(&reply, &all), [(2001, 5010), (2010, 5100)]);
    // A service's stop names no processes: its targets, less the skipped.
    let stop = json!({"done": [{"run": ["systemctl"], "unit": "a.service"}],
                      "skipped": [{"pid": 2000, "name": "bash", "reason": "r"}]});
    assert_eq!(
        apps::watched(&stop, &all),
        [(2001, 5010), (2010, 5100), (2011, 5200)]
    );
}
