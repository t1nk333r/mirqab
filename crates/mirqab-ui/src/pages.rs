//! The page slot (architecture §7.2): a stack that holds only the pages
//! shown since the window last appeared. A page is built the first time its
//! tab is shown and every page is dropped when the window hides, so a page
//! that subscribes while mapped holds nothing once it is gone.

use crate::app::Ctx;
use crate::tabs;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

pub struct Pages {
    pub stack: gtk::Stack,
    built: RefCell<HashSet<String>>,
}

impl Default for Pages {
    fn default() -> Self {
        Self::new()
    }
}

impl Pages {
    pub fn new() -> Pages {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::None);
        Pages {
            stack,
            built: RefCell::new(HashSet::new()),
        }
    }

    /// Shows `tab`, building its page first if this is its first showing.
    pub fn show(&self, tab: &str, ctx: &Rc<Ctx>) {
        if self.built.borrow_mut().insert(tab.to_string()) {
            let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
            page.add_css_class("mirqab-page");
            page.set_widget_name(tab);
            let content = tabs::build(tab, ctx);
            content.set_vexpand(true);
            page.append(&content);
            self.stack.add_named(&page, Some(tab));
        }
        self.stack.set_visible_child_name(tab);
    }

    /// Drops every page.
    pub fn clear(&self) {
        while let Some(c) = self.stack.first_child() {
            self.stack.remove(&c);
        }
        self.built.borrow_mut().clear();
    }

    /// The tabs whose pages exist now (the selftest reports them).
    pub fn built(&self) -> Vec<String> {
        let mut v: Vec<String> = self.built.borrow().iter().cloned().collect();
        v.sort();
        v
    }
}
