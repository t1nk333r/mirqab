//! The Apps page (plan 006, part 3): a search field, the sort control, the
//! "Show processes" switch and the pin control above a `gtk::ListView`
//! over a flat model; the inspector is an `adw::OverlaySplitView`'s
//! sidebar, shown while a row is selected and overlaid under 60 em of page
//! width (decisions 6 and 10).
//!
//! The page subscribes to `apps` while it is mapped, once, with
//! `{"limit": 1000, "expand": true}` (decision 1). Each frame writes row
//! `i` into the store's object `i` and redraws only the bound rows whose
//! row changed; the store grows or shrinks at its end. Nothing is
//! reordered under the list, so the scroll position holds.
//!
//! Part 4 (decision 16): a frame that arrives while a pointer is pressed on
//! the list is written at the release, so the row under the press is the
//! row selected (S8); Escape clears the selection, and with none it goes
//! on to the window (a popover hides); unmapping forgets the frame and the
//! rows, so a re-map shows nothing stale before its first frame (N3); the
//! selected state for assistive technology is on the list item (N8).

use crate::app::Ctx;
use crate::client::Subscription;
use crate::tabs::apps::group_row::GroupRow;
use crate::tabs::apps::inspector::Inspector;
use crate::tabs::apps::process_row::ProcessRow;
use crate::tabs::pin_button;
use crate::widgets::em;
use adw::prelude::*;
use gtk::{gio, glib};
use mirqab_proto::stream::{self, AppsData};
use mirqab_proto::strings;
use mirqab_view::tabs::apps::{self as model, Frame, Key, Outcome, Row, Sort};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// What one list item shows: a group row, a process row or a note, one at
/// a time, and the store object it is bound to.
struct Item {
    root: gtk::Box,
    group: GroupRow,
    process: ProcessRow,
    note: gtk::Label,
    bound: RefCell<Option<glib::BoxedAnyObject>>,
    /// Whether the bound row is the selection, for the list item's state.
    selected: Rc<Cell<bool>>,
    /// The value last written to the list item's `Selected` state; None
    /// before the row was first on screen (the widget tests read it: GTK
    /// gives no getter for a state's value).
    applied: Rc<Cell<Option<bool>>>,
}

/// Writes `Selected` on `item` and gives back what it wrote.
fn write_selected(item: &gtk::Widget, selected: bool) -> bool {
    item.update_state(&[gtk::accessible::State::Selected(Some(selected))]);
    selected
}

/// The selected state on the list item that holds `root`, which assistive
/// technology reads as the row (review of PR #19, N8). GTK writes
/// `Selected(false)` on every list item under `NoSelection`, so without
/// this no row ever reads selected. Only once `root` is on screen: a list
/// item being bound may not be parented yet, and taking a reference to it
/// then would sink and drop it.
fn mark_selected(root: &gtk::Box, selected: bool, applied: &Cell<Option<bool>>) {
    if !root.is_mapped() {
        return;
    }
    if let Some(item) = root.parent() {
        applied.set(Some(write_selected(&item, selected)));
    }
}

impl Item {
    fn new(on_toggle: Rc<dyn Fn(u64)>) -> Item {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let group = GroupRow::new(on_toggle);
        let process = ProcessRow::new();
        let note = gtk::Label::new(None);
        note.add_css_class("dim-label");
        note.add_css_class("mirqab-apps-note");
        root.append(&group.root);
        root.append(&process.root);
        root.append(&note);
        let selected = Rc::new(Cell::new(false));
        let applied = Rc::new(Cell::new(None));
        let (s, a) = (Rc::clone(&selected), Rc::clone(&applied));
        root.connect_map(move |root| mark_selected(root, s.get(), &a));
        Item {
            root,
            group,
            process,
            note,
            bound: RefCell::new(None),
            selected,
            applied,
        }
    }

