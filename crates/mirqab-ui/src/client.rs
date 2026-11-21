//! The daemon's socket on GLib's main loop (plan 001, "Interface shell"):
//! `gio::SocketClient` to the Unix socket, lines read with
//! `read_line_utf8_future`, writes queued on the output stream. No thread
//! and no async runtime; everything runs in `glib::spawn_future_local`.
//!
//! Subscriptions are handles. The daemon keeps one subscription per stream
//! and client, so the handles of one stream are merged: the shortest
//! interval any holds, and `live` while any live handle exists. Their other
//! params are merged by the stream's rule when one is set (`set_merge`;
//! the `apps` page and the `apps.top` card, plan 006), otherwise the first
//! handle's win. Dropping the last handle unsubscribes. After a reconnect
//! every stream with handles is subscribed again.

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use mirqab_proto::{ErrorBody, Request, ServerFrame, frame, strings};
use serde_json::{Value, json};
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// How long a new connection waits for the daemon's hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);

type EventFn = Rc<dyn Fn(&Value)>;
type ReplyFn = Box<dyn FnOnce(Result<Value, ErrorBody>)>;
type StateFn = Rc<dyn Fn(&Client)>;
/// What a request's reply is handed to, through `Source`.
pub type Reply = Box<dyn FnOnce(Result<Value, ErrorBody>)>;
/// A stream's rule for merging its handles' params into one subscription.
pub type Merge = fn(&[Value]) -> Value;

/// What a stream subscription asks the daemon for.
#[derive(Debug, Clone, PartialEq)]
struct Params {
    interval: u64,
    live: bool,
    extra: Value,
}

struct Handle {
    params: Params,
    on_event: EventFn,
}

#[derive(Default)]
struct StreamSubs {
    handles: BTreeMap<u64, Handle>,
    /// What the daemon currently holds for this stream, as far as we know.
    sent: Option<Params>,
}

impl StreamSubs {
    fn wanted(&self, merge: Option<Merge>) -> Option<Params> {
        let mut it = self.handles.values();
        let first = it.next()?;
        let mut p = first.params.clone();
        for h in it {
            p.interval = p.interval.min(h.params.interval);
            p.live |= h.params.live;
        }
        if let Some(merge) = merge {
            let extras: Vec<Value> = self
                .handles
                .values()
                .map(|h| h.params.extra.clone())
                .collect();
            p.extra = merge(&extras);
        }
        Some(p)
    }

    /// What to send the daemon now, if anything: `Some(Some(p))` subscribes
    /// with `p`, `Some(None)` unsubscribes, `None` sends nothing (it holds
    /// what the handles want already). Marks it sent.
    fn next(&mut self, merge: Option<Merge>) -> Option<Option<Params>> {
        let wanted = self.wanted(merge);
        if wanted == self.sent {
            return None;
        }
        self.sent = wanted.clone();
        Some(wanted)
    }
}

/// A subscribe request's params: the interval, `live` when set, and the
/// stream's own params merged in.
fn subscribe_params(p: Params) -> Value {
    let mut params = json!({"interval": p.interval});
    if p.live {
        params["live"] = json!(true);
    }
    if let Value::Object(extra) = p.extra {
        for (k, v) in extra {
            params[k] = v;
        }
    }
    params
}

#[derive(Default)]
struct State {
    conn: Option<gio::SocketConnection>,
    out: Option<gio::OutputStream>,
    queue: VecDeque<Vec<u8>>,
    writing: bool,
    next_id: u64,
    pending: HashMap<u64, ReplyFn>,
    subs: BTreeMap<String, StreamSubs>,
    merges: HashMap<String, Merge>,
    next_handle: u64,
    connected: bool,
    capabilities: Vec<String>,
    /// Seconds the next failed connection waits, doubled each failure.
    backoff: u64,
    /// Seconds the current wait before reconnecting lasts.
    retry_in: u64,
    /// Bumped on every disconnect, so a stale read loop knows to stop.
    generation: u64,
    listeners: Vec<StateFn>,
    closed: bool,
}

struct Inner {
    path: PathBuf,
    state: RefCell<State>,
}

/// A cheap handle to the one connection; clones share it.
#[derive(Clone)]
pub struct Client(Rc<Inner>);

/// Dropping it ends the subscription (or narrows the merged one).
pub struct Subscription {
    client: Weak<Inner>,
    stream: String,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.client.upgrade() {
            let c = Client(inner);
            // The handle is dropped after the borrow ends: its closure may
            // own another Subscription, whose drop borrows again.
            let removed =
                c.0.state
                    .borrow_mut()
                    .subs
                    .get_mut(&self.stream)
                    .and_then(|s| s.handles.remove(&self.id));
            drop(removed);
            c.sync(&self.stream);
        }
    }
}

