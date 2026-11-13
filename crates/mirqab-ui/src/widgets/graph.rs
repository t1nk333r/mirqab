//! The line graph (architecture §7.4): one stroked `gsk::Path` and one
//! filled path for the series, one more stroke for peaks, all geometry from
//! `mirqab_view::graph`. A gap is a second contour in the same path, so the
//! node count does not grow with the data. Written new; nothing is adapted
//! from haseen's QML graph.
//!
//! It owns the live rule (architecture §5.3, plan 001 "The `live` flag"):
//! it holds a live subscription only while it is mapped (so its window is
//! visible) and its range is `2m`, and drops it on unmap, on a range change
//! and when the window hides (which unmaps it). It is the one caller of
//! `Source::subscribe_live`; lint fails anywhere else.

use crate::client::Source;
use crate::style;
use crate::widgets::em;
use gtk::glib;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use mirqab_proto::strings;
use mirqab_view::graph::{self, Scale, Window};
use serde_json::Value;
use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Turns a stream event into the graph's value.
pub type Extract = Rc<dyn Fn(&Value) -> Option<f64>>;
/// Formats a value for the accessible description.
pub type Format = Rc<dyn Fn(f64) -> String>;

/// The stream a graph follows live at `2m`.
#[derive(Clone)]
pub struct Live {
    pub source: Rc<dyn Source>,
    pub stream: String,
    pub extract: Extract,
}

