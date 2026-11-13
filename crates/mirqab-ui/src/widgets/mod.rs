//! The interface's widgets, one type per file (architecture §7.2). Switches
//! and buttons are GTK's own; these are the ones mirqab adds.

pub mod badge;
pub mod card;
pub mod confirm_dialog;
pub mod empty_state;
pub mod footer;
pub mod graph;
pub mod icon;
pub mod meter;
pub mod pill;
pub mod range_picker;
pub mod sidebar;
pub mod stat;

use gtk::pango;
use gtk::prelude::*;

/// The widget's font size in logical pixels: custom widgets measure in
/// multiples of it, never in pixel literals (architecture §7.3).
pub fn em(widget: &impl IsA<gtk::Widget>) -> f32 {
    let Some(fd) = widget.pango_context().font_description() else {
        return 16.0;
    };
    let size = fd.size() as f32 / pango::SCALE as f32;
    if size <= 0.0 {
        16.0
    } else if fd.is_size_absolute() {
        size
    } else {
        // Points to logical pixels at the 96 dpi GTK lays out in; the
        // surface's scale is applied by GTK after that.
        size * 96.0 / 72.0
    }
}