/// What a widget needs to take a subscription: the client, or a fake in
/// tests (crates/mirqab-ui/tests/widgets.rs).
pub trait Source {
    /// A subscription without `live`: the 2 s floor applies (§5.3).
    fn subscribe(&self, stream: &str, interval_ms: u64, on_event: EventFn) -> Box<dyn Any>;
    /// A live subscription with the stream's `params` (the `process`
    /// stream's pid and start). Only `widgets/graph.rs` calls this (lint).
    fn subscribe_live(&self, stream: &str, params: Value, on_event: EventFn) -> Box<dyn Any>;
    /// One request; `on_reply` gets the result or the error, with its data.
    fn request(&self, method: &str, params: Value, on_reply: Reply);
    /// One `get` of `stream` (or a query under it, `apps.inspect`).
    fn get(&self, stream: &str, params: Value, on_reply: Reply);
}

impl Source for Client {
    fn subscribe(&self, stream: &str, interval_ms: u64, on_event: EventFn) -> Box<dyn Any> {
        Box::new(self.subscribe_with(stream, interval_ms, false, Value::Null, on_event))
    }
    fn subscribe_live(&self, stream: &str, params: Value, on_event: EventFn) -> Box<dyn Any> {
        // The daemon raises 1000 to `live.intervalMs` (§5.3).
        Box::new(self.subscribe_with(stream, 1000, true, params, on_event))
    }
    fn request(&self, method: &str, params: Value, on_reply: Reply) {
        Client::request(self, method, params, on_reply);
    }
    fn get(&self, stream: &str, params: Value, on_reply: Reply) {
        self.send(Request::new(0, "get").with_stream(stream), params, on_reply);
    }
}

/// The default socket: `$MIRQAB_SOCKET`, else
/// `$XDG_RUNTIME_DIR/mirqab/daemon.sock` (as the daemon decides).
pub fn default_socket() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("MIRQAB_SOCKET").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(s));
    }
    let run = std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty())?;
    Some(PathBuf::from(run).join("mirqab/daemon.sock"))
}

impl Client {
    /// A client for `path`; it connects at once and keeps reconnecting.
    pub fn new(path: PathBuf) -> Client {
        let c = Client(Rc::new(Inner {
            path,
            state: RefCell::new(State {
                backoff: 2,
                ..State::default()
            }),
        }));
        c.connect();
        c
    }

    pub fn connected(&self) -> bool {
        self.0.state.borrow().connected
    }

    /// The streams this machine serves, from the daemon's hello.
    pub fn capabilities(&self) -> Vec<String> {
        self.0.state.borrow().capabilities.clone()
    }

    /// Seconds until the next reconnect, while disconnected.
    pub fn retry_in(&self) -> u64 {
        self.0.state.borrow().retry_in
    }

    /// Called on every connect and disconnect.
    pub fn on_state(&self, f: impl Fn(&Client) + 'static) {
        self.0.state.borrow_mut().listeners.push(Rc::new(f));
    }

    /// Merges the params of `stream`'s handles with `merge` instead of
    /// taking the first handle's. Set once, at start-up, before anything
    /// subscribes to the stream.
    pub fn set_merge(&self, stream: &str, merge: Merge) {
        self.0
            .state
            .borrow_mut()
            .merges
            .insert(stream.to_string(), merge);
        self.sync(stream);
    }

    /// Stops reconnecting and drops the connection (the process is ending).
    pub fn close(&self) {
        let conn = {
            let mut st = self.0.state.borrow_mut();
            st.closed = true;
            st.connected = false;
            st.generation += 1;
            st.out = None;
            st.conn.take()
        };
        if let Some(conn) = conn {
            let _ = conn.close(gio::Cancellable::NONE);
        }
    }

    /// A subscription without `live`, with extra params merged in (the `ui`
    /// stream's `role` and `pid`).
    pub fn subscribe_params(
        &self,
        stream: &str,
        interval_ms: u64,
        extra: Value,
        on_event: EventFn,
    ) -> Subscription {
        self.subscribe_with(stream, interval_ms, false, extra, on_event)
    }

