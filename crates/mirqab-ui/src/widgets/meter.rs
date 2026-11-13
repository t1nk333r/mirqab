//! A horizontal bar for a fraction: the track in `--mirqab-meter-track`,
//! the filled part in a series colour. Drawn with `append_color` inside a
//! rounded clip (architecture §7.4); sized in em.

use crate::style;
use crate::widgets::em;
use gtk::glib;
use gtk::graphene;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Meter {
        pub fraction: Cell<f64>,
        pub series: Cell<usize>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Meter {
        const NAME: &'static str = "MirqabMeter";
        type Type = super::Meter;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("mirqab-meter");
            klass.set_accessible_role(gtk::AccessibleRole::Meter);
        }
    }

    impl ObjectImpl for Meter {}

    impl WidgetImpl for Meter {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let e = em(&*self.obj());
            match orientation {
                gtk::Orientation::Horizontal => ((e * 4.0) as i32, (e * 10.0) as i32, -1, -1),
                _ => {
                    let h = (e * 0.5).ceil() as i32;
                    (h, h, -1, -1)
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let w = self.obj().width() as f32;
            let h = self.obj().height() as f32;
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let bounds = graphene::Rect::new(0.0, 0.0, w, h);
            let radius = graphene::Size::new(h / 2.0, h / 2.0);
            snapshot.push_rounded_clip(&gsk::RoundedRect::new(
                bounds, radius, radius, radius, radius,
            ));
            snapshot.append_color(&style::color("mirqab-meter-track"), &bounds);
            let filled = (self.fraction.get().clamp(0.0, 1.0) as f32) * w;
            if filled > 0.0 {
                snapshot.append_color(
                    &style::series(self.series.get()),
                    &graphene::Rect::new(0.0, 0.0, filled, h),
                );
            }
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct Meter(ObjectSubclass<imp::Meter>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter {
    pub fn new() -> Meter {
        let m: Meter = glib::Object::new();
        let weak = m.downgrade();
        style::on_change(move || {
            let Some(m) = weak.upgrade() else {
                return false;
            };
            m.queue_draw();
            true
        });
        m
    }

    /// A fraction from 0 to 1, and its text for assistive technology.
    pub fn set_fraction(&self, fraction: f64, text: &str) {
        self.imp().fraction.set(fraction);
        self.update_property(&[
            gtk::accessible::Property::ValueMin(0.0),
            gtk::accessible::Property::ValueMax(1.0),
            gtk::accessible::Property::ValueNow(fraction.clamp(0.0, 1.0)),
            gtk::accessible::Property::ValueText(text),
        ]);
        self.queue_draw();
    }

    pub fn set_series(&self, n: usize) {
        self.imp().series.set(n);
        self.queue_draw();
    }
}
