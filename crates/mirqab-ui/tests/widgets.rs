//! Widget tests on a headless display (architecture §7.5, layer 2). They
//! need GTK and a display, so a plain `cargo test` skips them;
//! tests/test-ui-widgets.sh runs them under Broadway with `--ignored`.
//! Nothing compares pixels: the tests read the render node tree.

use gtk::glib;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use mirqab_proto::{Code, ErrorBody};
use mirqab_ui::client::{Reply, Source};
use mirqab_ui::widgets::confirm_dialog;
use mirqab_ui::widgets::graph::{Graph, Live};
use mirqab_ui::widgets::meter::Meter;
use serde_json::{Value, json};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

const DISPLAY: &str = "needs a display: run tests/test-ui-widgets.sh";

/// Runs the main loop until `done`, failing after ten seconds (a loaded CI
/// runner can take a while to map a window).
fn settle(what: &str, done: impl Fn() -> bool) {
    let ctx = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        while ctx.iteration(false) {}
        std::thread::sleep(Duration::from_millis(5));
    }
    while ctx.iteration(false) {}
}

/// A window destroyed when the test ends, even by a failed assertion, so
/// one failure does not leave a toplevel behind for the next test.
struct Shown(gtk::Window);

impl Drop for Shown {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

/// What the widget's own snapshot() appends, at its allocated size.
fn render<T>(w: &T) -> Option<gsk::RenderNode>
where
    T: ObjectSubclassIsExt,
    T::Subclass: WidgetImpl,
{
    let s = gtk::Snapshot::new();
    WidgetImpl::snapshot(w.imp(), &s);
    s.to_node()
}

/// Every node in the tree under `node`.
fn nodes(node: &gsk::RenderNode, out: &mut Vec<gsk::RenderNode>) {
    out.push(node.clone());
    let mut children = Vec::new();
    if let Some(c) = node.downcast_ref::<gsk::ContainerNode>() {
        children.extend((0..c.n_children()).map(|i| c.child(i)));
    } else if let Some(n) = node.downcast_ref::<gsk::TransformNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::ClipNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::RoundedClipNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::OpacityNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::DebugNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::StrokeNode>() {
        children.push(n.child());
    } else if let Some(n) = node.downcast_ref::<gsk::FillNode>() {
        children.push(n.child());
    }
    for c in children {
        nodes(&c, out);
    }
}

fn all<T>(w: &T) -> Vec<gsk::RenderNode>
where
    T: ObjectSubclassIsExt,
    T::Subclass: WidgetImpl,
{
    let mut out = Vec::new();
    if let Some(n) = render(w) {
        nodes(&n, &mut out);
    }
    out
}

fn count(nodes: &[gsk::RenderNode], t: gsk::RenderNodeType) -> usize {
    nodes.iter().filter(|n| n.node_type() == t).count()
}

/// The contours (move-to operations) of the first stroke's path.
fn contours(nodes: &[gsk::RenderNode]) -> usize {
    let Some(stroke) = nodes
        .iter()
        .find_map(|n| n.downcast_ref::<gsk::StrokeNode>().cloned())
    else {
        return 0;
    };
    let mut moves = 0;
    stroke
        .path()
        .foreach(gsk::PathForeachFlags::empty(), |op, _, _, _| {
            if *op == gsk::PathOperation::Move {
                moves += 1;
            }
            glib::ControlFlow::Continue
        });
    moves
}

fn no_blur(nodes: &[gsk::RenderNode]) {
    assert_eq!(
        count(nodes, gsk::RenderNodeType::BlurNode),
        0,
        "a blur node"
    );
}

/// A widget in a presented window, sized and laid out.
fn shown(w: &impl IsA<gtk::Widget>, width: i32, height: i32) -> Shown {
    let win = gtk::Window::new();
    w.set_size_request(width, height);
    win.set_child(Some(w));
    win.present();
    settle("the widget is mapped and allocated", || {
        w.width() > 0 && w.is_mapped()
    });
    Shown(win)
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn a_series_is_one_stroke_and_one_fill_and_a_gap_is_a_second_contour() {
    let _ = DISPLAY;
    let g = Graph::new();
    g.set_end(Some(3600));
    let win = shown(&g, 400, 100);

    g.set_points((0..10).map(|i| (3000 + i * 60, i as f64)).collect(), vec![]);
    let n = all(&g);
    assert_eq!(count(&n, gsk::RenderNodeType::StrokeNode), 1);
    assert_eq!(count(&n, gsk::RenderNodeType::FillNode), 1);
    assert_eq!(contours(&n), 1);
    no_blur(&n);

    // Buckets 3 to 5 missing: the line breaks, in the same path.
    let gap: Vec<(i64, f64)> = (0..10)
        .filter(|i| !(3..6).contains(i))
        .map(|i| (3000 + i * 60, 1.0))
        .collect();
    g.set_points(gap, vec![]);
    let n = all(&g);
    assert_eq!(count(&n, gsk::RenderNodeType::StrokeNode), 1);
    assert_eq!(contours(&n), 2);

    g.set_points(vec![], vec![]);
    assert_eq!(count(&all(&g), gsk::RenderNodeType::StrokeNode), 0);

    // Peaks are one more stroke.
    let pts: Vec<(i64, f64)> = (0..10).map(|i| (3000 + i * 60, 1.0)).collect();
    let peaks: Vec<(i64, f64)> = pts.iter().map(|&(t, v)| (t, v * 2.0)).collect();
    g.set_points(pts, peaks);
    assert_eq!(count(&all(&g), gsk::RenderNodeType::StrokeNode), 2);

    // A spike far above the averages stays visible: peaks set the shared
    // scale, so the averages' line sits low and the peaks' line is not
    // flattened onto the top edge.
    let avg: Vec<(i64, f64)> = (0..10).map(|i| (3000 + i * 60, 10.0)).collect();
    let mut spikes: Vec<(i64, f64)> = avg.iter().map(|&(t, _)| (t, 12.0)).collect();
    spikes[5].1 = 95.0;
    g.set_points(avg, spikes);
    let strokes: Vec<gsk::StrokeNode> = all(&g)
        .iter()
        .filter_map(|n| n.downcast_ref::<gsk::StrokeNode>().cloned())
        .collect();
    // (top, bottom) of a stroke's path, in widget coordinates.
    let span = |s: &gsk::StrokeNode| {
        let r = s.path().bounds().expect("a non-empty path");
        (r.y(), r.y() + r.height())
    };
    let (avg_top, _) = span(&strokes[0]);
    let (peak_top, peak_low) = span(&strokes[1]);
    assert!(
        avg_top > 50.0,
        "averages sit low on a 0–100 scale: {avg_top}"
    );
    assert!(
        peak_low - peak_top > 50.0,
        "the spike stands out from the other peaks"
    );
    drop(win);
}

type OnEvent = Rc<dyn Fn(&Value)>;

/// A `Source` that counts its subscriptions, keeps the live params and
/// every request, and answers requests from a queue.
#[derive(Default)]
struct Fake {
    live: Rc<Cell<i32>>,
    plain: Rc<Cell<i32>>,
    on_event: RefCell<Option<OnEvent>>,
    live_params: RefCell<Vec<Value>>,
    requests: RefCell<Vec<(String, Value)>>,
    answers: RefCell<std::collections::VecDeque<Result<Value, ErrorBody>>>,
    /// While set, a request's reply is held in `held` instead of answered.
    defer: Cell<bool>,
    held: RefCell<Option<Reply>>,
    /// Answers to `get`, in order; an inspection of `browser` when empty.
    get_answers: RefCell<std::collections::VecDeque<Result<Value, ErrorBody>>>,
}

impl Fake {
    fn new() -> Fake {
        Fake::default()
    }
}

struct Held(Rc<Cell<i32>>);
impl Drop for Held {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Source for Fake {
    fn subscribe(&self, _: &str, _: u64, _: Rc<dyn Fn(&Value)>) -> Box<dyn Any> {
        self.plain.set(self.plain.get() + 1);
        Box::new(Held(Rc::clone(&self.plain)))
    }
    fn subscribe_live(&self, _: &str, params: Value, on_event: Rc<dyn Fn(&Value)>) -> Box<dyn Any> {
        self.live.set(self.live.get() + 1);
        self.live_params.borrow_mut().push(params);
        *self.on_event.borrow_mut() = Some(on_event);
        Box::new(Held(Rc::clone(&self.live)))
    }
    fn request(&self, method: &str, params: Value, on_reply: Reply) {
        self.requests
            .borrow_mut()
            .push((method.to_string(), params));
        if self.defer.get() {
            *self.held.borrow_mut() = Some(on_reply);
            return;
        }
        let answer = self.answers.borrow_mut().pop_front();
        on_reply(answer.unwrap_or_else(|| {
            Err(ErrorBody {
                code: Code::Unavailable,
                message: "no answer queued".into(),
                data: None,
            })
        }));
    }
    fn get(&self, stream: &str, params: Value, on_reply: Reply) {
        self.requests
            .borrow_mut()
            .push((format!("get {stream}"), params.clone()));
        if let Some(answer) = self.get_answers.borrow_mut().pop_front() {
            on_reply(answer);
            return;
        }
        on_reply(Ok(json!({
            "pid": params["pid"],
            "name": "browser",
            "verdict": {"code": "home", "text": "Not owned by any package"},
        })));
    }
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn live_only_while_mapped_at_two_minutes() {
    let fake = Rc::new(Fake::new());
    let g = Graph::new();
    g.set_live(Some(Live {
        source: fake.clone(),
        stream: "system".into(),
        params: Value::Null,
        extract: Rc::new(|v: &Value| v["cpu"]["percent"].as_f64()),
    }));
    g.set_range("2m");
    assert_eq!(fake.live.get(), 0, "not mapped: no live subscription");

    let win = shown(&g, 400, 100);
    assert_eq!(fake.live.get(), 1, "mapped at 2m");
    assert!(g.is_live());

    // A live event becomes a point on the ring and is drawn.
    let cb = fake.on_event.borrow().clone().unwrap();
    cb(&json!({"cpu": {"percent": 40.0}}));
    cb(&json!({"cpu": {"percent": 60.0}}));
    assert_eq!(count(&all(&g), gsk::RenderNodeType::StrokeNode), 1);
    // Two samples in the same second: the second replaces the first.
    g.push_live(100, 1.0);
    g.push_live(100, 2.0);
    g.push_live(101, 3.0);
    assert_eq!(g.ring_len(), 2);

    g.set_range("1h");
    assert_eq!(fake.live.get(), 0, "a stored range drops live");
    g.set_range("2m");
    assert_eq!(fake.live.get(), 1);
    win.0.set_visible(false);
    settle("the graph is unmapped", || !g.is_mapped());
    assert_eq!(fake.live.get(), 0, "a hidden window drops live");
    win.0.present();
    settle("the graph is mapped again", || g.is_mapped());
    assert_eq!(fake.live.get(), 1, "shown again");
    g.set_live(None);
    assert_eq!(fake.live.get(), 0);
    assert_eq!(fake.plain.get(), 0, "the graph never takes a plain one");
    drop(win);
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_meter_draws_track_and_fill_in_a_rounded_clip() {
    let m = Meter::new();
    let _win = shown(&m, 200, 10);
    m.set_fraction(0.5, "50%");
    let n = all(&m);
    assert_eq!(count(&n, gsk::RenderNodeType::RoundedClipNode), 1);
    assert_eq!(count(&n, gsk::RenderNodeType::ColorNode), 2);
    no_blur(&n);
    m.set_fraction(0.0, "0%");
    assert_eq!(count(&all(&m), gsk::RenderNodeType::ColorNode), 1);
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_confirm_dialog_lists_every_would_do_line() {
    let would = [
        json!({"signal": "TERM", "pid": 4242, "name": "vite"}),
        json!({"remove": "/tmp/x"}),
    ];
    let lines = confirm_dialog::lines(&would);
    assert_eq!(lines, ["name vite, pid 4242, signal TERM", "remove /tmp/x"]);
    let d = confirm_dialog::confirm_dialog("Stop vite?", &would);
    use adw::prelude::*;
    assert_eq!(d.body().as_str(), lines.join("\n"));
    assert!(d.has_response(confirm_dialog::CONFIRM));
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn moving_a_home_card_keeps_its_graph_live() {
    use mirqab_ui::tabs::home::page::place;
    let fake = Rc::new(Fake::new());
    let graph = || {
        let g = Graph::new();
        g.set_live(Some(Live {
            source: fake.clone(),
            stream: "system".into(),
            params: Value::Null,
            extract: Rc::new(|v: &Value| v["cpu"]["percent"].as_f64()),
        }));
        g.set_range("2m");
        g.set_size_request(200, 60);
        g
    };
    let (a, b) = (graph(), graph());
    let grid = gtk::Grid::new();
    let wa: gtk::Widget = a.clone().upcast();
    let wb: gtk::Widget = b.clone().upcast();
    place(&grid, &[(wa.clone(), (0, 0, 1)), (wb.clone(), (0, 1, 1))]);
    a.set_focusable(true);
    let win = shown(&grid, 400, 100);
    settle("both graphs are mapped", || a.is_mapped() && b.is_mapped());
    a.grab_focus();
    settle("the first graph has focus", || {
        gtk::prelude::RootExt::focus(&win.0).as_ref() == Some(&wa)
    });
    a.push_live(100, 1.0);
    a.push_live(101, 2.0);
    b.push_live(100, 3.0);
    assert_eq!(fake.live.get(), 2);

    // Swapped: both stay mapped and keep their points.
    place(&grid, &[(wb.clone(), (0, 0, 1)), (wa.clone(), (0, 1, 1))]);
    assert_eq!((a.ring_len(), b.ring_len()), (2, 1));
    assert!(a.is_live() && b.is_live());
    assert_eq!(fake.live.get(), 2, "no live subscription was dropped");
    let lc = grid
        .layout_manager()
        .unwrap()
        .layout_child(&wa)
        .downcast::<gtk::GridLayoutChild>()
        .unwrap();
    assert_eq!(lc.column(), 1, "moved through its layout child");
    assert_eq!(
        gtk::prelude::RootExt::focus(&win.0),
        Some(wa.clone()),
        "a moved card keeps focus"
    );

    // b unpinned: removed; a untouched.
    place(&grid, &[(wa.clone(), (0, 0, 2))]);
    assert!(b.parent().is_none());
    assert_eq!(a.ring_len(), 2);
    assert_eq!(fake.live.get(), 1);
    drop(win);
}

/// The golden `apps` frame, keyed.
fn golden_frame() -> mirqab_view::tabs::apps::Frame {
    let v: Value = serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    let mut st = mirqab_view::tabs::apps::AppsState::default();
    st.take(serde_json::from_value(v).unwrap()).0
}

/// A widget named `name` under `w`.
fn named(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if w.widget_name() == name {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(hit) = named(&child, name) {
            return Some(hit);
        }
        c = child.next_sibling();
    }
    None
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_inspector_asks_before_it_quits_and_says_a_refusal_in_words() {
    use adw::prelude::*;
    use mirqab_ui::tabs::apps::inspector::Inspector;
    use mirqab_view::tabs::apps::{self as model, Key};
    let fake = Rc::new(Fake::new());
    let i = Inspector::new(
        fake.clone(),
        Rc::new(|| true),
        Rc::new(|| {}),
        Rc::new(|_| {}),
    );
    let win = shown(&i.root, 400, 600);
    let frame = golden_frame();

    // A process: the live graph follows `process` with its pid and start,
    // and the inspector asks `apps.inspect` once.
    let p = Key::Process {
        pid: 2001,
        start: 5010,
    };
    i.show(model::selection(&frame, p, 1), &frame, true);
    assert_eq!(fake.live.get(), 1, "a process is graphed live");
    assert_eq!(
        fake.live_params.borrow().last(),
        Some(&json!({"pid": 2001, "start": 5010}))
    );
    // The inspection names the row's start, so a reused pid is not
    // explained in its name (plan 006, part 4, decision 9).
    assert_eq!(
        fake.requests.borrow().last(),
        Some(&(
            "get apps.inspect".to_string(),
            json!({"pid": 2001, "start": 5010})
        ))
    );
    i.show(model::selection(&frame, p, 1), &frame, true);
    assert_eq!(fake.requests.borrow().len(), 1, "never on a timer");

    // A group: no live subscription; its main process is inspected.
    let browser = frame
        .groups
        .iter()
        .find(|(_, g)| g.name == "Example Browser")
        .unwrap()
        .0;
    i.show(
        model::selection(&frame, Key::Group(browser), 1),
        &frame,
        true,
    );
    assert_eq!(fake.live.get(), 0, "a group drops the live graph");
    assert_eq!(
        fake.requests.borrow().last().unwrap().1,
        json!({"pid": 2000, "start": 5000})
    );
    assert!(i.graph.ring_len() > 0, "fed from the frames instead");

    // Quit: the plan opens the dialog, which lists every pid.
    let would: Vec<Value> = [
        (2001, 5010),
        (2000, 5000),
        (2010, 5100),
        (2011, 5200),
        (2012, 5300),
    ]
    .iter()
    .map(|&(pid, start)| json!({"signal": "TERM", "pid": pid, "start": start, "name": "x"}))
    .collect();
    fake.answers.borrow_mut().push_back(Err(ErrorBody {
        code: Code::NeedsConfirm,
        message: "ask again".into(),
        data: Some(json!({"plan": "p1", "wouldDo": would, "skipped": [], "warnings": []})),
    }));
    let quit = named(i.root.upcast_ref(), "apps-quit")
        .and_downcast::<gtk::Button>()
        .unwrap();
    quit.emit_clicked();
    let group = json!({"group": "scope:app-Hyprland-browser.sh-5650fedc"});
    assert_eq!(
        fake.requests.borrow().last(),
        Some(&(
            "action".to_string(),
            json!({"name": "apps.quit", "args": group})
        )),
        "the first call has no confirm"
    );
    let d = i.dialog().expect("a dialog");
    for pid in ["2000", "2001", "2010", "2011", "2012"] {
        assert!(d.body().contains(pid), "{pid} in {}", d.body());
    }
    // Cancel is the default and the close response (review of PR #19).
    assert_eq!(d.default_response().as_deref(), Some("cancel"));
    assert_eq!(d.close_response(), "cancel");
    // Go ahead: the same args, confirm and the plan.
    fake.answers.borrow_mut().push_back(Ok(json!({
        "done": [{"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser"}],
        "gone": [], "failed": [],
        "skipped": [{"pid": 2000, "name": "bash", "reason": "r"}], "recorded": false
    })));
    d.emit_by_name::<()>("response", &[&"confirm"]);
    assert_eq!(
        fake.requests.borrow().last().unwrap().1,
        json!({"name": "apps.quit", "args": group, "confirm": true, "plan": "p1"})
    );
    assert!(i.result.text().contains(mirqab_proto::strings::APPS_DONE));
    assert!(i.warned(), "not recorded: said");
    assert_eq!(i.dialog(), None, "a dialog that answered is gone");
    assert_eq!(
        i.watching(),
        [(2001, 5010)],
        "still running watches what was signalled, not the member left alone"
    );
    // The row dies on the next frame: the outcome outlives it (B1).
    let mut st = mirqab_view::tabs::apps::AppsState::default();
    let v: Value = serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    let mut d2: mirqab_proto::stream::AppsData = serde_json::from_value(v).unwrap();
    d2.groups.retain(|g| g.name != "Example Browser");
    let frame2 = st.take(d2).0;
    i.show(None, &frame2, true);
    let o = i.take_outcome(Key::Group(browser)).expect("an outcome");
    assert!(o.warn);
    assert_eq!(
        o.lines,
        [
            mirqab_proto::strings::APPS_DONE,
            "Left alone:",
            "  bash (2000): r"
        ]
    );
    assert_eq!(o.name, "Example Browser");
    d.force_close();

    // A refusal is the daemon's message, and no dialog.
    let hypr = Key::Process {
        pid: 1050,
        start: 1500,
    };
    i.show(model::selection(&frame, hypr, 1), &frame, true);
    fake.answers.borrow_mut().push_back(Err(ErrorBody {
        code: Code::Refused,
        message: "pid 1050 is the session's compositor".into(),
        data: None,
    }));
    quit.emit_clicked();
    assert_eq!(i.result.text(), "pid 1050 is the session's compositor");
    assert_eq!(i.dialog(), None, "no dialog");

    // The System group offers no buttons.
    let sys = frame
        .groups
        .iter()
        .find(|(_, g)| g.id == "system")
        .unwrap()
        .0;
    i.show(model::selection(&frame, Key::Group(sys), 1), &frame, true);
    assert!(!i.buttons.is_visible());

    // Live only while mapped.
    i.show(model::selection(&frame, p, 1), &frame, true);
    assert_eq!(fake.live.get(), 1);
    win.0.set_visible(false);
    settle("the inspector is unmapped", || !i.graph.is_mapped());
    assert_eq!(fake.live.get(), 0, "a hidden window drops live");
    no_blur(&all(&i.graph));
    drop(win);
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn an_apps_row_draws_no_blur() {
    use mirqab_ui::tabs::apps::group_row::GroupRow;
    use mirqab_view::tabs::apps::{self as model, AppsState, Row};
    let frame = golden_frame();
    let rows = model::rows(&frame, &AppsState::default(), "");
    let Row::Group(g) = &rows[0] else { panic!() };
    let row = GroupRow::new(Rc::new(|_| {}));
    row.show(g);
    let _win = shown(&row.root, 600, 40);
    assert_eq!(row.root.widget_name(), "apps-group:session:3030");
    let mut meters = 0;
    let mut c = row.root.first_child();
    while let Some(w) = c {
        if let Some(m) = w.downcast_ref::<Meter>() {
            meters += 1;
            no_blur(&all(m));
        }
        c = w.next_sibling();
    }
    assert_eq!(meters, 1);
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_confirm_dialog_defaults_to_cancel() {
    use adw::prelude::*;
    let d = confirm_dialog::confirm_dialog_lines("Quit?", &["a (1)".to_string()]);
    assert_eq!(d.default_response().as_deref(), Some("cancel"));
    assert_eq!(d.close_response(), "cancel");
    assert_eq!(
        d.response_appearance(confirm_dialog::CONFIRM),
        adw::ResponseAppearance::Destructive
    );
}

/// The needs-confirm answer for `would` with plan `plan`.
fn needs_confirm(plan: &str, would: Value) -> Result<Value, ErrorBody> {
    Err(ErrorBody {
        code: Code::NeedsConfirm,
        message: "ask again".into(),
        data: Some(json!({"plan": plan, "wouldDo": would, "skipped": [], "warnings": []})),
    })
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn one_act_one_dialog_and_go_ahead_sends_what_it_showed() {
    use adw::prelude::*;
    use mirqab_ui::tabs::apps::inspector::Inspector;
    use mirqab_view::tabs::apps::{self as model, Key, Outcome};
    let fake = Rc::new(Fake::new());
    let outcomes: Rc<RefCell<Vec<Outcome>>> = Rc::new(RefCell::new(Vec::new()));
    let o = Rc::clone(&outcomes);
    let i = Inspector::new(
        fake.clone(),
        Rc::new(|| true),
        Rc::new(|| {}),
        Rc::new(move |x| o.borrow_mut().push(x)),
    );
    let _win = shown(&i.root, 400, 600);
    let frame = golden_frame();
    let quit = named(i.root.upcast_ref(), "apps-quit")
        .and_downcast::<gtk::Button>()
        .unwrap();
    let force = named(i.root.upcast_ref(), "apps-force-quit")
        .and_downcast::<gtk::Button>()
        .unwrap();
    let p = Key::Process {
        pid: 2001,
        start: 5010,
    };
    let other = Key::Process {
        pid: 2010,
        start: 5100,
    };
    let would = json!([{"signal": "TERM", "pid": 2001, "start": 5010, "name": "browser"}]);
    i.show(model::selection(&frame, p, 1), &frame, true);
    let base = fake.requests.borrow().len();

    // Only one act: a second Quit and a Force quit with the dialog open
    // send nothing and stack no second dialog (S3).
    fake.answers
        .borrow_mut()
        .push_back(needs_confirm("p1", would.clone()));
    quit.emit_clicked();
    let first = i.dialog().expect("a dialog");
    quit.emit_clicked();
    force.emit_clicked();
    assert_eq!(fake.requests.borrow().len(), base + 1, "one request");
    assert_eq!(i.dialog(), Some(first.clone()), "the first dialog");
    // Cancel sends nothing, and frees the next act.
    first.emit_by_name::<()>("response", &[&"cancel"]);
    assert_eq!(i.dialog(), None);
    assert_eq!(
        fake.requests.borrow().len(),
        base + 1,
        "cancel sends nothing"
    );
    fake.answers
        .borrow_mut()
        .push_back(needs_confirm("p2", would.clone()));
    quit.emit_clicked();
    assert_eq!(
        fake.requests.borrow().len(),
        base + 2,
        "a further click asks"
    );

    // The row goes while the dialog is open: it closes, and its Go ahead
    // sends nothing (S2).
    let d = i.dialog().expect("a dialog");
    i.show(None, &frame, true);
    assert_eq!(i.dialog(), None, "closed with its row");
    d.emit_by_name::<()>("response", &[&"confirm"]);
    assert_eq!(fake.requests.borrow().len(), base + 2, "nothing redeemed");

    // Another row selected while the dialog is open: closed too.
    i.show(model::selection(&frame, p, 1), &frame, true);
    fake.answers
        .borrow_mut()
        .push_back(needs_confirm("p3", would.clone()));
    quit.emit_clicked();
    assert!(i.dialog().is_some());
    i.show(model::selection(&frame, other, 1), &frame, true);
    assert_eq!(i.dialog(), None, "no redeem for a row the user left");

    // Go ahead sends the args the dialog showed; the reply comes after
    // the user moved on, and its outcome goes to the page (B1).
    i.show(model::selection(&frame, p, 1), &frame, true);
    fake.answers
        .borrow_mut()
        .push_back(needs_confirm("p4", would.clone()));
    quit.emit_clicked();
    let d = i.dialog().expect("a dialog");
    fake.defer.set(true);
    d.emit_by_name::<()>("response", &[&"confirm"]);
    fake.defer.set(false);
    assert_eq!(
        fake.requests.borrow().last().unwrap().1,
        json!({"name": "apps.quit", "args": {"pid": 2001, "start": 5010}, "confirm": true, "plan": "p4"})
    );
    i.show(model::selection(&frame, other, 1), &frame, true);
    let held = fake.held.borrow_mut().take().expect("a held reply");
    held(Ok(
        json!({"done": [{"signal": "TERM", "pid": 2001, "start": 5010}],
                   "gone": [], "failed": [], "skipped": [], "recorded": false}),
    ));
    let got = outcomes.borrow();
    assert_eq!(got.len(), 1, "the outcome went to the page once");
    assert_eq!(got[0].key, p);
    assert!(got[0].warn);
    assert_eq!(i.result.text(), "", "not shown under the other row");
    drop(got);

    // A plan naming the row's pid with another start: the pid was reused,
    // and nothing is asked (D5).
    i.show(model::selection(&frame, p, 1), &frame, true);
    let n = fake.requests.borrow().len();
    fake.answers.borrow_mut().push_back(needs_confirm(
        "p5",
        json!([{"signal": "TERM", "pid": 2001, "start": 9999, "name": "other"}]),
    ));
    quit.emit_clicked();
    assert_eq!(i.dialog(), None, "no dialog for a reused pid");
    assert_eq!(fake.requests.borrow().len(), n + 1, "only the first call");
    assert_eq!(i.result.text(), mirqab_proto::strings::APPS_ROW_GONE);
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn hiding_the_sheet_clears_the_selection() {
    use mirqab_view::tabs::apps::Key;
    adw::init().unwrap();
    let app = adw::Application::builder()
        .application_id("io.github.t1nk333r.mirqab.test")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    // A dead socket: nothing is sent, the page is only built.
    let dir = std::env::temp_dir().join(format!("mirqab-widgets-{}", std::process::id()));
    let ctx = mirqab_ui::app::Ctx::new(&app, dir.join("none.sock"));
    let page = mirqab_ui::tabs::apps::page::build(&ctx);
    fn split(w: &gtk::Widget) -> Option<adw::OverlaySplitView> {
        if let Some(s) = w.downcast_ref::<adw::OverlaySplitView>() {
            return Some(s.clone());
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            if let Some(s) = split(&child) {
                return Some(s);
            }
            c = child.next_sibling();
        }
        None
    }
    let split = split(&page).expect("the split view");
    assert!(!split.enables_hide_gesture(), "no swipe hides the sheet");
    split.set_show_sidebar(true);
    ctx.apps.borrow_mut().selected = Some(Key::Group(1));
    split.set_show_sidebar(false);
    assert_eq!(ctx.apps.borrow().selected, None, "hidden: nothing selected");
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn a_reused_pid_is_not_explained_and_a_tagged_group_says_why() {
    use mirqab_ui::tabs::apps::inspector::Inspector;
    use mirqab_view::tabs::apps::{self as model, Key};
    let fake = Rc::new(Fake::new());
    let i = Inspector::new(
        fake.clone(),
        Rc::new(|| true),
        Rc::new(|| {}),
        Rc::new(|_| {}),
    );
    let _win = shown(&i.root, 400, 600);
    let frame = golden_frame();
    let p = Key::Process {
        pid: 2001,
        start: 5010,
    };

    // The pid holds another process by the time it is asked: the sheet
    // says the row is gone and forms no verdict (decision 9).
    fake.get_answers.borrow_mut().push_back(Err(ErrorBody {
        code: Code::Failed,
        message: mirqab_proto::strings::apps_process_gone(2001),
        data: None,
    }));
    i.show(model::selection(&frame, p, 1), &frame, true);
    assert_eq!(
        fake.requests.borrow().last(),
        Some(&(
            "get apps.inspect".to_string(),
            json!({"pid": 2001, "start": 5010})
        ))
    );
    assert_eq!(i.result.text(), mirqab_proto::strings::APPS_ROW_GONE);
    let verdict = named(i.root.upcast_ref(), "apps-verdict")
        .and_downcast::<gtk::Label>()
        .unwrap();
    assert_eq!(verdict.text(), "", "no verdict");
    // Another failure is not the row gone.
    fake.get_answers.borrow_mut().push_back(Err(ErrorBody {
        code: Code::Failed,
        message: "no such stream".into(),
        data: None,
    }));
    let other = Key::Process {
        pid: 2010,
        start: 5100,
    };
    i.show(model::selection(&frame, other, 1), &frame, true);
    assert_eq!(i.result.text(), "");
    assert!(!i.idle.is_visible(), "a process row has no tag line");

    // A tagged group: the sheet's line under the stats; untagged, none.
    let mut st = model::AppsState::default();
    let v: Value = serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    let mut d: mirqab_proto::stream::AppsData = serde_json::from_value(v).unwrap();
    for g in &mut d.groups {
        g.idle_heavy = g.id == "session:3030";
    }
    let tagged = st.take(d).0;
    let key = |id: &str| tagged.groups.iter().find(|(_, g)| g.id == id).unwrap().0;
    i.show(
        model::selection(&tagged, Key::Group(key("session:3030")), 1),
        &tagged,
        true,
    );
    assert!(i.idle.is_visible());
    assert_eq!(i.idle.text(), mirqab_proto::strings::APPS_IDLE_HEAVY_LINE);
    i.show(
        model::selection(
            &tagged,
            Key::Group(key("scope:app-Hyprland-browser.sh-5650fedc")),
            1,
        ),
        &tagged,
        true,
    );
    assert!(!i.idle.is_visible());
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_caption_shows_only_on_a_tagged_row() {
    use mirqab_ui::tabs::apps::group_row::GroupRow;
    use mirqab_view::tabs::apps::{self as model, AppsState, Row};
    let frame = golden_frame();
    let rows = model::rows(&frame, &AppsState::default(), "");
    let Row::Group(g) = &rows[0] else { panic!() };
    let row = GroupRow::new(Rc::new(|_| {}));
    let _win = shown(&row.root, 600, 40);
    row.show(g);
    assert!(!row.idle.is_visible(), "untagged: no caption");
    let mut t = g.clone();
    t.idle_heavy = true;
    row.show(&t);
    assert!(row.idle.is_visible());
    assert_eq!(row.idle.text(), mirqab_proto::strings::APPS_IDLE_HEAVY);
    assert!(row.idle.has_css_class("dim-label") && row.idle.has_css_class("caption"));
    row.show(g);
    assert!(!row.idle.is_visible(), "rebound to an untagged group");
}

/// The Apps page on a `Ctx` with a dead socket, shown in a window, fed the
/// golden frame by hand.
fn apps_page() -> (
    Rc<mirqab_ui::app::Ctx>,
    gtk::Widget,
    mirqab_ui::tabs::apps::page::Handle,
    Shown,
) {
    adw::init().unwrap();
    let app = adw::Application::builder()
        .application_id("io.github.t1nk333r.mirqab.test")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let dir = std::env::temp_dir().join(format!("mirqab-widgets-{}", std::process::id()));
    let ctx = mirqab_ui::app::Ctx::new(&app, dir.join("none.sock"));
    let (page, handle) = mirqab_ui::tabs::apps::page::build_page(&ctx);
    let win = shown(&page, 1400, 700);
    let v: Value = serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    handle.frame(&v);
    settle("the rows are drawn", || {
        named(win.0.upcast_ref(), "apps-group:system").is_some_and(|w| w.is_mapped())
    });
    (ctx, page, handle, win)
}

fn group_ids(rows: &[mirqab_view::tabs::apps::Row]) -> Vec<String> {
    use mirqab_view::tabs::apps::Row;
    rows.iter()
        .filter_map(|r| match r {
            Row::Group(g) => Some(g.id.clone()),
            _ => None,
        })
        .collect()
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn escape_clears_the_selection_and_otherwise_goes_on() {
    let (ctx, page, handle, _win) = apps_page();
    // The page's own Escape: a local shortcut, asked before the window's.
    let keys = page
        .observe_controllers()
        .into_iter()
        .filter_map(|c| c.ok()?.downcast::<gtk::ShortcutController>().ok())
        .find(|c| c.scope() == gtk::ShortcutScope::Local)
        .expect("a local shortcut controller on the page");
    let escape = (0..keys.n_items())
        .filter_map(|i| keys.item(i).and_downcast::<gtk::Shortcut>())
        .find(|s| s.trigger().is_some_and(|t| t.to_str() == "Escape"))
        .expect("Escape is bound");
    let press = || {
        escape
            .action()
            .unwrap()
            .activate(gtk::ShortcutActionFlags::empty(), &page, None)
    };
    handle.activate(0);
    assert!(ctx.apps.borrow().selected.is_some(), "a row selected");
    assert!(press(), "spent: it cleared the selection");
    assert_eq!(ctx.apps.borrow().selected, None);
    assert!(
        !press(),
        "nothing selected: it goes on (a popover hides; a window does nothing)"
    );
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn a_frame_during_a_press_waits_for_the_release() {
    let (ctx, _page, handle, _win) = apps_page();
    let before = handle.rows();
    assert_eq!(group_ids(&before)[0], "session:3030", "cargo first by CPU");
    // A frame that reorders the list arrives while a pointer is down: the
    // rows under the pointer stay until the release (S8).
    let mut v: Value =
        serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    for g in v["groups"].as_array_mut().unwrap() {
        if g["id"] == "session:3030" {
            g["cpu"] = json!(0.0);
        }
    }
    handle.press(true);
    handle.frame(&v);
    assert_eq!(handle.rows(), before, "held while pressed");
    // The release activates the row that was under the press.
    handle.activate(0);
    assert_eq!(
        ctx.apps
            .borrow()
            .selected
            .map(|k| matches!(k, mirqab_view::tabs::apps::Key::Group(_))),
        Some(true)
    );
    let picked = match ctx.apps.borrow().selected {
        Some(mirqab_view::tabs::apps::Key::Group(k)) => k,
        _ => unreachable!(),
    };
    handle.press(false);
    let after = handle.rows();
    assert_ne!(
        group_ids(&after)[0],
        "session:3030",
        "the release wrote the frame"
    );
    let cargo = after
        .iter()
        .find_map(|r| match r {
            mirqab_view::tabs::apps::Row::Group(g) if g.id == "session:3030" => Some(g.key),
            _ => None,
        })
        .unwrap();
    assert_eq!(picked, cargo, "the press's row, cargo, was selected");
    // A release with no frame waiting writes nothing.
    handle.press(true);
    handle.press(false);
    assert_eq!(handle.rows(), after);
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn the_list_item_carries_the_selected_state() {
    let (ctx, _page, handle, win) = apps_page();
    // What the page wrote on each list item: GTK itself writes false on
    // every item under NoSelection, so a row reads selected only through
    // the page's write (N8). GTK gives no getter for the value; the page
    // records what it wrote, and an AT readback is an owner check (Orca).
    let marked = |name: &str| {
        handle
            .marked()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    };
    let others_false = |name: &str| {
        let m = handle.marked();
        assert!(
            m.iter().all(|(n, v)| n == name || *v != Some(true)),
            "only {name} reads true: {m:?}"
        );
        assert!(
            m.iter().filter(|(_, v)| *v == Some(false)).count() >= 10,
            "the rows on screen read false: {m:?}"
        );
    };
    handle.activate(0);
    settle("the selected row is drawn", || {
        named(win.0.upcast_ref(), "apps-group:session:3030")
            .and_then(|w| w.parent())
            .is_some_and(|b| b.has_css_class("selected"))
    });
    assert_eq!(marked("apps-group:session:3030"), Some(Some(true)));
    others_false("apps-group:session:3030");
    // On the list item, not on the row's box.
    let row = named(win.0.upcast_ref(), "apps-group:session:3030").unwrap();
    let item_box = row.parent().unwrap();
    let list_item = item_box.parent().unwrap();
    assert_eq!(list_item.accessible_role(), gtk::AccessibleRole::ListItem);
    assert!(gtk::test_accessible_has_state(
        &list_item,
        gtk::AccessibleState::Selected
    ));
    assert!(
        !gtk::test_accessible_has_state(&item_box, gtk::AccessibleState::Selected),
        "not on the row's box"
    );

    // A frame that reorders the list rebinds the items: cargo still reads
    // true wherever it went, and the row now first reads false.
    let mut v: Value =
        serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    for g in v["groups"].as_array_mut().unwrap() {
        if g["id"] == "session:3030" {
            g["cpu"] = json!(0.0);
        }
    }
    handle.frame(&v);
    settle("the reordered rows are drawn", || {
        handle.marked().iter().all(|(_, v)| v.is_some())
    });
    let first = group_ids(&handle.rows())[0].clone();
    assert_ne!(first, "session:3030");
    assert_eq!(marked(&format!("apps-group:{first}")), Some(Some(false)));
    if let Some(m) = marked("apps-group:session:3030") {
        assert_eq!(m, Some(true), "cargo, rebound elsewhere, still selected");
    }
    others_false("apps-group:session:3030");

    // Cleared: nothing reads true.
    assert!(handle.escape());
    assert_eq!(marked("apps-group:session:3030").flatten(), Some(false));
    assert!(handle.marked().iter().all(|(_, v)| *v != Some(true)));
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn a_press_the_list_never_releases_lets_frames_through() {
    let (ctx, _page, handle, _win) = apps_page();
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    let mut v = golden.clone();
    for g in v["groups"].as_array_mut().unwrap() {
        if g["id"] == "session:3030" {
            g["cpu"] = json!(0.0);
        }
    }
    let before = handle.rows();
    handle.press(true);
    handle.frame(&v);
    handle.frame(&v);
    assert_eq!(handle.rows(), before, "two frames held while pressed");
    // The third frame in one press: the release was lost, the rows move.
    handle.frame(&v);
    assert_ne!(group_ids(&handle.rows())[0], "session:3030", "written");
    // The press is clear: the next frame is written at once.
    handle.frame(&golden);
    assert_eq!(group_ids(&handle.rows())[0], "session:3030");
    // A new press holds again, from zero.
    handle.press(true);
    handle.frame(&v);
    assert_eq!(group_ids(&handle.rows())[0], "session:3030", "held");
    handle.press(false);
    assert_ne!(group_ids(&handle.rows())[0], "session:3030", "released");
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn collapsing_the_page_keeps_the_selection() {
    let (ctx, page, handle, win) = apps_page();
    fn split(w: &gtk::Widget) -> Option<adw::OverlaySplitView> {
        if let Some(s) = w.downcast_ref::<adw::OverlaySplitView>() {
            return Some(s.clone());
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            if let Some(s) = split(&child) {
                return Some(s);
            }
            c = child.next_sibling();
        }
        None
    }
    let split = split(&page).expect("the split view");
    assert!(split.is_pin_sidebar(), "the sheet is the selection's");
    handle.activate(0);
    assert!(split.shows_sidebar());
    // Under the 60 em breakpoint the sheet overlays the list; it must not
    // close, nor clear the selection (review of PR #21: the Home flake).
    split.set_collapsed(true);
    assert!(ctx.apps.borrow().selected.is_some(), "still selected");
    assert!(split.shows_sidebar(), "still shown");
    split.set_collapsed(false);
    assert!(ctx.apps.borrow().selected.is_some());
    // The same through the breakpoint itself: a narrow window.
    page.set_size_request(300, 400);
    win.0.set_default_size(300, 400);
    settle("the page is narrow and collapsed", || split.is_collapsed());
    assert!(ctx.apps.borrow().selected.is_some(), "still selected");
    assert!(split.shows_sidebar());
    // An explicit hide still closes the sheet and clears the selection.
    split.set_show_sidebar(false);
    assert_eq!(ctx.apps.borrow().selected, None);
    ctx.client.close();
}

#[gtk::test]
#[ignore = "needs a display: run tests/test-ui-widgets.sh"]
fn a_remapped_page_shows_nothing_before_its_first_frame() {
    let (ctx, page, handle, win) = apps_page();
    assert!(!handle.rows().is_empty());
    win.0.set_visible(false);
    settle("the page is unmapped", || !page.is_mapped());
    assert!(
        handle.rows().is_empty(),
        "the rows went with the frame (N3)"
    );
    win.0.present();
    settle("the page is mapped again", || page.is_mapped());
    assert!(handle.rows().is_empty(), "nothing stale before a frame");
    let v: Value = serde_json::from_str(include_str!("../../../tests/golden/apps.json")).unwrap();
    handle.frame(&v);
    assert!(!handle.rows().is_empty(), "the next frame fills it");
    ctx.client.close();
}