    /// Private, so `live` is reachable only through `Source::subscribe_live`
    /// (review of PR #5: a page could pass `true` here past the lint rule).
    fn subscribe_with(
        &self,
        stream: &str,
        interval_ms: u64,
        live: bool,
        extra: Value,
        on_event: EventFn,
    ) -> Subscription {
        let id = {
            let mut st = self.0.state.borrow_mut();
            st.next_handle += 1;
            let id = st.next_handle;
            st.subs
                .entry(stream.to_string())
                .or_default()
                .handles
                .insert(
                    id,
                    Handle {
                        params: Params {
                            interval: interval_ms,
                            live,
                            extra,
                        },
                        on_event,
                    },
                );
            id
        };
        self.sync(stream);
        Subscription {
            client: Rc::downgrade(&self.0),
            stream: stream.to_string(),
            id,
        }
    }

    /// One request; `on_reply` gets the result or the error. A request made
    /// while disconnected fails at once.
    pub fn request(
        &self,
        method: &str,
        params: Value,
        on_reply: impl FnOnce(Result<Value, ErrorBody>) + 'static,
    ) {
        self.send(Request::new(0, method), params, on_reply);
    }

    /// `req` with its id and `params`; the reply goes to `on_reply`.
    fn send(
        &self,
        req: Request,
        params: Value,
        on_reply: impl FnOnce(Result<Value, ErrorBody>) + 'static,
    ) {
        let line = {
            let mut st = self.0.state.borrow_mut();
            if !st.connected {
                drop(st);
                on_reply(Err(ErrorBody {
                    code: mirqab_proto::Code::Unavailable,
                    message: strings::DISCONNECTED.to_string(),
                    data: None,
                }));
                return;
            }
            st.next_id += 1;
            let id = st.next_id;
            st.pending.insert(id, Box::new(on_reply));
            let mut req = req.with_params(params);
            req.id = id;
            frame::encode(&req)
        };
        self.write(line);
    }

    /// Brings the daemon's subscription for `stream` in line with the
    /// handles held.
    fn sync(&self, stream: &str) {
        let line = {
            let mut st = self.0.state.borrow_mut();
            if !st.connected {
                return;
            }
            let merge = st.merges.get(stream).copied();
            let Some(subs) = st.subs.get_mut(stream) else {
                return;
            };
            let Some(wanted) = subs.next(merge) else {
                return;
            };
            if subs.handles.is_empty() {
                st.subs.remove(stream);
            }
            st.next_id += 1;
            let id = st.next_id;
            let req = match wanted {
                Some(p) => Request::new(id, "subscribe")
                    .with_stream(stream)
                    .with_params(subscribe_params(p)),
                None => Request::new(id, "unsubscribe").with_stream(stream),
            };
            frame::encode(&req)
        };
        self.write(line);
    }

    fn write(&self, line: Vec<u8>) {
        let start = {
            let mut st = self.0.state.borrow_mut();
            st.queue.push_back(line);
            !std::mem::replace(&mut st.writing, true)
        };
        if start {
            let c = self.clone();
            glib::spawn_future_local(async move { c.drain().await });
        }
    }

    async fn drain(&self) {
        loop {
            let (out, line) = {
                let mut st = self.0.state.borrow_mut();
                match (st.out.clone(), st.queue.pop_front()) {
                    (Some(out), Some(line)) => (out, line),
                    _ => {
                        st.writing = false;
                        return;
                    }
                }
            };
            if out
                .write_all_future(line, glib::Priority::DEFAULT)
                .await
                .is_err()
            {
                let mut st = self.0.state.borrow_mut();
                st.writing = false;
                st.queue.clear();
                return;
            }
        }
    }

    fn connect(&self) {
        let c = self.clone();
        let generation = self.0.state.borrow().generation;
        glib::spawn_future_local(async move {
            let addr = gio::UnixSocketAddress::new(&c.0.path);
            let conn = match gio::SocketClient::new().connect_future(&addr).await {
                Ok(conn) => conn,
                Err(_) => {
                    c.disconnected(generation);
                    return;
                }
            };
            let input = gio::DataInputStream::new(&conn.input_stream());
            input.set_newline_type(gio::DataStreamNewlineType::Lf);
            {
                let mut st = c.0.state.borrow_mut();
                if st.closed || st.generation != generation {
                    return;
                }
                st.out = Some(conn.output_stream());
                st.conn = Some(conn);
            }
            loop {
                let read = input.read_line_utf8_future(glib::Priority::DEFAULT);
                let line = if c.0.state.borrow().connected {
                    read.await
                } else {
                    // A stopped daemon whose socket is still bound lets the
                    // connection complete from the backlog and then says
                    // nothing: without a limit no notice shows, no retry
                    // runs and a parked show never happens (audit
                    // 2026-11-21, L49). A timeout drops the read.
                    // mirqab:ui-timeout
                    match glib::future_with_timeout(HELLO_WAIT, read).await {
                        Ok(r) => r,
                        Err(_) => break,
                    }
                };
                match line {
                    Ok(Some(line)) => c.handle_line(&line),
                    _ => break,
                }
                if c.0.state.borrow().generation != generation {
                    return;
                }
            }
            c.disconnected(generation);
        });
    }

