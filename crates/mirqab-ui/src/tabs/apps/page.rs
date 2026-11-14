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
        Item {
            root,
            group,
            process,
            note,
            bound: RefCell::new(None),
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
        self.root
            .update_state(&[gtk::accessible::State::Selected(Some(selected))]);
    }

    fn show_bound(&self) {
        let bound = self.bound.borrow();
        if let Some(obj) = bound.as_ref() {
            self.show(&obj.borrow::<Row>());
        }
    }
}

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
    /// frame.
    fn refresh(&self, tick: bool) {
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
    let split = adw::OverlaySplitView::new();
    split.set_sidebar_position(gtk::PackType::End);
    split.set_enable_show_gesture(false);
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
        *p.sub.borrow_mut() = None;
    });
    bin.upcast()
}
