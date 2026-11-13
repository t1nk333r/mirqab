//! The sampler trait: one per stream (architecture §5.3). A sampler runs only
//! while something subscribes; the server owns its thread and its interval.

use crate::root::Root;
use serde_json::Value;
use std::time::{Duration, Instant};

pub trait Sampler: Send {
    fn stream(&self) -> &'static str;

    /// Whether a `live: true` subscription may go below the 2 s floor, down
    /// to `live.intervalMs` (§5.3). Only streams with a live graph say yes.
    fn live(&self) -> bool {
        false
    }

    /// The interval when a subscriber names none.
    fn default_interval(&self) -> Duration {
        Duration::from_secs(2)
    }

    /// Err when this machine cannot serve the stream; the reason goes to
    /// `status` under `unavailable` and the stream leaves `capabilities`.
    fn probe(&mut self, root: &Root) -> Result<(), String>;

    fn sample(&mut self, root: &Root, now: Instant) -> Value;

    /// Called once a minute, subscribers or not (phase C). Returns (series
    /// key, scaled value) pairs.
    fn minute(&mut self, _root: &Root) -> Vec<(&'static str, i64)> {
        Vec::new()
    }
}
