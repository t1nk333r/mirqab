//! The System page (plan 002): the cards top to bottom, fed by one
//! `system` subscription held while the page is on screen. The graphs take
//! their own live subscription at `2m` (widgets/graph.rs).

use crate::app::Ctx;
use crate::tabs::system::cpu_card::CpuCard;
use crate::tabs::system::gpu_card::GpuCard;
use crate::tabs::system::memory_card::MemoryCard;
use crate::tabs::system::overview_strip::OverviewStrip;
use crate::tabs::system::power_card::PowerCard;
use crate::tabs::system::pressure_card::PressureCard;
use crate::tabs::system::temps_card::TempsCard;
use gtk::prelude::*;
use mirqab_proto::stream::{self, SystemData};
use mirqab_view::tabs::system;
use serde_json::Value;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

struct Page {
    ctx: Weak<Ctx>,
    overview: OverviewStrip,
    cpu: CpuCard,
    memory: MemoryCard,
    pressure: PressureCard,
    gpu_box: gtk::Box,
    gpus: RefCell<Vec<GpuCard>>,
    temps: TempsCard,
    power: PowerCard,
    sub: RefCell<Option<Box<dyn Any>>>,
    first: Cell<bool>,
}

impl Page {
    fn update(&self, data: &SystemData) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let unit = ctx.config.borrow().temperature;
        let v = system::view(data, unit);
        self.overview.update(&v.overview);
        self.cpu.graph.card.root.set_visible(v.cpu.is_some());
        if let Some(c) = &v.cpu {
            self.cpu.update(c);
        }
        self.memory.graph.card.root.set_visible(v.memory.is_some());
        if let Some(m) = &v.memory {
            self.memory.update(m);
        }
        self.pressure.update(&v.pressure);
        let ids: Vec<&str> = v.gpus.iter().map(|g| g.id.as_str()).collect();
        let have: Vec<String> = self.gpus.borrow().iter().map(|g| g.id.clone()).collect();
        if ids != have.iter().map(String::as_str).collect::<Vec<_>>() {
            while let Some(c) = self.gpu_box.first_child() {
                self.gpu_box.remove(&c);
            }
            let cards: Vec<GpuCard> = v
                .gpus
                .iter()
                .enumerate()
                .filter_map(|(n, g)| GpuCard::new(&ctx, n, g))
                .collect();
            for c in &cards {
                self.gpu_box.append(&c.graph.card.root);
            }
            *self.gpus.borrow_mut() = cards;
        }
        for (card, g) in self.gpus.borrow().iter().zip(&v.gpus) {
            card.update(g);
        }
        self.gpu_box.set_visible(!v.gpus.is_empty());
        self.temps.update(&v.temps);
        self.power.update(&v.power);
        if !self.first.replace(true) {
            ctx.page_ready("system");
        }
    }
}

pub fn build(ctx: &Rc<Ctx>) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("mirqab-page-column");
    let gpu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    gpu_box.add_css_class("mirqab-page-column");
    let page = Rc::new(Page {
        ctx: Rc::downgrade(ctx),
        overview: OverviewStrip::new(ctx),
        cpu: CpuCard::new(ctx),
        memory: MemoryCard::new(ctx),
        pressure: PressureCard::new(ctx),
        gpu_box: gpu_box.clone(),
        gpus: RefCell::new(Vec::new()),
        temps: TempsCard::new(ctx),
        power: PowerCard::new(ctx),
        sub: RefCell::new(None),
        first: Cell::new(false),
    });
    column.append(&page.overview.card.root);
    column.append(&page.cpu.graph.card.root);
    column.append(&page.memory.graph.card.root);
    column.append(&page.pressure.card.root);
    column.append(&gpu_box);
    column.append(&page.temps.card.root);
    column.append(&page.power.graph.card.root);
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&column)
        .vexpand(true)
        .build();
    root.add_css_class("mirqab-system");

    // The numbers subscribe while the page is on screen, at 2 s; only the
    // graphs go live (architecture §5.3).
    let p = Rc::clone(&page);
    let weak_ctx = Rc::downgrade(ctx);
    root.connect_map(move |_| {
        let Some(ctx) = weak_ctx.upgrade() else {
            return;
        };
        let weak: Weak<Page> = Rc::downgrade(&p);
        let sub = crate::client::Source::subscribe(
            &ctx.client,
            stream::SYSTEM,
            2000,
            Rc::new(move |data: &Value| {
                let Some(page) = weak.upgrade() else { return };
                if let Ok(d) = serde_json::from_value::<SystemData>(data.clone()) {
                    page.update(&d);
                }
            }),
        );
        *p.sub.borrow_mut() = Some(sub);
    });
    let p = Rc::clone(&page);
    root.connect_unmap(move |_| {
        *p.sub.borrow_mut() = None;
    });
    root.upcast()
}