    fn show(&self, row: &Row) {
        let selected = match row {
            Row::Group(g) => {
                self.group.show(g);
                g.selected
            }
            Row::Process(p) => {
                self.process.show(p);
                p.selected
            }
            Row::Note(n) => {
                self.note.set_text(n);
                false
            }
        };
        self.group.root.set_visible(matches!(row, Row::Group(_)));
        self.process
            .root
            .set_visible(matches!(row, Row::Process(_)));
        self.note.set_visible(matches!(row, Row::Note(_)));
        if selected {
            self.root.add_css_class("selected");
        } else {
            self.root.remove_css_class("selected");
        }
        // On the list item (N8); the box keeps only its style class.
        self.selected.set(selected);
        mark_selected(&self.root, selected, &self.applied);
    }

    fn show_bound(&self) {
        let row = self
            .bound
            .borrow()
            .as_ref()
            .map(|o| o.borrow::<Row>().clone());
        if let Some(row) = row {
            self.show(&row);
        }
    }
}

/// Frames a press may hold back before it is taken as lost (S8).
const HOLD_FRAMES: u8 = 2;

struct Page {
    ctx: Weak<Ctx>,
    split: adw::OverlaySplitView,
    search: gtk::SearchEntry,
    notice: gtk::Label,
    store: gio::ListStore,
    items: RefCell<HashMap<gtk::ListItem, Rc<Item>>>,
    frame: RefCell<Frame>,
    inspector: RefCell<Option<Rc<Inspector>>>,
    sub: RefCell<Option<Subscription>>,
    first: Cell<bool>,
    /// A pointer is pressed on the list: a frame's rows wait for the
    /// release (S8).
    pressed: Cell<bool>,
    /// A frame came while pressed; the release writes it.
    deferred: Cell<bool>,
    /// Frames held during this press.
    held: Cell<u8>,
    /// The page itself is hiding the sheet (on unmap): the selection stays.
    quiet: Cell<bool>,
}

impl Page {
    fn on_frame(self: &Rc<Self>, data: &Value) {
        let Ok(d) = serde_json::from_value::<AppsData>(data.clone()) else {
            return;
        };
        if !model::expanded(&d) {
            return;
        }
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let was = ctx.apps.borrow().selected;
        let (frame, lost) = ctx.apps.borrow_mut().take(d);
        *self.frame.borrow_mut() = frame;
        if lost {
            // The outcome of an act on the row outlives it (review of
            // PR #19, B1); a row that went with no act says only that.
            let outcome = was.and_then(|k| {
                self.inspector
                    .borrow()
                    .as_ref()
                    .and_then(|i| i.take_outcome(k))
            });
            match outcome {
                Some(o) => self.say_outcome(&o),
                None => {
                    self.notice.set_text(strings::APPS_ROW_GONE);
                    self.notice.remove_css_class("warning");
                    self.notice.set_visible(true);
                }
            }
        }
        self.refresh(true);
        if !self.first.replace(true) {
            ctx.page_ready("apps");
        }
    }

