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

/// Whether a subscriber's (or a `get`'s) params are ones the stream can
/// serve: Err with the reason makes the request `bad-request` before
/// anything is sampled (the `process` stream needs a `pid`, plan 006).
pub type Accepts = fn(&Value) -> Result<(), String>;

/// The answer to a query: the data, or an error code and message.
pub type QueryResult = Result<Value, (Code, String)>;

/// How a query answers (plan 006, part 4, decision 10): at once, or with
/// the slow rest left to a closure the server runs after it released the
/// sampler's lock, so a subscriber's tick never waits behind it (the
/// inspector's `pacman` call, up to 5 s).
pub enum Answer {
    Ready(QueryResult),
    Later(Box<dyn FnOnce() -> QueryResult + Send>),
}

impl Answer {
    /// The result, running the closure of a `Later` here.
    pub fn finish(self) -> QueryResult {
        match self {
            Answer::Ready(r) => r,
            Answer::Later(f) => f(),
        }
    }
}

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

    /// Called under the lock before `snapshot` for a `get`. A sampler whose
    /// rates need two readings takes the first here and returns how long
    /// the second should wait; the server releases the lock for that time,
    /// so the wait never holds up a subscriber (plan 006, part 4,
    /// decision 10). None: `snapshot` can answer at once.
    fn prime(&mut self, _root: &Root, _now: Instant) -> Option<Duration> {
        None
    }

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

    /// The check a subscription's and a `get`'s params must pass; None
    /// takes any. A plain function, copied out once like `view`, so a
    /// subscribe never waits on the sampler's lock.
    fn accepts(&self) -> Option<Accepts> {
        None
    }

    /// The distinct params of every subscriber, handed over before each
    /// sample (or a `get`'s alone, before its snapshot), for a sampler
    /// that reads only what its subscribers name: the `process` stream
    /// reads the pids they ask for. Most samplers read everything and
    /// ignore it.
    fn wants(&mut self, _params: &[Value]) {}

    /// The stream was left with no subscriber: forget what was kept for
    /// them and stop what ran for them. Called under the sampler's lock
    /// once, when the last subscriber left (within one interval), and so
    /// always before the next subscriber's first sample (the `process`
    /// stream drops its pins and baseline, review of PR #18; `apps` stops
    /// its focus source, plan 006 part 4).
    fn idle(&mut self) {}

    /// `get <stream>.<name>`: a request about the stream's subject that is
    /// not a sample of it (`get apps.inspect {pid}`, plan 006). None when
    /// the sampler has no query by that name.
    fn query(&mut self, _root: &Root, _name: &str, _params: &Value) -> Option<Answer> {
        None
    }

    /// Called once a minute, subscribers or not (phase C). Returns (series
    /// key, scaled value) pairs.
    fn minute(&mut self, _root: &Root) -> Vec<(&'static str, i64)> {
        Vec::new()
    }

    /// Called once a minute after `minute`, subscribers or not, with the
    /// effective config: the `event` rows to write for the minute that
    /// ended, as (kind, data). The `apps` stream's `top-apps` row (plan
    /// 006, part 4, decision 7).
    fn minute_events(&mut self, _root: &Root, _config: &Value) -> Vec<(&'static str, Value)> {
        Vec::new()
    }

    /// Keys this sampler adds to `status`, as (key, value): `apps` says
    /// which focus source runs (`focus`).
    fn status(&self) -> Vec<(&'static str, Value)> {
        Vec::new()
    }
}
