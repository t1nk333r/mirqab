//! Callbacks owned by a widget: the theme's (`style::on_change`) and the
//! config's (`Ctx::on_config`). Each is kept with a weak reference to its
//! owner and runs only while the owner lives. A dead owner's callback is
//! dropped on the next registration as well as on the next run, so pages
//! built and dropped between two theme or config changes do not pile up
//! callbacks (audit 2026-11-21, L52 and L58).

use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Callback = Rc<dyn Fn(&glib::Object)>;

pub struct Listeners {
    list: RefCell<Vec<(glib::WeakRef<glib::Object>, Callback)>>,
    /// A run is under way: a nested one only asks for another pass.
    running: Cell<bool>,
    /// A nested run asked for another pass.
    again: Cell<bool>,
}

impl Default for Listeners {
    fn default() -> Self {
        Self::new()
    }
}

impl Listeners {
    pub const fn new() -> Listeners {
        Listeners {
            list: RefCell::new(Vec::new()),
            running: Cell::new(false),
            again: Cell::new(false),
        }
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
        let mut l = self.list.borrow_mut();
        l.retain(|(w, _)| w.upgrade().is_some());
        l.push((weak, f));
    }

    /// Runs every callback whose owner lives. No borrow is held while they
    /// run: one may register another, which waits for the next run.
    ///
    /// A run started from inside a callback does not recurse: it asks for
    /// one more pass, made after the current one ends, and any number of
    /// such asks during a pass make one pass. The nested caller wanted
    /// every callback to see a change; the next pass shows it to all of
    /// them, the ones the current pass already ran included. Skipping it
    /// would lose that change, and recursing has no bound (review of the
    /// audit 2026-11-21 lows). A callback that asks on every pass still
    /// never ends, as before, but no longer overflows the stack.
    pub fn run(&self) {
        if self.running.replace(true) {
            self.again.set(true);
            return;
        }
        loop {
            self.again.set(false);
            let live: Vec<(glib::Object, Callback)> = {
                let mut l = self.list.borrow_mut();
                l.retain(|(w, _)| w.upgrade().is_some());
                l.iter()
                    .filter_map(|(w, f)| w.upgrade().map(|o| (o, Rc::clone(f))))
                    .collect()
            };
            for (o, f) in live {
                f(&o);
            }
            if !self.again.get() {
                break;
            }
        }
        self.running.set(false);
    }

    /// Callbacks held, dead owners' included until the next prune (tests).
    pub fn len(&self) -> usize {
        self.list.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Listeners;
    use gtk::glib;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Review of the audit-lows-ui branch: a callback that starts another
    /// run recursed once per run. A nested run is one more pass after the
    /// current one, however many times it was asked for.
    #[test]
    fn a_run_started_from_a_callback_is_one_more_pass_not_a_recursion() {
        thread_local! {
            static L: Listeners = const { Listeners::new() };
        }
        let owner = glib::Object::new::<glib::Object>();
        let calls = Rc::new(Cell::new(0u32));
        let depth = Rc::new(Cell::new(0u32));
        let deepest = Rc::new(Cell::new(0u32));
        let (c, d, m) = (calls.clone(), depth.clone(), deepest.clone());
        L.with(|l| {
            l.add(&owner, move |_| {
                c.set(c.get() + 1);
                d.set(d.get() + 1);
                m.set(m.get().max(d.get()));
                // Two nested runs on each of the first two calls.
                if c.get() <= 2 {
                    L.with(Listeners::run);
                    L.with(Listeners::run);
                }
                d.set(d.get() - 1);
            })
        });
        L.with(Listeners::run);
        assert_eq!(deepest.get(), 1, "a callback ran inside itself");
        assert_eq!(
            calls.get(),
            3,
            "the first pass and one more per pass that asked"
        );
        L.with(Listeners::run);
        assert_eq!(calls.get(), 4, "a later run is a run again");
    }
}