    /// Writes the rows into the store and the inspector; `tick` for a new
    /// frame. A frame that comes while a pointer is pressed on the list
    /// waits for the release: the press's row must still be at its position
    /// when the release activates it (review of PR #19, S8). The release
    /// writes the latest frame, so none is lost.
    fn refresh(&self, tick: bool) {
        if tick && self.pressed.get() {
            // A press on a single-click list lasts well under the two
            // frames (some 4 s) this holds; a longer one is a release the
            // list never saw (a keyboard workspace switch mid-press on
            // Wayland sends a leave, no release), and the third frame is
            // written with the press taken as lost (review of PR #21).
            if self.held.get() < HOLD_FRAMES {
                self.held.set(self.held.get() + 1);
                self.deferred.set(true);
                return;
            }
            self.pressed.set(false);
            self.held.set(0);
            self.deferred.set(false);
        }
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let (rows, selected) = {
            let st = ctx.apps.borrow();
            let frame = self.frame.borrow();
            (model::rows(&frame, &st, &self.search.text()), st.selected)
        };
        let old = self.store.n_items();
        let mut changed: Vec<glib::BoxedAnyObject> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            match self
                .store
                .item(i as u32)
                .and_downcast::<glib::BoxedAnyObject>()
            {
                Some(obj) => {
                    if *obj.borrow::<Row>() != *row {
                        *obj.borrow_mut::<Row>() = row.clone();
                        changed.push(obj);
                    }
                }
                None => self.store.append(&glib::BoxedAnyObject::new(row.clone())),
            }
        }
        let n = rows.len() as u32;
        if old > n {
            self.store.splice(n, old - n, &[] as &[glib::Object]);
        }
        if !changed.is_empty() {
            for item in self.items.borrow().values() {
                let hit = item
                    .bound
                    .borrow()
                    .as_ref()
                    .is_some_and(|b| changed.contains(b));
                if hit {
                    item.show_bound();
                }
            }
        }
        let sel =
            selected.and_then(|k| model::selection(&self.frame.borrow(), k, std::process::id()));
        self.split.set_show_sidebar(sel.is_some());
        if let Some(i) = self.inspector.borrow().as_ref() {
            // Hidden too, not only slid away: an overlaid sheet stays
            // mapped otherwise, and so would its live graph.
            i.root.set_visible(sel.is_some());
            i.show(sel, &self.frame.borrow(), tick);
        }
    }

    /// A row was activated: select it (decision 6).
    fn activate(&self, pos: u32) {
        let Some(obj) = self.store.item(pos).and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let key = match &*obj.borrow::<Row>() {
            Row::Group(g) => Key::Group(g.key),
            Row::Process(p) => Key::Process {
                pid: p.pid,
                start: p.start,
            },
            Row::Note(_) => return,
        };
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.apps.borrow_mut().selected = Some(key);
        }
        self.notice.set_visible(false);
        self.refresh(false);
    }

    fn toggle(&self, key: u64) {
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.apps.borrow_mut().toggle_open(key);
        }
        self.refresh(false);
    }

    /// An act's outcome on the line above the list, until the next
    /// selection.
    fn say_outcome(&self, o: &Outcome) {
        self.notice.set_text(&model::outcome_notice(o));
        if o.warn {
            self.notice.add_css_class("warning");
        } else {
            self.notice.remove_css_class("warning");
        }
        self.notice.set_visible(true);
    }

    fn close_inspector(&self) {
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.apps.borrow_mut().selected = None;
        }
        self.refresh(false);
    }

    /// A pointer went down on the list (`true`) or came up (`false`); the
    /// release writes a frame that waited for it.
    fn press(&self, down: bool) {
        self.pressed.set(down);
        self.held.set(0);
        if !down && self.deferred.replace(false) {
            self.refresh(true);
        }
    }

    /// Escape: clears the selection, which closes the sheet, and is then
    /// spent; with nothing selected it goes on to the window, so a popover
    /// still hides (owner, 2026-11-14, part 4 decision 2). A window never
    /// closes on Escape (plan 022).
    fn escape(&self) -> bool {
        let selected = self
            .ctx
            .upgrade()
            .is_some_and(|c| c.apps.borrow().selected.is_some());
        if selected {
            self.close_inspector();
        }
        selected
    }

    /// Off screen: the subscription goes, and the frame and the rows with
    /// it, so a re-map shows nothing stale before its first frame (review
    /// of PR #19, N3). The sheet is hidden without clearing the selection,
    /// which the page keeps (decision 13).
    fn unmapped(&self) {
        *self.sub.borrow_mut() = None;
        *self.frame.borrow_mut() = Frame::default();
        self.store.remove_all();
        self.pressed.set(false);
        self.deferred.set(false);
        self.held.set(0);
        self.quiet.set(true);
        self.split.set_show_sidebar(false);
        self.quiet.set(false);
        if let Some(i) = self.inspector.borrow().as_ref() {
            i.hidden();
            i.root.set_visible(false);
        }
    }
}

/// The page, for the widget tests: frames fed by hand, a press held, the
/// rows the store holds.
pub struct Handle(Rc<Page>);

impl Handle {
    /// Takes `data` as an `apps` frame.
    pub fn frame(&self, data: &Value) {
        self.0.on_frame(data);
    }

    /// A pointer down on the list, or up.
    pub fn press(&self, down: bool) {
        self.0.press(down);
    }

    /// The rows the list shows now, in order.
    pub fn rows(&self) -> Vec<Row> {
        (0..self.0.store.n_items())
            .filter_map(|i| self.0.store.item(i).and_downcast::<glib::BoxedAnyObject>())
            .map(|o| o.borrow::<Row>().clone())
            .collect()
    }

