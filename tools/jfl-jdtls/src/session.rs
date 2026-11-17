//! One backend per worktree root, shared by its clients: the threads around a [`Mux`].
//!
//! One loop thread owns the [`Mux`] and every sink (message passing, no shared mutable state);
//! a reader thread per client and one for the backend feed it through one bounded channel; a
//! writer thread per client and one for the backend drain bounded channels. A client whose
//! channel is full is dropped (a counted refusal) rather than stalling the others. When no client
//! has been attached for the idle timeout, the owner is asked to evict the session: the backend
//! then gets `shutdown` and `exit`, and is killed if it outlives the stop timeout.
//!
//! Pinned by `tests/session.rs`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufReader, Read, Write};
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
use crate::json;
use crate::json::Message;
use crate::mux::{ClientId, Mux, MuxConfig, Output, codes, exit_notification};

/// Events queued for a session's loop. Bounded: readers block when the loop falls behind, which
/// is the backpressure a slow backend or a flood of client messages needs.
pub const EVENT_CAPACITY: usize = 1024;

/// Messages queued for one client before it is dropped as too slow.
pub const CLIENT_CAPACITY: usize = 1024;

/// Messages queued for the backend before the loop waits on it.
pub const BACKEND_CAPACITY: usize = 1024;

/// An event for a session's loop.
pub enum Event {
    /// A client joins; `sink` receives its outgoing frame bodies.
    Attach {
        /// The client.
        client: ClientId,
        /// Where its messages go.
        sink: SyncSender<Vec<u8>>,
    },
    /// A frame body from a client.
    FromClient {
        /// The client.
        client: ClientId,
        /// The body.
        body: Vec<u8>,
    },
    /// A client's stream ended.
    ClientGone {
        /// The client.
        client: ClientId,
    },
    /// A frame body from the backend.
    FromBackend(Vec<u8>),
    /// The backend's output ended.
    BackendGone,
    /// Stop the backend now; `done` is signalled once it is gone.
    Stop {
        /// Signalled when the session has ended.
        done: SyncSender<()>,
    },
}

/// A backend's streams and, for a real one, its process.
pub struct Backend {
    /// Its output.
    pub reader: Box<dyn Read + Send>,
    /// Its input.
    pub writer: Box<dyn Write + Send>,
    /// Its process, killed and reaped when the session ends.
    pub child: Option<Child>,
}

/// Session timing.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The multiplexer's settings.
    pub mux: MuxConfig,
    /// How long a backend with no client lives.
    pub idle_timeout: Duration,
    /// How long a stopping backend gets before it is killed.
    pub stop_timeout: Duration,
    /// How often the loop wakes with nothing to do, to check the two timeouts.
    pub tick: Duration,
}

/// What a session asks of whoever registered it.
pub trait Owner: Send + 'static {
    /// The session has been idle for its timeout. Under the owner's lock, call `still_idle`
    /// (false when an event arrived meanwhile) and, if it is still idle, unregister the session
    /// so nothing new attaches; return whether it was unregistered.
    fn evict(&self, still_idle: &mut dyn FnMut() -> bool) -> bool;
    /// The session has ended; unregister it if it still is.
    fn ended(&self);
}

/// Live numbers of a session, for `status`.
#[derive(Debug, Default)]
pub struct SessionStatus {
    clients: AtomicUsize,
}

impl SessionStatus {
    /// Attached clients.
    #[must_use]
    pub fn clients(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }
}

/// A running session.
pub struct SessionHandle {
    /// Its event channel.
    pub events: SyncSender<Event>,
    /// Its backend's process id, when it is a process.
    pub backend_pid: Option<u32>,
    /// Its live numbers.
    pub status: Arc<SessionStatus>,
    /// Its loop thread.
    pub thread: JoinHandle<()>,
}

