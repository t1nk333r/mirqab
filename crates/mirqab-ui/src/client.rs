//! The daemon's socket on GLib's main loop (plan 001, "Interface shell"):
//! `gio::SocketClient` to the Unix socket, lines read with
//! `read_line_utf8_future`, writes queued on the output stream. No thread
//! and no async runtime; everything runs in `glib::spawn_future_local`.
//!
//! Subscriptions are handles. The daemon keeps one subscription per stream
//! and client, so the handles of one stream are merged: the shortest
//! interval any holds, and `live` while any live handle exists. Dropping
//! the last handle unsubscribes. After a reconnect every stream with
//! handles is subscribed again.

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

type EventFn = Rc<dyn Fn(&Value)>;
type ReplyFn = Box<dyn FnOnce(Result<Value, ErrorBody>)>;
type StateFn = Rc<dyn Fn(&Client)>;

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
    fn wanted(&self) -> Option<Params> {
        let mut it = self.handles.values();
        let first = it.next()?;
        let mut p = first.params.clone();
        for h in it {
            p.interval = p.interval.min(h.params.interval);
            p.live |= h.params.live;
        }
        Some(p)
    }
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
    next_handle: u64,
    connected: bool,
    capabilities: Vec<String>,
    /// Seconds until the next reconnect attempt, doubled each failure.
    backoff: u64,
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
            if let Some(s) = c.0.state.borrow_mut().subs.get_mut(&self.stream) {
                s.handles.remove(&self.id);
            }
            c.sync(&self.stream);
        }
    }
}

/// What a widget needs to take a subscription: the client, or a fake in
/// tests (crates/mirqab-ui/tests/widgets.rs).
pub trait Source {
    /// A subscription without `live`: the 2 s floor applies (§5.3).
    fn subscribe(&self, stream: &str, interval_ms: u64, on_event: EventFn) -> Box<dyn Any>;
    /// A live subscription. Only `widgets/graph.rs` calls this (lint).
    fn subscribe_live(&self, stream: &str, on_event: EventFn) -> Box<dyn Any>;
}

impl Source for Client {
    fn subscribe(&self, stream: &str, interval_ms: u64, on_event: EventFn) -> Box<dyn Any> {
        Box::new(self.subscribe_with(stream, interval_ms, false, Value::Null, on_event))
    }
    fn subscribe_live(&self, stream: &str, on_event: EventFn) -> Box<dyn Any> {
        // The daemon raises 1000 to `live.intervalMs` (§5.3).
        Box::new(self.subscribe_with(stream, 1000, true, Value::Null, on_event))
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
        self.0.state.borrow().backoff
    }

    /// Called on every connect and disconnect.
    pub fn on_state(&self, f: impl Fn(&Client) + 'static) {
        self.0.state.borrow_mut().listeners.push(Rc::new(f));
    }

    /// Stops reconnecting and drops the connection (the process is ending).
    pub fn close(&self) {
        let conn = {
            let mut st = self.0.state.borrow_mut();
            st.closed = true;
            st.generation += 1;
            st.out = None;
            st.conn.take()
        };
        if let Some(conn) = conn {
            let _ = conn.close(gio::Cancellable::NONE);
        }
    }

    /// A subscription with explicit params; `extra` is merged into them
    /// (the `ui` stream's `role` and `pid`).
    pub fn subscribe_with(
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
        let line = {
            let mut st = self.0.state.borrow_mut();
            if !st.connected {
                drop(st);
                on_reply(Err(ErrorBody {
                    code: mirqab_proto::Code::Unavailable,
                    message: strings::DISCONNECTED.to_string(),
                }));
                return;
            }
            st.next_id += 1;
            let id = st.next_id;
            st.pending.insert(id, Box::new(on_reply));
            frame::encode(&Request::new(id, method).with_params(params))
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
            let Some(subs) = st.subs.get_mut(stream) else {
                return;
            };
            let wanted = subs.wanted();
            if wanted == subs.sent {
                return;
            }
            subs.sent = wanted.clone();
            if subs.handles.is_empty() {
                st.subs.remove(stream);
            }
            st.next_id += 1;
            let id = st.next_id;
            let req = match wanted {
                Some(p) => {
                    let mut params = json!({"interval": p.interval});
                    if p.live {
                        params["live"] = json!(true);
                    }
                    if let Value::Object(extra) = p.extra {
                        for (k, v) in extra {
                            params[k] = v;
                        }
                    }
                    Request::new(id, "subscribe")
                        .with_stream(stream)
                        .with_params(params)
                }
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
                match input.read_line_utf8_future(glib::Priority::DEFAULT).await {
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
            st.backoff = (st.backoff * 2).min(30);
            (std::mem::take(&mut st.pending), delay)
        };
        for (_, cb) in pending {
            cb(Err(ErrorBody {
                code: mirqab_proto::Code::Unavailable,
                message: strings::DISCONNECTED.to_string(),
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
