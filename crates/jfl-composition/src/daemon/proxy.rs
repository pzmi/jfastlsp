//! The proxy's side (plan §5.2): connect, and when no daemon answers, the spawn lock, the second
//! check under it, the spawn and the bounded notify handshake; on every new connection the
//! version handshake, then the open documents, then the unanswered request (IDX-12's obligation
//! 8). Synchronous and std only, so a CLI call starts without a runtime.
//!
//! Each step is one action of `formal/daemon.qnt`'s proxies ([`Proxy::connect`],
//! [`Proxy::take_spawn_lock`], [`Proxy::spawn`], [`Proxy::await_notice`] for `handshake` and
//! `spawnTimeout`, [`Proxy::on_accepted`] for the proxy's half of `accept`, [`Proxy::notice`],
//! and the client's [`Proxy::request`], [`Proxy::did_open`], [`Proxy::did_close`],
//! [`Proxy::quit`]); [`Proxy::call`] runs them in a loop for a real client. Every wait has a
//! deadline (the accept and handshake, each reply, the notify handshake): an expired one is a
//! reset, so a daemon alive but hung cannot hold a client forever.
//!
//! A re-sent document keeps the client's version and text, never renumbered (the diagnostics
//! model's obligation 11).
//!
//! Pinned by the daemon conformance driver and `tests/daemon.rs`.

use std::collections::BTreeMap;
use std::io;
use std::os::fd::BorrowedFd;
use std::path::Path;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use jfl_adapter_ipc::client::{self, ClientConn, ConnectError, Received, RecvError};
use jfl_adapter_ipc::notify::{Notice, NotifyListener, Waited};
use jfl_adapter_ipc::{Flock, LockError, SocketPath, StateDir};
use jfl_protocol::codec;
use jfl_protocol::messages::{
    ClientInfo, ClientKind, ContentChange, DidChange, DidChangeParams, DidClose, DidCloseParams,
    DidOpen, DidOpenParams, Handshake, HandshakeParams, Method, Stop, StopParams,
};
use jfl_protocol::rpc::{ErrorCode, Outcome, RequestId, Response};
use jfl_protocol::{DocumentUri, PROTOCOL_VERSION, Version};

/// The proxy's phase: the model's proxy phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyPhase {
    /// No connection: next it connects.
    Disconnected,
    /// The connect failed: no socket, or nobody listening on it.
    NoDaemon,
    /// Holds the spawn lock.
    Spawning,
    /// Spawned a daemon and waits on the notify socket.
    Awaiting,
    /// Connected, its handshake not answered yet.
    Connecting,
    /// Connected, its documents and request re-sent.
    Ready,
}

/// How a spawned daemon is started.
pub trait Launcher {
    /// Starts `jfl daemon` detached, telling it to report on `notify`.
    fn launch(&mut self, notify: &Path) -> io::Result<Box<dyn Spawned>>;
}

/// A daemon the proxy started.
pub trait Spawned {
    /// Whether it has exited, reaping it then (a dead child ends the notify wait at once).
    fn exited(&mut self) -> bool;

    /// A descriptor that becomes readable when it exits (a pidfd on Linux), which the notify
    /// wait polls beside its socket; `None` where there is none: the wait is then cut into
    /// slices of `jfl_process::EXIT_RECHECK`, [`Spawned::exited`] checked between them.
    fn exit_fd(&self) -> Option<BorrowedFd<'_>> {
        None
    }
}

/// How the notify wait ended. Every outcome releases the spawn lock and connects again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handshook {
    /// The daemon listens.
    Ready,
    /// It failed, for this reason (the lock held, the bind refused).
    Failed(String),
    /// It died without a word.
    Died,
    /// The wait expired while it was still starting.
    TimedOut,
}

/// The waits' budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// A frame written or read: the handshake's answer, a reply.
    pub reply: Duration,
    /// The notify handshake after a spawn.
    pub spawn: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            reply: Duration::from_secs(30),
            spawn: Duration::from_secs(10),
        }
    }
}

