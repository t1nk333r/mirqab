//! The System tab (plan 002): CPU, memory, pressure, GPUs, temperatures and
//! power, from the `system` stream; the view model is
//! `mirqab_view::tabs::system`. Each card stands alone, so Home can pin it
//! (plan 003).

pub mod cores_grid;
pub mod cpu_card;
pub mod gpu_card;
pub mod graph_card;
pub mod memory_card;
pub mod overview_strip;
pub mod page;
pub mod power_card;
pub mod pressure_card;
pub mod temps_card;

use crate::app::Ctx;
use mirqab_proto::stream::{self, SystemData};
use mirqab_view::tabs::system::{self as model, SystemView};
use serde_json::Value;
use std::rc::Weak;

/// The view of a `system` event, in the user's temperature unit; None for
/// another stream or a shape that does not parse.
pub fn view_of(ctx: &Weak<Ctx>, stream_name: &str, data: &Value) -> Option<SystemView> {
    if stream_name != stream::SYSTEM {
        return None;
    }
    let ctx = ctx.upgrade()?;
    let d: SystemData = serde_json::from_value(data.clone()).ok()?;
    let unit = ctx.config.borrow().temperature;
    Some(model::view(&d, unit))
}