    /// Activates the row at `pos`, as a click does.
    pub fn activate(&self, pos: u32) {
        self.0.activate(pos);
    }

    /// Each bound row's widget name (`apps-group:<id>`, `apps-process:<pid>`)
    /// with the `Selected` value the page last wrote on its list item; None
    /// for a row not yet on screen.
    pub fn marked(&self) -> Vec<(String, Option<bool>)> {
        self.0
            .items
            .borrow()
            .values()
            .filter_map(|item| {
                let row = item
                    .bound
                    .borrow()
                    .as_ref()
                    .map(|o| o.borrow::<Row>().clone())?;
                let name = match row {
                    Row::Group(g) => format!("apps-group:{}", g.id),
                    Row::Process(p) => format!("apps-process:{}", p.pid),
                    Row::Note(_) => return None,
                };
                Some((name, item.applied.get()))
            })
            .collect()
    }

    /// Escape, as the page's shortcut runs it: true when it was spent.
    pub fn escape(&self) -> bool {
        self.0.escape()
    }
}

fn toolbar(ctx: &Rc<Ctx>, page: &Rc<Page>) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("mirqab-apps-toolbar");
    page.search.set_hexpand(true);
    page.search.set_placeholder_text(Some(strings::APPS_SEARCH));
    page.search
        .update_property(&[gtk::accessible::Property::Label(strings::APPS_SEARCH)]);
    page.search.set_widget_name("apps-search");
    // `changed`, not `search-changed`: filtering is a pure function over
    // the last frame, with nothing to wait for.
    let weak = Rc::downgrade(page);
    page.search.connect_changed(move |_| {
        if let Some(p) = weak.upgrade() {
            p.refresh(false);
        }
    });

    let labels: Vec<&str> = Sort::ALL.iter().map(|s| s.label()).collect();
    let sort = gtk::DropDown::from_strings(&labels);
    sort.set_widget_name("apps-sort");
    sort.set_tooltip_text(Some(strings::APPS_SORT));
    sort.update_property(&[gtk::accessible::Property::Label(strings::APPS_SORT)]);
    sort.set_selected(ctx.apps.borrow().sort.index());
    let weak = Rc::downgrade(page);
    sort.connect_selected_notify(move |d| {
        let Some(p) = weak.upgrade() else { return };
        if let Some(ctx) = p.ctx.upgrade() {
            ctx.apps.borrow_mut().sort = Sort::from_index(d.selected());
        }
        p.refresh(false);
    });

    let flat_label = gtk::Label::new(Some(strings::APPS_SHOW_PROCESSES));
    let flat = gtk::Switch::new();
    flat.set_widget_name("apps-flatten");
    flat.set_valign(gtk::Align::Center);
    flat.set_active(ctx.apps.borrow().flatten);
    flat.update_property(&[gtk::accessible::Property::Label(
        strings::APPS_SHOW_PROCESSES,
    )]);
    let weak = Rc::downgrade(page);
    flat.connect_active_notify(move |sw| {
        let Some(p) = weak.upgrade() else { return };
        if let Some(ctx) = p.ctx.upgrade() {
            ctx.apps.borrow_mut().flatten = sw.is_active();
        }
        p.refresh(false);
    });

    bar.append(&page.search);
    bar.append(&sort);
    bar.append(&flat_label);
    bar.append(&flat);
    bar.append(&pin_button(ctx, "apps.top"));
    bar
}

