//! The sampler trait: one per stream (architecture §5.3). A sampler runs only
//! while something subscribes; the server owns its thread and its interval.

use crate::proto::Code;
use crate::root::Root;
use serde_json::Value;
use std::time::{Duration, Instant};

/// One subscriber's view of a sample: `(sample, params) -> data`. The
/// sampler measures once for everyone, and each subscriber (and each
/// `get`) receives the sample shaped by its own params, such as the
/// `apps` stream's `sort`, `limit` and `expand` (plan 006).
pub type View = fn(&Value, &Value) -> Value;

/// The answer to a query: the data, or an error code and message.
pub type QueryResult = Result<Value, (Code, String)>;

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

    /// One reading for `get`, outside any subscription. A sampler with rate
    /// baselines overrides it so a `get` does not move a subscriber's
    /// baseline (review of plan 001 phase B).
    fn snapshot(&mut self, root: &Root, now: Instant) -> Value {
        self.sample(root, now)
    }

    /// How a subscriber's params shape a sample; None sends every
    /// subscriber the sample as it is.
    fn view(&self) -> Option<View> {
        None
    }

    /// `get <stream>.<name>`: a request about the stream's subject that is
    /// not a sample of it (`get apps.inspect {pid}`, plan 006). None when
    /// the sampler has no query by that name.
    fn query(&mut self, _root: &Root, _name: &str, _params: &Value) -> Option<QueryResult> {
        None
    }

    /// Called once a minute, subscribers or not (phase C). Returns (series
    /// key, scaled value) pairs.
    fn minute(&mut self, _root: &Root) -> Vec<(&'static str, i64)> {
        Vec::new()
    }
}