/// Why a step failed.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// The step does not apply in the proxy's phase.
    #[error("`{step}` does not apply in phase {phase:?}")]
    Phase {
        /// The step.
        step: &'static str,
        /// The phase.
        phase: ProxyPhase,
    },
    /// A wait's deadline passed.
    #[error("the deadline passed waiting for the daemon")]
    TimedOut,
    /// The budget ran out while every daemon spawned failed; the last one said why.
    #[error("the daemon could not start: {0}")]
    SpawnFailed(String),
    /// The daemon closed the connection.
    #[error("the daemon closed the connection")]
    Closed,
    /// The daemon answered something that is not the expected message.
    #[error("unexpected answer from the daemon: {0}")]
    Protocol(String),
    /// The daemon speaks another protocol major; it was asked to stop.
    #[error("the daemon speaks another protocol major; it was asked to stop")]
    Incompatible,
    /// The spawn lock.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// Anything the OS refused.
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Doc {
    version: i32,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Want {
    id: RequestId,
    body: Vec<u8>,
}

/// One client session's proxy.
pub struct Proxy<L> {
    name: String,
    kind: ClientKind,
    state: StateDir,
    socket: SocketPath,
    launcher: L,
    budget: Budget,
    phase: ProxyPhase,
    conn: Option<ClientConn>,
    hello: Option<RequestId>,
    serial: u64,
    docs: BTreeMap<DocumentUri, Doc>,
    want: Option<Want>,
    spawn_lock: Option<Flock>,
    notify: Option<NotifyListener>,
    child: Option<Box<dyn Spawned>>,
    /// Daemons this proxy started that may still run: reaped as they exit, so a long-lived
    /// proxy collects no zombies.
    spawned: Vec<Box<dyn Spawned>>,
    protocol: Version,
    /// The end of the [`Proxy::call`] in progress: every wait inside it is cut there, so a call
    /// takes at most its budget, not a reply budget per wait.
    until: Option<Instant>,
}

impl<L> std::fmt::Debug for Proxy<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy")
            .field("name", &self.name)
            .field("phase", &self.phase)
            .field("hello", &self.hello)
            .finish_non_exhaustive()
    }
}

/// The spawns of this process, every proxy's: the notify token's low 16 bits.
static SPAWNS: AtomicU16 = AtomicU16::new(0);

fn deadline(budget: Duration) -> impl FnMut() -> Option<Duration> {
    let end = Instant::now().checked_add(budget);
    move || end.and_then(|end| end.checked_duration_since(Instant::now()))
}