fn list(page: &Rc<Page>) -> gtk::ListView {
    let factory = gtk::SignalListItemFactory::new();
    let weak = Rc::downgrade(page);
    factory.connect_setup(move |_, obj| {
        let (Some(p), Some(li)) = (weak.upgrade(), obj.downcast_ref::<gtk::ListItem>()) else {
            return;
        };
        let w = Rc::downgrade(&p);
        let item = Rc::new(Item::new(Rc::new(move |key| {
            if let Some(p) = w.upgrade() {
                p.toggle(key);
            }
        })));
        li.set_child(Some(&item.root));
        p.items.borrow_mut().insert(li.clone(), item);
    });
    let weak = Rc::downgrade(page);
    factory.connect_bind(move |_, obj| {
        let (Some(p), Some(li)) = (weak.upgrade(), obj.downcast_ref::<gtk::ListItem>()) else {
            return;
        };
        let items = p.items.borrow();
        let Some(item) = items.get(li) else { return };
        *item.bound.borrow_mut() = li.item().and_downcast::<glib::BoxedAnyObject>();
        item.show_bound();
    });
    let weak = Rc::downgrade(page);
    factory.connect_unbind(move |_, obj| {
        let (Some(p), Some(li)) = (weak.upgrade(), obj.downcast_ref::<gtk::ListItem>()) else {
            return;
        };
        if let Some(item) = p.items.borrow().get(li) {
            *item.bound.borrow_mut() = None;
        }
    });
    let weak = Rc::downgrade(page);
    factory.connect_teardown(move |_, obj| {
        let (Some(p), Some(li)) = (weak.upgrade(), obj.downcast_ref::<gtk::ListItem>()) else {
            return;
        };
        p.items.borrow_mut().remove(li);
    });
    let model = gtk::NoSelection::new(Some(page.store.clone()));
    let view = gtk::ListView::new(Some(model), Some(factory));
    view.set_single_click_activate(true);
    // A press and its release as raw events, in the capture phase: a
    // gesture here could be cancelled when a row's own gesture claims the
    // press. The release is taken once its event is done, because the row
    // activates on that same release (S8).
    let legacy = gtk::EventControllerLegacy::new();
    legacy.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(page);
    legacy.connect_event(move |_, ev| {
        use gtk::gdk::EventType as T;
        match ev.event_type() {
            T::ButtonPress | T::TouchBegin => {
                if let Some(p) = weak.upgrade() {
                    p.press(true);
                }
            }
            T::ButtonRelease | T::TouchEnd | T::TouchCancel | T::GrabBroken => {
                let weak = weak.clone();
                glib::idle_add_local_once(move || {
                    if let Some(p) = weak.upgrade() {
                        p.press(false);
                    }
                });
            }
            _ => {}
        }
        glib::Propagation::Proceed
    });
    view.add_controller(legacy);
    view.add_css_class("mirqab-apps-list");
    view.update_property(&[gtk::accessible::Property::Label(strings::APPS_LIST)]);
    let weak = Rc::downgrade(page);
    view.connect_activate(move |_, pos| {
        if let Some(p) = weak.upgrade() {
            p.activate(pos);
        }
    });
    view
}

pub fn build(ctx: &Rc<Ctx>) -> gtk::Widget {
    build_page(ctx).0
}