pub struct State {
    pub points: Vec<(i64, f64)>,
    pub peaks: Vec<(i64, f64)>,
    pub scale: Scale,
    pub series: usize,
    pub range: String,
    /// The right edge for stored ranges; None is now.
    pub end: Option<i64>,
    pub live: Option<Live>,
    /// The 2-minute ring of live samples (§6.4); emptied when live ends.
    pub ring: VecDeque<(i64, f64)>,
    pub handle: Option<Box<dyn Any>>,
    pub name: String,
    pub format: Option<Format>,
    /// Shorter, for a card on Home (plan 003).
    pub compact: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            points: Vec::new(),
            peaks: Vec::new(),
            scale: Scale::Auto { floor: 1.0 },
            series: 0,
            range: "1h".to_string(),
            end: None,
            live: None,
            ring: VecDeque::new(),
            handle: None,
            name: String::new(),
            format: None,
            compact: false,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Graph {
        pub state: RefCell<State>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Graph {
        const NAME: &'static str = "MirqabGraph";
        type Type = super::Graph;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("mirqab-graph");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Graph {
        fn dispose(&self) {
            self.state.borrow_mut().handle = None;
        }
    }

    impl WidgetImpl for Graph {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let e = em(&*self.obj());
            match orientation {
                gtk::Orientation::Horizontal => ((e * 10.0) as i32, (e * 24.0) as i32, -1, -1),
                _ if self.state.borrow().compact => ((e * 2.5) as i32, (e * 3.5) as i32, -1, -1),
                _ => ((e * 4.0) as i32, (e * 6.0) as i32, -1, -1),
            }
        }

        fn map(&self) {
            self.parent_map();
            self.obj().update_live();
        }

        fn unmap(&self) {
            self.obj().update_live_mapped(false);
            self.parent_unmap();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let st = self.state.borrow();
            let Some(window) = st.window() else { return };
            let line_width = em(&*obj) * 0.1;
            let main: Vec<(i64, f64)> = if st.range == "2m" {
                st.ring.iter().copied().collect()
            } else {
                st.points.clone()
            };
            // Peaks sit at or above the averages, so when there are peaks
            // they set the scale both share; otherwise every spike above the
            // averages' ceiling would be clipped (review of PR #4).
            let peaks = (st.range != "2m" && !st.peaks.is_empty())
                .then(|| graph::geometry(&st.peaks, window, st.scale, w, h));
            let scale = match &peaks {
                Some(p) => Scale::Fixed {
                    min: p.bottom,
                    max: p.top,
                },
                None => st.scale,
            };
            let g = graph::geometry(&main, window, scale, w, h);
            let color = style::series(st.series);
            if let Some(fill) = path(g.runs.iter().map(|r| r.fill(h)), true) {
                let mut faint = color;
                faint.set_alpha(color.alpha() * 0.2);
                snapshot.append_fill(&fill, gsk::FillRule::Winding, &faint);
            }
            if let Some(line) = path(g.runs.iter().map(|r| r.line.clone()), false) {
                snapshot.append_stroke(&line, &gsk::Stroke::new(line_width), &color);
            }
            if let Some(pg) = peaks
                && let Some(line) = path(pg.runs.iter().map(|r| r.line.clone()), false)
            {
                let mut faint = color;
                faint.set_alpha(color.alpha() * 0.5);
                snapshot.append_stroke(&line, &gsk::Stroke::new(line_width * 0.6), &faint);
            }
        }
    }
}

/// One path holding every run as its own contour; None when there is
/// nothing to draw.
fn path(runs: impl Iterator<Item = Vec<(f32, f32)>>, closed: bool) -> Option<gsk::Path> {
    let b = gsk::PathBuilder::new();
    let mut any = false;
    for run in runs {
        let mut pts = run.into_iter();
        let Some((x, y)) = pts.next() else { continue };
        b.move_to(x, y);
        for (x, y) in pts {
            b.line_to(x, y);
        }
        if closed {
            b.close();
        }
        any = true;
    }
    any.then(|| b.to_path())
}

impl State {
    fn window(&self) -> Option<Window> {
        let end = if self.range == "2m" {
            self.ring.back().map(|&(t, _)| t).unwrap_or_else(now)
        } else {
            self.end.unwrap_or_else(now)
        };
        Window::for_range(&self.range, end)
    }
}

fn now() -> i64 {
    glib::real_time() / 1_000_000
}

glib::wrapper! {
    pub struct Graph(ObjectSubclass<imp::Graph>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    pub fn new() -> Graph {
        let g: Graph = glib::Object::new();
        let weak = g.downgrade();
        style::on_change(move || {
            let Some(g) = weak.upgrade() else {
                return false;
            };
            g.queue_draw();
            true
        });
        g
    }

    /// Stored buckets: `(unix seconds, avg)` and, optionally, their peaks.
    pub fn set_points(&self, points: Vec<(i64, f64)>, peaks: Vec<(i64, f64)>) {
        {
            let mut st = self.imp().state.borrow_mut();
            st.points = points;
            st.peaks = peaks;
        }
        self.describe();
        self.queue_draw();
    }

    pub fn set_compact(&self, compact: bool) {
        self.imp().state.borrow_mut().compact = compact;
        self.queue_resize();
    }

    pub fn set_scale(&self, scale: Scale) {
        self.imp().state.borrow_mut().scale = scale;
        self.queue_draw();
    }

    pub fn set_series(&self, n: usize) {
        self.imp().state.borrow_mut().series = n;
        self.queue_draw();
    }

    /// The right edge of a stored range (tests pin it; None follows now).
    pub fn set_end(&self, end: Option<i64>) {
        self.imp().state.borrow_mut().end = end;
        self.queue_draw();
    }

    /// The series' name and how to format its value, for the accessible
    /// description.
    pub fn set_name(&self, name: &str, format: Format) {
        {
            let mut st = self.imp().state.borrow_mut();
            st.name = name.to_string();
            st.format = Some(format);
        }
        self.describe();
    }

    /// The stream to follow at `2m`.
    pub fn set_live(&self, live: Option<Live>) {
        {
            let mut st = self.imp().state.borrow_mut();
            st.handle = None;
            st.ring.clear();
            st.live = live;
        }
        self.update_live();
    }

    pub fn range(&self) -> String {
        self.imp().state.borrow().range.clone()
    }

    pub fn set_range(&self, range: &str) {
        if self.imp().state.borrow().range == range {
            return;
        }
        self.imp().state.borrow_mut().range = range.to_string();
        self.update_live();
        self.describe();
        self.queue_draw();
    }

    /// Samples in the live ring (tests).
    pub fn ring_len(&self) -> usize {
        self.imp().state.borrow().ring.len()
    }

    /// Whether a live subscription is held (tests).
    pub fn is_live(&self) -> bool {
        self.imp().state.borrow().handle.is_some()
    }

    fn update_live(&self) {
        self.update_live_mapped(self.is_mapped());
    }

    fn update_live_mapped(&self, mapped: bool) {
        let want = {
            let st = self.imp().state.borrow();
            mapped && st.range == "2m" && st.live.is_some()
        };
        let have = self.is_live();
        if want == have {
            return;
        }
        if !want {
            let mut st = self.imp().state.borrow_mut();
            st.handle = None;
            st.ring.clear();
            return;
        }
        let Some(live) = self.imp().state.borrow().live.clone() else {
            return;
        };
        let weak = self.downgrade();
        let extract = Rc::clone(&live.extract);
        let handle = live.source.subscribe_live(
            &live.stream,
            Rc::new(move |data: &Value| {
                let Some(g) = weak.upgrade() else { return };
                let Some(v) = extract(data) else { return };
                g.push_live(now(), v);
            }),
        );
        self.imp().state.borrow_mut().handle = Some(handle);
    }

    /// Adds a live sample and keeps the ring to two minutes. Samples are
    /// stamped in whole seconds, so a second arrival in the same second
    /// replaces the first rather than drawing a vertical step.
    pub fn push_live(&self, t: i64, v: f64) {
        {
            let mut st = self.imp().state.borrow_mut();
            if st.ring.back().is_some_and(|&(last, _)| last >= t) {
                st.ring.pop_back();
            }
            st.ring.push_back((t, v));
            while st.ring.front().is_some_and(|&(t0, _)| t - t0 > 120) {
                st.ring.pop_front();
            }
        }
        self.describe();
        self.queue_draw();
    }

    fn describe(&self) {
        let text = {
            let st = self.imp().state.borrow();
            let latest = if st.range == "2m" {
                st.ring.back()
            } else {
                st.points.last()
            };
            let value = match (latest, &st.format) {
                (Some(&(_, v)), Some(f)) => Some(f(v)),
                _ => None,
            };
            strings::graph_summary(&st.name, value.as_deref())
        };
        self.update_property(&[gtk::accessible::Property::Description(&text)]);
    }
}
