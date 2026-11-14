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
    let i = Inspector::new(fake.clone(), Rc::new(|| true), Rc::new(|| {}));
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
    assert_eq!(
        fake.requests.borrow().last(),
        Some(&("get apps.inspect".to_string(), json!({"pid": 2001})))
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
        json!({"pid": 2000})
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
    // Go ahead: the same args, confirm and the plan.
    fake.answers.borrow_mut().push_back(Ok(json!({
        "done": [{"signal": "TERM", "pid": 2001}], "gone": [], "failed": [],
        "skipped": [], "recorded": false
    })));
    d.emit_by_name::<()>("response", &[&"confirm"]);
    assert_eq!(
        fake.requests.borrow().last().unwrap().1,
        json!({"name": "apps.quit", "args": group, "confirm": true, "plan": "p1"})
    );
    assert!(i.result.text().contains(mirqab_proto::strings::APPS_DONE));
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
    assert_eq!(i.dialog(), Some(d), "no second dialog");

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
