//! Callbacks owned by a widget: the theme's (`style::on_change`) and the
//! config's (`Ctx::on_config`). Each is kept with a weak reference to its
//! owner and runs only while the owner lives. A dead owner's callback is
//! dropped on the next registration as well as on the next run, so pages
//! built and dropped between two theme or config changes do not pile up
//! callbacks (audit 2026-11-21, L52 and L58).

use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type Callback = Rc<dyn Fn(&glib::Object)>;

pub struct Listeners(RefCell<Vec<(glib::WeakRef<glib::Object>, Callback)>>);

impl Default for Listeners {
    fn default() -> Self {
        Self::new()
    }
}

impl Listeners {
    pub const fn new() -> Listeners {
        Listeners(RefCell::new(Vec::new()))
    }

    /// Runs `f` with `owner` on every [`run`](Self::run) while `owner`
    /// lives. `f` gets the owner, so it need not hold it.
    pub fn add<W: IsA<glib::Object>>(&self, owner: &W, f: impl Fn(&W) + 'static) {
        let weak = owner.upcast_ref::<glib::Object>().downgrade();
        let f: Callback = Rc::new(move |o: &glib::Object| {
            if let Some(w) = o.downcast_ref::<W>() {
                f(w);
            }
        });
        let mut l = self.0.borrow_mut();
        l.retain(|(w, _)| w.upgrade().is_some());
        l.push((weak, f));
    }

    /// Runs every callback whose owner lives. No borrow is held while they
    /// run: one may register another, which waits for the next run.
    pub fn run(&self) {
        let live: Vec<(glib::Object, Callback)> = {
            let mut l = self.0.borrow_mut();
            l.retain(|(w, _)| w.upgrade().is_some());
            l.iter()
                .filter_map(|(w, f)| w.upgrade().map(|o| (o, Rc::clone(f))))
                .collect()
        };
        for (o, f) in live {
            f(&o);
        }
    }

    /// Callbacks held, dead owners' included until the next prune (tests).
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}