/// The page and a handle on it (the widget tests feed it frames).
pub fn build_page(ctx: &Rc<Ctx>) -> (gtk::Widget, Handle) {
    let split = adw::OverlaySplitView::new();
    split.set_sidebar_position(gtk::PackType::End);
    split.set_enable_show_gesture(false);
    // The sheet's visibility is the selection's (`refresh` sets it every
    // frame), so collapsing or uncollapsing must not change it on its own:
    // unpinned, the 60 em breakpoint's collapse hides the sidebar, and the
    // notify handler below took that for the user closing the sheet and
    // cleared the selection (review of PR #21; the Home test's flake since
    // part 3b, and a window shrunk under 60 em lost its selection).
    split.set_pin_sidebar(true);
    // A swipe would hide the sheet with its row still selected, its graph
    // still live, and the next frame would show it again (review of
    // PR #19, S7).
    split.set_enable_hide_gesture(false);
    let notice = gtk::Label::new(None);
    notice.add_css_class("dim-label");
    notice.set_xalign(0.0);
    notice.set_visible(false);
    notice.set_widget_name("apps-notice");
    let page = Rc::new(Page {
        ctx: Rc::downgrade(ctx),
        split: split.clone(),
        search: gtk::SearchEntry::new(),
        notice: notice.clone(),
        store: gio::ListStore::new::<glib::BoxedAnyObject>(),
        items: RefCell::new(HashMap::new()),
        frame: RefCell::new(Frame::default()),
        inspector: RefCell::new(None),
        sub: RefCell::new(None),
        first: Cell::new(false),
        pressed: Cell::new(false),
        deferred: Cell::new(false),
        held: Cell::new(0),
        quiet: Cell::new(false),
    });
    let weak = Rc::downgrade(&page);
    let weak_outcome = Rc::downgrade(&page);
    let weak_ctx = Rc::downgrade(ctx);
    let inspector = Inspector::new(
        Rc::new(ctx.client.clone()),
        Rc::new(move || {
            weak_ctx
                .upgrade()
                .is_some_and(|c| c.client.capabilities().iter().any(|s| s == stream::PROCESS))
        }),
        Rc::new(move || {
            if let Some(p) = weak.upgrade() {
                p.close_inspector();
            }
        }),
        Rc::new(move |o: Outcome| {
            if let Some(p) = weak_outcome.upgrade() {
                p.say_outcome(&o);
            }
        }),
    );
    *page.inspector.borrow_mut() = Some(Rc::clone(&inspector));
    // Whatever hides the sheet while a row is selected clears the
    // selection, so the graph is dropped and the sheet stays shut.
    let weak = Rc::downgrade(&page);
    split.connect_show_sidebar_notify(move |split| {
        let Some(p) = weak.upgrade() else { return };
        if p.quiet.get() {
            return;
        }
        let selected = p
            .ctx
            .upgrade()
            .is_some_and(|c| c.apps.borrow().selected.is_some());
        if !split.shows_sidebar() && selected {
            p.close_inspector();
        }
    });

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("mirqab-apps");
    column.append(&toolbar(ctx, &page));
    column.append(&notice);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list(&page))
        .vexpand(true)
        .build();
    column.append(&scroll);
    split.set_content(Some(&column));
    split.set_sidebar(Some(&inspector.root));
    inspector.root.set_visible(false);

    // Sizes in em of the page's font (architecture §7.3): the sheet takes
    // 20 to 26 em, and overlays the list under 60 em of page width.
    let bin = adw::BreakpointBin::new();
    let e = f64::from(em(&bin));
    split.set_sidebar_width_unit(adw::LengthUnit::Px);
    split.set_min_sidebar_width(e * 20.0);
    split.set_max_sidebar_width(e * 26.0);
    split.set_sidebar_width_fraction(0.4);
    bin.set_size_request((e * 20.0) as i32, (e * 12.0) as i32);
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        e * 60.0,
        adw::LengthUnit::Px,
    ));
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    bin.add_breakpoint(narrow);
    bin.set_child(Some(&split));
    bin.set_vexpand(true);

    // Escape while focus is in the page (decision 16): local, so it is
    // asked before the window's own Escape, which hides a popover; the
    // modal dialog takes its own Escape (Cancel) before either.
    let keys = gtk::ShortcutController::new();
    keys.set_scope(gtk::ShortcutScope::Local);
    let weak = Rc::downgrade(&page);
    let clear = gtk::CallbackAction::new(move |_, _| {
        if weak.upgrade().is_some_and(|p| p.escape()) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    if let Some(t) = gtk::ShortcutTrigger::parse_string("Escape") {
        keys.add_shortcut(gtk::Shortcut::new(Some(t), Some(clear)));
    }
    bin.add_controller(keys);

    // Subscribed while on screen, at 2 s (decision 14).
    let p = Rc::clone(&page);
    let weak_ctx = Rc::downgrade(ctx);
    bin.connect_map(move |_| {
        let Some(ctx) = weak_ctx.upgrade() else {
            return;
        };
        let weak: Weak<Page> = Rc::downgrade(&p);
        let sub = ctx.client.subscribe_params(
            stream::APPS,
            2000,
            model::page_params(),
            Rc::new(move |data: &Value| {
                if let Some(page) = weak.upgrade() {
                    page.on_frame(data);
                }
            }),
        );
        *p.sub.borrow_mut() = Some(sub);
    });
    let p = Rc::clone(&page);
    bin.connect_unmap(move |_| {
        p.unmapped();
    });
    (bin.upcast(), Handle(page))
}