    fn handle_line(&self, line: &str) {
        let Ok(frame) = serde_json::from_str::<ServerFrame>(line) else {
            return;
        };
        match frame {
            ServerFrame::Hello(h) => {
                let streams: Vec<String> = {
                    let mut st = self.0.state.borrow_mut();
                    st.connected = true;
                    st.backoff = 2;
                    st.capabilities = h.capabilities;
                    for s in st.subs.values_mut() {
                        s.sent = None;
                    }
                    st.subs.keys().cloned().collect()
                };
                for s in streams {
                    self.sync(&s);
                }
                self.notify();
            }
            ServerFrame::Reply(r) => {
                let cb = self.0.state.borrow_mut().pending.remove(&r.id);
                if let Some(cb) = cb {
                    cb(match r.error {
                        Some(e) => Err(e),
                        None => Ok(r.result.unwrap_or(Value::Null)),
                    });
                }
            }
            ServerFrame::Event(e) => {
                // Callbacks may subscribe or unsubscribe: call them with no
                // borrow held.
                let fns: Vec<EventFn> = self
                    .0
                    .state
                    .borrow()
                    .subs
                    .get(&e.stream)
                    .map(|s| s.handles.values().map(|h| Rc::clone(&h.on_event)).collect())
                    .unwrap_or_default();
                for f in fns {
                    f(&e.data);
                }
            }
        }
    }

    fn disconnected(&self, generation: u64) {
        let (pending, delay) = {
            let mut st = self.0.state.borrow_mut();
            if st.closed || st.generation != generation {
                return;
            }
            st.generation += 1;
            st.connected = false;
            st.out = None;
            st.conn = None;
            st.queue.clear();
            st.writing = false;
            for s in st.subs.values_mut() {
                s.sent = None;
            }
            let delay = st.backoff;
            st.retry_in = delay;
            st.backoff = (st.backoff * 2).min(30);
            (std::mem::take(&mut st.pending), delay)
        };
        for (_, cb) in pending {
            cb(Err(ErrorBody {
                code: mirqab_proto::Code::Unavailable,
                message: strings::DISCONNECTED.to_string(),
                data: None,
            }));
        }
        self.notify();
        let weak = Rc::downgrade(&self.0);
        // mirqab:ui-timeout
        glib::timeout_add_local_once(Duration::from_secs(delay), move || {
            if let Some(inner) = weak.upgrade() {
                Client(inner).connect();
            }
        });
    }

    fn notify(&self) {
        let fns = self.0.state.borrow().listeners.clone();
        for f in fns {
            f(self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle(extra: Value) -> Handle {
        Handle {
            params: Params {
                interval: 2000,
                live: false,
                extra,
            },
            on_event: Rc::new(|_| {}),
        }
    }

    /// Gap 5 of the review of PR #19: when one of two merged handles
    /// drops, the one left is sent with its own params, and the last one
    /// leaving unsubscribes.
    #[test]
    fn a_dropped_handle_leaves_the_others_params() {
        let merge: Option<Merge> = Some(mirqab_view::tabs::apps::merge_params);
        let mut subs = StreamSubs::default();
        subs.handles
            .insert(1, handle(mirqab_view::tabs::apps::page_params()));
        subs.handles.insert(2, handle(Value::Null));
        let both = subs.next(merge).flatten().map(subscribe_params);
        assert_eq!(
            both,
            Some(json!({"interval": 2000, "limit": 1000, "expand": true}))
        );
        assert_eq!(subs.next(merge), None, "nothing new: nothing sent");

        // The page goes: the card's view, the daemon's default limit.
        subs.handles.remove(&1);
        let card = subs.next(merge).flatten().map(subscribe_params);
        assert_eq!(card, Some(json!({"interval": 2000, "limit": 50})));

        // The page again, then the card goes: the daemon already holds the
        // page's view, the merge of the two, so nothing is sent.
        subs.handles
            .insert(3, handle(mirqab_view::tabs::apps::page_params()));
        let page = subs.next(merge).flatten().map(subscribe_params);
        assert_eq!(
            page,
            Some(json!({"interval": 2000, "limit": 1000, "expand": true}))
        );
        subs.handles.remove(&2);
        assert_eq!(subs.next(merge), None, "the page's view is held");

        subs.handles.remove(&3);
        assert_eq!(subs.next(merge), Some(None), "the last one unsubscribes");
    }
}