/// Starts a session over a backend.
///
/// # Errors
///
/// When a thread cannot be spawned.
pub fn start<O: Owner>(
    config: SessionConfig,
    backend: Backend,
    owner: O,
) -> io::Result<SessionHandle> {
    let (events, rx) = sync_channel(EVENT_CAPACITY);
    let backend_pid = backend.child.as_ref().map(Child::id);
    let reader_events = events.clone();
    let mut reader = BufReader::new(backend.reader);
    thread::Builder::new()
        .name("jdtls-backend-reader".into())
        .spawn(move || {
            while let Ok(Some(body)) = read_frame(&mut reader, MAX_FRAME_BYTES) {
                if reader_events.send(Event::FromBackend(body)).is_err() {
                    return;
                }
            }
            let _ = reader_events.send(Event::BackendGone);
        })?;
    let backend_tx = spawn_writer(backend.writer, BACKEND_CAPACITY, drop)?;
    let status = Arc::new(SessionStatus::default());
    let loop_status = Arc::clone(&status);
    let child = backend.child;
    let thread = thread::Builder::new()
        .name("jdtls-session".into())
        .spawn(move || {
            let mut state = Loop {
                mux: Mux::new(config.mux.clone()),
                config,
                clients: HashMap::new(),
                backend_tx: Some(backend_tx),
                child,
                status: loop_status,
                stopping: None,
                done: Vec::new(),
            };
            state.run(&rx, &owner);
            owner.ended();
        })?;
    Ok(SessionHandle {
        events,
        backend_pid,
        status,
        thread,
    })
}

/// Spawns a thread writing frames to `writer` until every sender is dropped, then hands the
/// writer to `finish` (closing a socket, say, so its reader sees the end).
///
/// # Errors
///
/// When the thread cannot be spawned.
pub fn spawn_writer<W, F>(
    mut writer: W,
    capacity: usize,
    finish: F,
) -> io::Result<SyncSender<Vec<u8>>>
where
    W: Write + Send + 'static,
    F: FnOnce(W) + Send + 'static,
{
    let (tx, rx) = sync_channel::<Vec<u8>>(capacity);
    thread::Builder::new()
        .name("jdtls-writer".into())
        .spawn(move || {
            for body in rx {
                if write_frame(&mut writer, &body).is_err() {
                    break;
                }
            }
            finish(writer);
        })?;
    Ok(tx)
}

/// Reads a client's frames into its session until the stream ends or is malformed, then
/// reports the client gone.
pub fn pump_client<R: Read>(events: &SyncSender<Event>, client: ClientId, reader: R) {
    let mut reader = BufReader::new(reader);
    while let Ok(Some(body)) = read_frame(&mut reader, MAX_FRAME_BYTES) {
        if events.send(Event::FromClient { client, body }).is_err() {
            return;
        }
    }
    let _ = events.send(Event::ClientGone { client });
}

/// Kills a backend's whole process group (it was started as its own group leader) and reaps it.
pub fn kill_group(child: &mut Child) {
    let pid = rustix::process::Pid::from_child(child);
    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    let _ = child.kill();
    let _ = child.wait();
}

struct Stopping {
    deadline: Instant,
}

struct Loop {
    mux: Mux,
    config: SessionConfig,
    clients: HashMap<ClientId, SyncSender<Vec<u8>>>,
    backend_tx: Option<SyncSender<Vec<u8>>>,
    child: Option<Child>,
    status: Arc<SessionStatus>,
    stopping: Option<Stopping>,
    done: Vec<SyncSender<()>>,
}