impl<L: Launcher> Proxy<L> {
    /// A disconnected proxy named `name` (unique among the state directory's clients: it names
    /// its requests) for the daemon of `state`.
    pub fn new(
        name: &str,
        kind: ClientKind,
        state: StateDir,
        socket: SocketPath,
        launcher: L,
        budget: Budget,
    ) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            state,
            socket,
            launcher,
            budget,
            phase: ProxyPhase::Disconnected,
            conn: None,
            hello: None,
            serial: 0,
            docs: BTreeMap::new(),
            want: None,
            spawn_lock: None,
            notify: None,
            child: None,
            spawned: Vec::new(),
            protocol: PROTOCOL_VERSION,
            until: None,
        }
    }

    /// The same proxy announcing `protocol` in its handshake: a proxy of another major, for the
    /// tests of the version mismatch (ADR 0021).
    #[must_use]
    pub fn with_protocol(mut self, protocol: Version) -> Self {
        self.protocol = protocol;
        self
    }

    /// Reaps the daemons this proxy started that have exited.
    fn reap(&mut self) {
        self.spawned.retain_mut(|child| !child.exited());
    }

    /// The phase.
    #[must_use]
    pub const fn phase(&self) -> ProxyPhase {
        self.phase
    }

    /// The handshake id of the current connection.
    #[must_use]
    pub const fn hello(&self) -> Option<&RequestId> {
        self.hello.as_ref()
    }

    /// Whether it holds the spawn lock.
    #[must_use]
    pub const fn holds_spawn_lock(&self) -> bool {
        self.spawn_lock.is_some()
    }

    /// The client's open documents.
    pub fn docs(&self) -> impl Iterator<Item = &DocumentUri> {
        self.docs.keys()
    }

    /// Whether the client waits for an answer.
    #[must_use]
    pub const fn wants(&self) -> bool {
        self.want.is_some()
    }

    fn expect(&self, step: &'static str, phases: &[ProxyPhase]) -> Result<(), ProxyError> {
        if phases.contains(&self.phase) {
            Ok(())
        } else {
            Err(ProxyError::Phase {
                step,
                phase: self.phase,
            })
        }
    }

    fn next_id(&mut self, what: &str) -> RequestId {
        self.serial = self.serial.saturating_add(1);
        RequestId::String(format!("{}/{what}{}", self.name, self.serial))
    }

    /// The budget of one wait: the reply budget, cut at the end of the call in progress.
    fn wait_budget(&self) -> Duration {
        let reply = self.budget.reply;
        self.until.map_or(reply, |end| {
            reply.min(end.saturating_duration_since(Instant::now()))
        })
    }

    /// Writes one frame. A write whose deadline passed may have left part of the frame on the
    /// wire: the connection is dropped then, so no frame follows a half-written one, and the next
    /// step connects again, re-sending the documents and the request. A daemon gone is left to
    /// be noticed as its end of file (the model's `notice`).
    fn send(&mut self, body: &[u8]) -> bool {
        let budget = self.wait_budget();
        let Some(conn) = self.conn.as_mut() else {
            return false;
        };
        match conn.send(body, &mut deadline(budget)) {
            Ok(()) => true,
            Err(RecvError::TimedOut) => {
                self.reset();
                false
            }
            Err(_) => false,
        }
    }

    fn reset(&mut self) {
        self.conn = None;
        self.hello = None;
        self.phase = ProxyPhase::Disconnected;
    }

    /// Connects: queued in a listening daemon's backlog, with the handshake request sent
    /// (`Connecting`), or no daemon (`NoDaemon`).
    pub fn connect(&mut self) -> Result<ProxyPhase, ProxyError> {
        self.expect("connect", &[ProxyPhase::Disconnected])?;
        match ClientConn::connect(self.socket.path()) {
            Ok(conn) => {
                let id = self.next_id("hs");
                let body = codec::encode(&codec::request::<Handshake>(
                    id.clone(),
                    HandshakeParams {
                        protocol: self.protocol,
                        client: ClientInfo {
                            kind: self.kind,
                            version: env!("CARGO_PKG_VERSION").to_owned(),
                        },
                    },
                ))
                .map_err(|e| ProxyError::Protocol(e.to_string()))?;
                self.conn = Some(conn);
                self.hello = Some(id);
                // A daemon that resets the connection meanwhile is noticed on the next read.
                let _sent = self.send(&body);
                self.phase = ProxyPhase::Connecting;
            }
            Err(ConnectError::Absent | ConnectError::Refused) => {
                self.phase = ProxyPhase::NoDaemon;
            }
            Err(ConnectError::Io(e)) => return Err(e.into()),
        }
        Ok(self.phase)
    }

    /// Waits for the spawn lock (`Spawning`): its holder keeps it at most for its bounded spawn
    /// wait, after which this proxy's second check finds the daemon it started. A real client's
    /// way to take it ([`Proxy::call`]); [`Proxy::take_spawn_lock`] is the model's step.
    pub fn wait_spawn_lock(&mut self) -> Result<ProxyPhase, ProxyError> {
        self.expect("takeSpawnLock", &[ProxyPhase::NoDaemon])?;
        self.spawn_lock = Some(Flock::acquire(&self.state.spawn_lock_path())?);
        self.phase = ProxyPhase::Spawning;
        Ok(self.phase)
    }

    /// Tries the spawn lock: `Spawning`, or `Disconnected` while another proxy holds it.
    pub fn take_spawn_lock(&mut self) -> Result<ProxyPhase, ProxyError> {
        self.expect("takeSpawnLock", &[ProxyPhase::NoDaemon])?;
        match Flock::try_acquire(&self.state.spawn_lock_path()) {
            Ok(lock) => {
                self.spawn_lock = Some(lock);
                self.phase = ProxyPhase::Spawning;
            }
            Err(LockError::Held { .. }) => self.phase = ProxyPhase::Disconnected,
            Err(e) => return Err(e.into()),
        }
        Ok(self.phase)
    }

    /// Under the spawn lock, checks again: a daemon a racing proxy started may listen by now,
    /// and then the lock is released and the proxy connects again (`Disconnected`). Otherwise
    /// binds the notify socket and starts the daemon (`Awaiting`).
    pub fn spawn(&mut self) -> Result<ProxyPhase, ProxyError> {
        self.expect("spawn", &[ProxyPhase::Spawning])?;
        if client::listening(self.socket.path()) {
            self.spawn_lock = None;
            self.phase = ProxyPhase::Disconnected;
            return Ok(self.phase);
        }
        self.reap();
        // A fixed-width token, unique among this state directory's spawns: the process's pid and
        // its spawn count, shared by every proxy of the process (only the spawn lock's holder
        // binds one, and an abandoned spawn's path is gone long before the count wraps).
        let spawns = SPAWNS.fetch_add(1, Ordering::Relaxed);
        let token = (u64::from(std::process::id()) << 16) | u64::from(spawns);
        let path = self.state.notify_path(token);
        let started = NotifyListener::bind(&path).and_then(|notify| {
            let child = self.launcher.launch(&path)?;
            Ok((notify, child))
        });
        match started {
            Ok((notify, child)) => {
                self.notify = Some(notify);
                self.child = Some(child);
                self.phase = ProxyPhase::Awaiting;
                Ok(self.phase)
            }
            Err(e) => {
                self.spawn_lock = None;
                self.phase = ProxyPhase::Disconnected;
                Err(e.into())
            }
        }
    }

    /// Waits at most `wait` for the spawned daemon's notice (zero: only one already sent), or
    /// its death. Whatever the outcome, releases the spawn lock and connects again.
    pub fn await_notice(&mut self, wait: Duration) -> Result<Handshook, ProxyError> {
        self.expect("handshake", &[ProxyPhase::Awaiting])?;
        let mut remaining = deadline(wait);
        let outcome = loop {
            let left = remaining().unwrap_or_default();
            let (Some(notify), Some(child)) = (&self.notify, &mut self.child) else {
                break Handshook::Died;
            };
            let exit = child.exit_fd();
            // With an exit descriptor one poll covers the whole wait; without one, slices.
            let slice = if exit.is_some() {
                left
            } else {
                left.min(jfl_process::EXIT_RECHECK)
            };
            let polled = exit.is_some();
            match notify.recv(slice, exit)? {
                Waited::Notice(Notice::Ready { .. }) => break Handshook::Ready,
                Waited::Notice(Notice::Failed { reason }) => break Handshook::Failed(reason),
                Waited::Exited => break Handshook::Died,
                Waited::Nothing => {}
            }
            if !polled && child.exited() {
                break Handshook::Died;
            }
            if remaining().is_none_or(|left| left.is_zero()) {
                break Handshook::TimedOut;
            }
        };
        self.notify = None;
        if let Some(child) = self.child.take() {
            self.spawned.push(child);
        }
        self.reap();
        self.spawn_lock = None;
        self.phase = ProxyPhase::Disconnected;
        Ok(outcome)
    }

    /// The proxy's half of the daemon's accept: reads the handshake's answer, then re-sends the
    /// open documents and the unanswered request. Returns how many frames it sent after the
    /// handshake. Another major: asks the daemon to stop and resets ([`ProxyError::Incompatible`]).
    pub fn on_accepted(&mut self) -> Result<usize, ProxyError> {
        self.expect("accept", &[ProxyPhase::Connecting])?;
        let body = self.recv()?;
        let response = codec::decode_response::<Handshake>(&body);
        match response.map(|r| r.outcome) {
            Ok(Outcome::Result(_)) => {}
            Ok(Outcome::Error(e)) if e.code == ErrorCode::VersionMismatch => {
                self.stop_incompatible();
                return Err(ProxyError::Incompatible);
            }
            Ok(Outcome::Error(e)) => return Err(ProxyError::Protocol(e.message)),
            Err(jfl_protocol::DecodeError::MajorMismatch(_)) => {
                self.stop_incompatible();
                return Err(ProxyError::Incompatible);
            }
            Err(e) => return Err(ProxyError::Protocol(e.to_string())),
        }
        let mut frames = Vec::with_capacity(self.docs.len().saturating_add(1));
        for (uri, doc) in &self.docs {
            frames.push(
                codec::encode(&codec::notification::<DidOpen>(DidOpenParams {
                    uri: uri.clone(),
                    version: doc.version,
                    text: doc.text.clone(),
                }))
                .map_err(|e| ProxyError::Protocol(e.to_string()))?,
            );
        }
        if let Some(want) = &self.want {
            frames.push(want.body.clone());
        }
        for frame in &frames {
            if !self.send(frame) {
                // The daemon went away meanwhile: noticed as its EOF.
                break;
            }
        }
        self.phase = ProxyPhase::Ready;
        Ok(frames.len())
    }

    /// Asks a daemon of another major to stop (`jfl/stop`, frozen across majors, ADR 0021) on
    /// the connection it refused, waits for its answer and the connection's end (it has stopped
    /// accepting then), and resets: the next connect finds no daemon and spawns this build's.
    fn stop_incompatible(&mut self) {
        let id = self.next_id("stop");
        let stop = codec::encode(&codec::request::<Stop>(id, StopParams {})).unwrap_or_default();
        if self.send(&stop) {
            // Its answer, then its EOF; a daemon that does neither within the budget is left to
            // the next connect.
            let _answer = self.recv();
            let _end = self.recv();
        }
        self.reset();
    }

    fn recv(&mut self) -> Result<Vec<u8>, ProxyError> {
        let reply = self.wait_budget();
        let conn = self.conn.as_mut().ok_or(ProxyError::Closed)?;
        match conn.recv(&mut deadline(reply)) {
            Ok(Received::Frame(body)) => Ok(body),
            Ok(Received::Eof) => Err(ProxyError::Closed),
            Err(RecvError::TimedOut) => Err(ProxyError::TimedOut),
            Err(RecvError::Framing(e)) => Err(ProxyError::Protocol(e.to_string())),
            Err(RecvError::Io(e)) => Err(ProxyError::Io(e)),
        }
    }

    /// Reads the connection's EOF: the daemon died or stopped. The client's request stays
    /// wanted; the proxy connects again.
    pub fn notice(&mut self) -> Result<(), ProxyError> {
        self.expect("notice", &[ProxyPhase::Connecting, ProxyPhase::Ready])?;
        match self.recv() {
            Err(ProxyError::Closed) => {
                self.reset();
                Ok(())
            }
            Ok(_) => Err(ProxyError::Protocol(
                "a frame where the connection's end was expected".to_owned(),
            )),
            Err(e) => Err(e),
        }
    }

    /// The client asks: the request `M` with `params`, sent at once by a ready proxy (whether it
    /// was is returned) and on every new connection until answered.
    pub fn request<M: Method>(&mut self, params: M::Params) -> Result<bool, ProxyError> {
        if self.want.is_some() {
            return Err(ProxyError::Protocol(
                "a request is already waiting".to_owned(),
            ));
        }
        let id = self.next_id("r");
        let body = codec::encode(&codec::request::<M>(id.clone(), params))
            .map_err(|e| ProxyError::Protocol(e.to_string()))?;
        let sent = self.phase == ProxyPhase::Ready && self.send(&body);
        self.want = Some(Want { id, body });
        Ok(sent)
    }

    /// Reads the answer to the waiting request; it is answered then.
    pub fn read_answer<M: Method>(&mut self) -> Result<Response<M::Result>, ProxyError> {
        self.expect("answer", &[ProxyPhase::Ready])?;
        let body = self.recv()?;
        let response =
            codec::decode_response::<M>(&body).map_err(|e| ProxyError::Protocol(e.to_string()))?;
        if self.want.as_ref().map(|w| &w.id) != response.id.as_ref() {
            return Err(ProxyError::Protocol(
                "an answer to another request".to_owned(),
            ));
        }
        self.want = None;
        Ok(response)
    }

    /// The client opens a document; a ready proxy forwards it (whether it did is returned).
    pub fn did_open(&mut self, uri: DocumentUri, version: i32, text: String) -> bool {
        let body = codec::encode(&codec::notification::<DidOpen>(DidOpenParams {
            uri: uri.clone(),
            version,
            text: text.clone(),
        }))
        .unwrap_or_default();
        self.docs.insert(uri, Doc { version, text });
        self.phase == ProxyPhase::Ready && self.send(&body)
    }

    /// The client changed a document to `text` (the whole text: the LSP proxy applies a client's
    /// ranged edits itself); a ready proxy forwards it as one full-text change (whether it did is
    /// returned). A document not open is opened, so a re-send carries it either way.
    pub fn did_change(&mut self, uri: DocumentUri, version: i32, text: String) -> bool {
        if !self.docs.contains_key(&uri) {
            return self.did_open(uri, version, text);
        }
        let body = codec::encode(&codec::notification::<DidChange>(DidChangeParams {
            uri: uri.clone(),
            version,
            position_encoding: jfl_domain::PositionEncoding::Utf16,
            changes: vec![ContentChange {
                range: None,
                text: text.clone(),
            }],
        }))
        .unwrap_or_default();
        self.docs.insert(uri, Doc { version, text });
        self.phase == ProxyPhase::Ready && self.send(&body)
    }

    /// The client closes a document; a ready proxy forwards it (whether it did is returned).
    pub fn did_close(&mut self, uri: &DocumentUri) -> bool {
        self.docs.remove(uri);
        let body = codec::encode(&codec::notification::<DidClose>(DidCloseParams {
            uri: uri.clone(),
        }))
        .unwrap_or_default();
        self.phase == ProxyPhase::Ready && self.send(&body)
    }

    /// The client gives up waiting for its request (its deadline passed, or the call failed): the
    /// request is dropped, and so is the connection, whose late answer to it would otherwise be
    /// read as the next one's. The documents stay, re-sent on the next connection.
    pub fn abandon(&mut self) {
        self.want = None;
        self.reset();
    }

    /// The client session ends: its connection closed, its spawn lock released, its documents
    /// and request dropped; a new session takes the proxy.
    pub fn quit(&mut self) {
        self.conn = None;
        self.hello = None;
        self.spawn_lock = None;
        self.notify = None;
        self.child = None;
        self.docs.clear();
        self.want = None;
        self.phase = ProxyPhase::Disconnected;
    }

    /// Asks `M` with `params` and waits for the answer, connecting, spawning and reconnecting as
    /// needed, within `budget` in all. A lost connection re-sends the documents and the request
    /// on the next one.
    pub fn call<M: Method>(
        &mut self,
        params: M::Params,
        budget: Duration,
    ) -> Result<Response<M::Result>, ProxyError> {
        let mut remaining = deadline(budget);
        self.until = Instant::now().checked_add(budget);
        let answer = self.call_until::<M>(params, &mut remaining);
        self.until = None;
        answer
    }

    fn call_until<M: Method>(
        &mut self,
        params: M::Params,
        remaining: &mut impl FnMut() -> Option<Duration>,
    ) -> Result<Response<M::Result>, ProxyError> {
        let mut failure = None;
        let mut spawned_here = false;
        self.request::<M>(params)?;
        loop {
            if remaining().is_none() {
                return Err(failure.map_or(ProxyError::TimedOut, ProxyError::SpawnFailed));
            }
            match self.phase {
                ProxyPhase::Disconnected => {
                    self.connect()?;
                }
                ProxyPhase::NoDaemon => {
                    self.wait_spawn_lock()?;
                }
                ProxyPhase::Spawning => {
                    spawned_here |= self.spawn()? == ProxyPhase::Awaiting;
                }
                ProxyPhase::Awaiting => {
                    let wait = remaining().unwrap_or_default().min(self.budget.spawn);
                    if let Handshook::Failed(reason) = self.await_notice(wait)? {
                        failure = Some(reason);
                    }
                }
                ProxyPhase::Connecting => match self.on_accepted() {
                    Ok(_) => {}
                    Err(ProxyError::Closed | ProxyError::TimedOut) => self.reset(),
                    // The old daemon was asked to stop: connect again and spawn this build's,
                    // unless this call's own spawn is the one of another major (a broken install).
                    Err(ProxyError::Incompatible) if !spawned_here => {}
                    Err(e) => return Err(e),
                },
                ProxyPhase::Ready => match self.read_answer::<M>() {
                    Ok(response) => return Ok(response),
                    Err(ProxyError::Closed | ProxyError::TimedOut) => self.reset(),
                    Err(e) => return Err(e),
                },
            }
        }
    }
}