impl Loop {
    fn run<O: Owner>(&mut self, rx: &Receiver<Event>, owner: &O) {
        // The idle clock starts when the last client leaves, never before the first attach: the
        // daemon attaches the client that caused the session right after starting it.
        let mut idle_since: Option<Instant> = None;
        let mut attached_once = false;
        let mut backlog: VecDeque<Event> = VecDeque::new();
        loop {
            let event = match backlog.pop_front() {
                Some(event) => Some(event),
                None => match rx.recv_timeout(self.config.tick) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            if let Some(event) = event
                && !self.handle(event)
            {
                break;
            }
            self.status
                .clients
                .store(self.clients.len(), Ordering::Relaxed);
            let now = Instant::now();
            if !self.clients.is_empty() {
                attached_once = true;
                idle_since = None;
            } else if attached_once && idle_since.is_none() {
                idle_since = Some(now);
            }
            if let Some(stopping) = &self.stopping {
                if now >= stopping.deadline {
                    break;
                }
                continue;
            }
            let idle_for = idle_since.map(|since| now.saturating_duration_since(since));
            if idle_for.is_some_and(|d| d >= self.config.idle_timeout) {
                let evicted = owner.evict(&mut || match rx.try_recv() {
                    Ok(event) => {
                        backlog.push_back(event);
                        false
                    }
                    Err(_) => true,
                });
                if evicted {
                    self.begin_stop();
                } else {
                    idle_since = Some(now);
                }
            }
        }
        self.finish();
    }

    /// Handles one event; false ends the loop.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Attach { client, sink } => {
                if self.stopping.is_some() {
                    return true;
                }
                self.mux.attach(client);
                self.clients.insert(client, sink);
            }
            Event::FromClient { client, body } => {
                let out = match json::parse(&body) {
                    Ok(msg) => self.mux.from_client(client, msg),
                    Err(_) => self.mux.client_undecodable(client, json::scan_id(&body)),
                };
                self.apply(out);
            }
            Event::ClientGone { client } => self.drop_client(client),
            Event::FromBackend(body) => {
                let out = match json::parse(&body) {
                    Ok(msg) => self.mux.from_backend(msg),
                    Err(_) => self.mux.backend_undecodable(json::scan_id(&body)),
                };
                self.apply(out);
            }
            Event::BackendGone => {
                let out = self.mux.backend_gone();
                self.apply(out);
                return false;
            }
            Event::Stop { done } => {
                self.done.push(done);
                self.clients.clear();
                self.begin_stop();
            }
        }
        true
    }

    fn drop_client(&mut self, client: ClientId) {
        self.clients.remove(&client);
        let out = self.mux.detach(client);
        self.apply(out);
    }

    fn begin_stop(&mut self) {
        if self.stopping.is_some() {
            return;
        }
        let now = Instant::now();
        let deadline = now.checked_add(self.config.stop_timeout).unwrap_or(now);
        self.stopping = Some(Stopping { deadline });
        let shutdown = self.mux.shutdown_request();
        self.send_backend(&shutdown);
    }

    fn send_backend(&mut self, msg: &Message) {
        let Some(tx) = &self.backend_tx else { return };
        let Ok(body) = json::encode(msg) else { return };
        if tx.send(body).is_err() {
            self.backend_tx = None;
        }
    }

    fn apply(&mut self, out: Vec<Output>) {
        let mut work: VecDeque<Output> = out.into();
        while let Some(output) = work.pop_front() {
            match output {
                Output::Backend(msg) => self.send_backend(&msg),
                Output::Client(client, msg) => {
                    let Some(sink) = self.clients.get(&client) else {
                        continue;
                    };
                    // Encoding a decoded envelope cannot fail; should it, the client still gets
                    // an answer under the same id rather than waiting for ever.
                    let body = json::encode(&msg).or_else(|_| {
                        let id = msg.id.clone().unwrap_or(json::Value::Null);
                        json::encode(&Message::error(
                            id,
                            codes::INTERNAL_ERROR,
                            "message does not encode",
                        ))
                    });
                    let Ok(body) = body else { continue };
                    match sink.try_send(body) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                            self.clients.remove(&client);
                            work.extend(self.mux.detach(client));
                        }
                    }
                }
                Output::Close(client) => {
                    self.clients.remove(&client);
                }
                Output::ShutdownAcknowledged => {
                    self.send_backend(&exit_notification());
                    self.backend_tx = None;
                }
            }
        }
    }

    fn finish(&mut self) {
        self.backend_tx = None;
        self.clients.clear();
        self.status.clients.store(0, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            // The backend's output has ended or the stop deadline has passed: whatever is left
            // of its process group (the backend if stuck, helpers it started) must not outlive
            // the session.
            kill_group(&mut child);
        }
        for done in self.done.drain(..) {
            let _ = done.send(());
        }
    }
}
