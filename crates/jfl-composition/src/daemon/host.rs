//! The daemon host (plan §5.2): the owner's lock, the socket, the connections and their overlays,
//! as steps the conformance driver can take one at a time (IDX-12's obligation 9) and the run
//! loop ([`super::run`]) takes as events arrive.
//!
//! Each step is one action of `formal/daemon.qnt`: [`Host::lock`], [`Host::unlink_stale`],
//! [`Host::listen`], [`Host::accept`] (one queued connection: the version handshake), reading one
//! frame ([`Host::receive`]: an overlay, a request, a closed connection's EOF), [`Host::answer`],
//! [`Host::reap`], and the shutdown's three steps [`Host::stop`], [`Host::unlink`] and
//! [`Host::release`]. The orders are the run loop's: start lock, unlink, bind, ready (a held lock
//! is reported on the notify socket and the daemon exits 1); shutdown stop accepting, unlink,
//! release, never the release first (the model's `releaseBeforeUnlink`, the negative control of
//! the daemon driver).
//!
//! Cleanup is explicit, never a `Drop`: a crash ([`Host::crash`], in the driver an abort of the
//! host) closes every descriptor and runs none of the shutdown, so the socket file stays, stale,
//! and the kernel drops the lock (obligation 6). Overlays and requests are keyed by connection
//! and die with it (obligation 7): on its EOF's reap, on stop and on a crash; nothing persists.
//!
//! **Pushed diagnostics** (LSP-04): a connection that sends `jfl/watchDiagnostics` gets its open
//! documents' syntax diagnostics as `jfl/publishDiagnostics`, through [`super::diagnostics`]: each
//! document notification is counted (the publications' `seq`) and handed on, the compiles run on
//! the blocking pool and come back through [`Host::take_compiled`]'s channel, and each publication
//! is queued like a reply. The watch dies with its connection.
//!
//! Pinned by the daemon conformance driver (`crates/jfl-conformance/src/daemon.rs`, every
//! regression trace and `conformance-replay`'s generated ones) and by
//! `tests/daemon.rs`.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::PathBuf;

use jfl_adapter_ipc::notify::{self, Notice};
use jfl_adapter_ipc::{
    Accepted, Call, ConnReader, ConnWriter, FileId, Flock, Incoming, Listener, LockError, Note,
    SocketPath, StateDir,
};
use jfl_protocol::codec;
use jfl_protocol::messages::{
    Cancel, CancelParams, DidChange, DidChangeParams, DidClose, DidCloseParams, DidOpen,
    DidOpenParams, Handshake, HandshakeParams, HandshakeResult, Method, Notify, ServerInfo, Stop,
    WatchDiagnostics, WatchDiagnosticsParams, version_mismatch,
};
use jfl_protocol::rpc::{ErrorCode, ErrorObject, Outcome, RequestId, Response};
use jfl_protocol::{DocumentUri, PROTOCOL_VERSION};
use tokio::sync::mpsc;

use super::diagnostics::{COMPILED_QUEUE, Change, Compiled, Diagnostics, Outgoing, PoolRunner};
use super::service::{Context, Overlay, Service, error_body, result_body};

/// Where a daemon lives.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// The checked state directory.
    pub state: StateDir,
    /// Its socket path, checked for length.
    pub socket: SocketPath,
    /// The uid every peer must have.
    pub uid: u32,
    /// The proxy's notify socket, when a proxy spawned this daemon.
    pub notify: Option<PathBuf>,
}

/// A host's phase: the model's daemon phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Running, before the lock.
    Started,
    /// Holds the lock, nothing bound.
    Locked,
    /// Bound, `0600`, listening, ready reported.
    Serving,
    /// Stopped accepting, every connection closed.
    Stopping,
    /// Its socket file removed, the lock still held.
    Unlinked,
    /// Released the lock: exits 0.
    Exited,
    /// The lock was held or the bind failed: reported, exits 1.
    Failed,
    /// Aborted: every descriptor closed, nothing cleaned up.
    Crashed,
}

/// A connection's number, in the order the host opened them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnId(pub u64);

/// Why a step was not taken.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The step does not apply in the host's phase.
    #[error("`{step}` does not apply in phase {phase:?}")]
    Phase {
        /// The step.
        step: &'static str,
        /// The phase.
        phase: Phase,
    },
    /// No such open connection.
    #[error("no open connection {0:?}")]
    NoConnection(ConnId),
    /// The connection's peer has not closed it.
    #[error("connection {0:?} is still open")]
    StillOpen(ConnId),
    /// The connection holds no request.
    #[error("connection {0:?} holds no request")]
    NoRequest(ConnId),
    /// The idle exit while a connection is accepted.
    #[error("the daemon is not idle")]
    NotIdle,
    /// The peer's handshake was refused (another major); its connection is closed.
    #[error("the peer speaks another protocol major")]
    Incompatible,
    /// The lock.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// Anything the OS refused.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Where a connection's replies go.
#[derive(Debug)]
pub enum Outbox {
    /// Written at once, awaited (the stepped host: the driver reads each reply after its step).
    Direct(ConnWriter),
    /// Queued to the connection's writer task (the run loop), which writes each under its
    /// deadline: the loop never waits on a peer. A full queue severs the connection.
    Queue(mpsc::Sender<Vec<u8>>),
}

impl Outbox {
    /// Sends `body`; `false` when the connection must be severed (its queue is full or its
    /// writer gone). A direct write's failure is a peer gone meanwhile, whose EOF comes next.
    async fn send(&mut self, body: Vec<u8>) -> bool {
        match self {
            Self::Direct(writer) => {
                let _gone = writer.send(&body).await;
                true
            }
            Self::Queue(queue) => queue.try_send(body).is_ok(),
        }
    }
}

/// One accepted connection.
#[derive(Debug)]
pub struct Conn {
    reader: Option<Box<ConnReader>>,
    out: Outbox,
    /// Its handshake announced another protocol major: only `jfl/stop` is served (ADR 0021).
    mismatched: bool,
    peer_closed: bool,
    overlays: BTreeMap<DocumentUri, Overlay>,
    requests: VecDeque<Call>,
    /// The document notifications applied (`jfl/didOpen`, `jfl/didChange`, `jfl/didClose`): the
    /// `seq` of pushed diagnostics.
    applied: u64,
}

impl Conn {
    /// Whether its peer closed it (the daemon read its EOF) and it awaits its reap.
    #[must_use]
    pub const fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    /// Its open documents.
    #[must_use]
    pub const fn overlays(&self) -> &BTreeMap<DocumentUri, Overlay> {
        &self.overlays
    }

    /// Whether it holds an unanswered request.
    #[must_use]
    pub fn inflight(&self) -> bool {
        !self.requests.is_empty()
    }

    /// Whether its handshake announced another protocol major.
    #[must_use]
    pub const fn mismatched(&self) -> bool {
        self.mismatched
    }
}

/// A connection taken from the kernel's backlog whose handshake has not been answered.
#[derive(Debug)]
struct Pending {
    hello: Call,
    reader: Box<ConnReader>,
    writer: ConnWriter,
}

/// What a tick found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// The lock and the socket are where they were.
    Fine,
    /// The lock path no longer names the locked file (obligation 1): shut down.
    LockLost,
    /// The socket path no longer names the bound socket (the model's `sweep`): shut down.
    SocketLost,
}

/// What reading one frame did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Received {
    /// An overlay changed, or a notification was ignored.
    Applied,
    /// A request was queued.
    Request,
    /// The peer closed the connection: its state awaits the reap.
    Closed,
    /// A frame that was not a message was answered with an error.
    Refused,
}

/// The daemon's state.
#[derive(Debug)]
pub struct Host {
    config: HostConfig,
    server: ServerInfo,
    phase: Phase,
    lock: Option<Flock>,
    listener: Option<Listener>,
    bound: Option<FileId>,
    pending: Vec<Pending>,
    conns: BTreeMap<ConnId, Conn>,
    severed: Vec<ConnId>,
    next: u64,
    refused_peers: u64,
    diagnostics: Diagnostics,
    compiled: Option<mpsc::Receiver<Compiled>>,
}

impl Host {
    /// A started daemon (it has called `setsid`, if it is a process of its own).
    #[must_use]
    pub fn new(config: HostConfig) -> Self {
        let (done, compiled) = mpsc::channel(COMPILED_QUEUE);
        Self {
            diagnostics: Diagnostics::new(Box::new(PoolRunner::new(done))),
            compiled: Some(compiled),
            config,
            server: ServerInfo {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                pid: std::process::id(),
            },
            phase: Phase::Started,
            lock: None,
            listener: None,
            bound: None,
            pending: Vec::new(),
            conns: BTreeMap::new(),
            severed: Vec::new(),
            next: 0,
            refused_peers: 0,
        }
    }

    /// The phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> &HostConfig {
        &self.config
    }

    /// The locked file, from the lock's descriptor.
    #[must_use]
    pub fn lock_id(&self) -> Option<FileId> {
        self.lock.as_ref().map(Flock::id)
    }

    /// The socket file this host bound and has not unlinked.
    #[must_use]
    pub const fn bound(&self) -> Option<FileId> {
        self.bound
    }

    /// Whether the listener is open.
    #[must_use]
    pub const fn listening(&self) -> bool {
        self.listener.is_some()
    }

    /// The accepted connections, open or awaiting their reap.
    #[must_use]
    pub const fn connections(&self) -> &BTreeMap<ConnId, Conn> {
        &self.conns
    }

    /// Peers refused for their uid.
    #[must_use]
    pub const fn refused_peers(&self) -> u64 {
        self.refused_peers
    }

    /// No connection is accepted: what the idle exit waits for (the backlog is not seen).
    #[must_use]
    pub fn idle(&self) -> bool {
        self.conns.is_empty()
    }

    fn expect(&self, step: &'static str, phases: &[Phase]) -> Result<(), HostError> {
        if phases.contains(&self.phase) {
            Ok(())
        } else {
            Err(HostError::Phase {
                step,
                phase: self.phase,
            })
        }
    }

    fn report(&self, notice: &Notice) {
        if let Some(path) = &self.config.notify {
            // The proxy may have given up (its wait expired): nothing to tell then.
            let _ignored = notify::send(path, notice);
        }
    }

    /// Takes the owner's lock. Held by another daemon: reports it on the notify socket and fails
    /// (the daemon exits 1). The path must still name the locked file afterwards.
    pub fn lock(&mut self) -> Result<(), HostError> {
        self.expect("lock", &[Phase::Started])?;
        let taken = Flock::try_acquire(&self.config.state.lock_path()).and_then(|lock| match lock
            .still_named()
        {
            Ok(true) => Ok(lock),
            Ok(false) => Err(LockError::Held {
                path: lock.path().to_owned(),
            }),
            Err(source) => Err(LockError::Io {
                path: lock.path().to_owned(),
                source,
            }),
        });
        match taken {
            Ok(lock) => {
                self.lock = Some(lock);
                self.phase = Phase::Locked;
                Ok(())
            }
            Err(error) => {
                self.phase = Phase::Failed;
                self.report(&Notice::Failed {
                    reason: error.to_string(),
                });
                Err(error.into())
            }
        }
    }

    /// As the owner, removes a socket file left at the path: stale, since only the owner binds
    /// and an owner removes its socket before it releases the lock.
    pub fn unlink_stale(&mut self) -> Result<(), HostError> {
        self.expect("unlinkStale", &[Phase::Locked])?;
        remove(&self.config.socket)
    }

    /// Binds the socket, sets it `0600`, listens and reports ready. A refused bind is reported
    /// and fails the daemon. Runs inside a Tokio runtime with IO.
    pub fn listen(&mut self) -> Result<(), HostError> {
        self.expect("listen", &[Phase::Locked])?;
        match Listener::bind(&self.config.socket, self.config.uid) {
            Ok(listener) => {
                self.bound = Some(listener.id());
                self.listener = Some(listener);
                self.phase = Phase::Serving;
                self.report(&Notice::Ready {
                    pid: self.server.pid,
                });
                Ok(())
            }
            Err(error) => {
                self.phase = Phase::Failed;
                self.lock = None;
                self.report(&Notice::Failed {
                    reason: format!("cannot bind {}: {error}", self.config.socket),
                });
                Err(error.into())
            }
        }
    }

    /// Accepts the queued connection whose handshake request has `hello` as its id, taking
    /// connections from the kernel's backlog in order until it is there, and answers its
    /// handshake (the driver's accept, which names one connection of the backlog). A connection
    /// that closes before its handshake (a proxy's second check) is dropped.
    pub async fn accept(&mut self, hello: &RequestId) -> Result<ConnId, HostError> {
        self.expect("accept", &[Phase::Serving])?;
        loop {
            if let Some(at) = self.pending.iter().position(|p| &p.hello.id == hello) {
                let pending = self.pending.remove(at);
                return self.open(pending).await;
            }
            let listener = self.listener.as_ref().ok_or(HostError::Phase {
                step: "accept",
                phase: self.phase,
            })?;
            match listener.accept().await? {
                Accepted::Refused { .. } => {
                    self.refused_peers = self.refused_peers.saturating_add(1);
                }
                Accepted::Peer(mut reader, writer) => {
                    if let Incoming::Call(call) = reader.next().await
                        && call.method == Handshake::NAME
                    {
                        self.pending.push(Pending {
                            hello: call,
                            reader,
                            writer,
                        });
                    }
                }
            }
        }
    }

    /// Answers a connection's handshake. A peer of another major gets `VersionMismatch` and its
    /// connection stays open for `jfl/stop` alone, which is frozen across majors: a newer proxy
    /// stops this daemon that way and starts its own (ADR 0021, plan §6.6). Returns the
    /// connection; handshake params that do not decode are answered with an error and dropped.
    pub(crate) async fn open_with(
        &mut self,
        hello: Call,
        reader: Option<Box<ConnReader>>,
        mut out: Outbox,
    ) -> Result<ConnId, HostError> {
        let params = codec::payload_from_value::<HandshakeParams>(hello.params.clone());
        let theirs = match params {
            Ok(params) => params.protocol,
            Err(jfl_protocol::DecodeError::MajorMismatch(mismatch)) => mismatch.theirs,
            Err(e) => {
                let body = error_body(
                    Some(hello.id.clone()),
                    ErrorCode::InvalidParams,
                    &e.to_string(),
                );
                let _sent = out.send(body).await;
                return Err(HostError::Incompatible);
            }
        };
        let mismatched = PROTOCOL_VERSION.check(theirs).is_err();
        let body = if mismatched {
            let response: Response<()> = Response {
                id: Some(hello.id.clone()),
                outcome: Outcome::Error(version_mismatch(PROTOCOL_VERSION, theirs)),
            };
            codec::encode(&response).unwrap_or_default()
        } else {
            result_body(
                hello.id.clone(),
                &HandshakeResult {
                    protocol: PROTOCOL_VERSION,
                    server: self.server.clone(),
                },
            )
        };
        if !out.send(body).await {
            return Err(HostError::Io(io::ErrorKind::BrokenPipe.into()));
        }
        let id = ConnId(self.next);
        self.next = self.next.saturating_add(1);
        self.conns.insert(
            id,
            Conn {
                reader,
                out,
                mismatched,
                peer_closed: false,
                overlays: BTreeMap::new(),
                requests: VecDeque::new(),
                applied: 0,
            },
        );
        Ok(id)
    }

    async fn open(&mut self, pending: Pending) -> Result<ConnId, HostError> {
        self.open_with(
            pending.hello,
            Some(pending.reader),
            Outbox::Direct(pending.writer),
        )
        .await
    }

    /// Sends `body` on `conn`; a connection whose queue is full is severed.
    async fn deliver(&mut self, conn: ConnId, body: Vec<u8>) {
        if let Some(state) = self.conns.get_mut(&conn)
            && !state.out.send(body).await
        {
            self.sever(conn);
        }
    }

    /// Drops `conn` at once, overlays and requests with it (a peer that does not read its
    /// replies); the run loop closes its socket by ending its tasks ([`Host::take_severed`]).
    pub(crate) fn sever(&mut self, conn: ConnId) {
        self.diagnostics.gone(conn);
        if self.conns.remove(&conn).is_some() {
            self.severed.push(conn);
        }
    }

    /// The connections severed since the last call.
    pub(crate) fn take_severed(&mut self) -> Vec<ConnId> {
        std::mem::take(&mut self.severed)
    }

    /// Reads one frame of `conn` and applies it.
    pub async fn receive(&mut self, conn: ConnId) -> Result<Received, HostError> {
        let reader = self
            .conns
            .get_mut(&conn)
            .and_then(|c| c.reader.as_mut())
            .ok_or(HostError::NoConnection(conn))?;
        let incoming = reader.next().await;
        self.apply(conn, incoming).await
    }

    /// Applies one frame of `conn`: an overlay's change, a queued request, a cancellation, the
    /// EOF (the connection then awaits its reap, its overlays with it), or an error answered. On
    /// a mismatched connection only `jfl/stop` is queued; any other request is refused again.
    pub async fn apply(&mut self, conn: ConnId, incoming: Incoming) -> Result<Received, HostError> {
        let state = self
            .conns
            .get_mut(&conn)
            .ok_or(HostError::NoConnection(conn))?;
        let mut noted = Noted::Nothing;
        let (received, reply) = match incoming {
            Incoming::Call(call) if state.mismatched && call.method != Stop::NAME => {
                let response: Response<()> = Response {
                    id: Some(call.id),
                    outcome: Outcome::Error(ErrorObject {
                        code: ErrorCode::VersionMismatch,
                        message: "another protocol major: only jfl/stop is served".to_owned(),
                        data: None,
                    }),
                };
                (Received::Refused, codec::encode(&response).ok())
            }
            Incoming::Call(call) => {
                state.requests.push_back(call);
                (Received::Request, None)
            }
            Incoming::Note(_) if state.mismatched => (Received::Applied, None),
            Incoming::Note(note) => {
                if matches!(
                    note.method.as_str(),
                    DidOpen::NAME | DidChange::NAME | DidClose::NAME
                ) {
                    state.applied = state.applied.saturating_add(1);
                }
                noted = note_into(state, note);
                let reply = match &noted {
                    Noted::Cancelled(cancelled) => Some(error_body(
                        Some(cancelled.clone()),
                        ErrorCode::RequestCancelled,
                        "cancelled by $/cancelRequest",
                    )),
                    _ => None,
                };
                (Received::Applied, reply)
            }
            Incoming::Malformed(malformed) => (
                Received::Refused,
                Some(error_body(
                    malformed.id,
                    malformed.code,
                    "not a JSON-RPC message",
                )),
            ),
            Incoming::Oversized(oversized) => (
                Received::Refused,
                Some(error_body(
                    None,
                    ErrorCode::InvalidRequest,
                    &format!(
                        "a frame of {} bytes is over the limit of {} bytes",
                        oversized.declared, oversized.limit
                    ),
                )),
            ),
            Incoming::End(_) => {
                state.peer_closed = true;
                state.requests.clear();
                self.diagnostics.gone(conn);
                (Received::Closed, None)
            }
        };
        if let Some(body) = reply {
            self.deliver(conn, body).await;
        }
        for (to, body) in self.noted(conn, noted) {
            self.deliver(to, body).await;
        }
        Ok(received)
    }

    /// Hands a document notification or a watch to the diagnostics; what to publish now.
    fn noted(&mut self, conn: ConnId, noted: Noted) -> Vec<Outgoing> {
        let Some(state) = self.conns.get(&conn) else {
            return Vec::new();
        };
        let diagnostics = &mut self.diagnostics;
        match noted {
            Noted::Watch(encoding) => diagnostics.watch(
                conn,
                encoding,
                state.applied,
                state
                    .overlays
                    .iter()
                    .map(|(uri, o)| (uri, o.version, o.text.as_str())),
            ),
            Noted::Document(uri, kind) => {
                let overlay = state.overlays.get(&uri);
                let change = match (kind, overlay) {
                    (DocNote::Open, Some(o)) => Change::Open {
                        version: o.version,
                        text: &o.text,
                    },
                    (DocNote::Change, Some(o)) => Change::Edit {
                        version: o.version,
                        text: &o.text,
                    },
                    (DocNote::Close, _) => Change::Close,
                    (DocNote::Open | DocNote::Change, None) => return Vec::new(),
                };
                diagnostics.document(conn, state.applied, &uri, change)
            }
            Noted::Cancelled(_) | Noted::Nothing => Vec::new(),
        }
    }

    /// The channel the run loop reads finished compiles from (once).
    pub(crate) fn take_compiled(&mut self) -> Option<mpsc::Receiver<Compiled>> {
        self.compiled.take()
    }

    /// A compile came back: its publication, if current, is queued on its connection.
    pub(crate) async fn diagnosed(&mut self, compiled: Compiled) {
        for (to, body) in self.diagnostics.compiled(compiled) {
            self.deliver(to, body).await;
        }
    }

    /// The disk of every watched open document, seen again (the run loop's tick).
    pub(crate) fn observe_disk(&mut self) {
        self.diagnostics.observe_disk();
    }

    /// Answers the oldest request of `conn` through `service`. Returns whether it was
    /// `jfl/stop`, after which the daemon shuts down.
    pub async fn answer(
        &mut self,
        conn: ConnId,
        service: &mut dyn Service,
    ) -> Result<bool, HostError> {
        self.expect("answer", &[Phase::Serving])?;
        let state = self
            .conns
            .get_mut(&conn)
            .ok_or(HostError::NoConnection(conn))?;
        let call = state
            .requests
            .pop_front()
            .ok_or(HostError::NoRequest(conn))?;
        let stop = call.method == Stop::NAME;
        let body = if stop {
            result_body(call.id.clone(), &jfl_protocol::messages::StopResult {})
        } else {
            let context = Context {
                server: &self.server,
                overlays: &state.overlays,
            };
            service.answer(&call, &context).await
        };
        self.deliver(conn, body).await;
        Ok(stop)
    }

    /// Drops a connection whose EOF was read, with its overlays.
    pub fn reap(&mut self, conn: ConnId) -> Result<(), HostError> {
        self.expect("reap", &[Phase::Serving])?;
        match self.conns.get(&conn) {
            None => Err(HostError::NoConnection(conn)),
            Some(state) if !state.peer_closed => Err(HostError::StillOpen(conn)),
            Some(_) => {
                self.conns.remove(&conn);
                self.diagnostics.gone(conn);
                Ok(())
            }
        }
    }

    /// The idle exit's first step: only with no connection accepted.
    pub fn idle_exit(&mut self) -> Result<(), HostError> {
        self.expect("idleExit", &[Phase::Serving])?;
        if !self.idle() {
            return Err(HostError::NotIdle);
        }
        self.stop()
    }

    /// The shutdown's first step (SIGTERM, the idle exit, a lost lock or socket, `jfl/stop`):
    /// closes the listener (a connection still in the backlog is reset), and every connection
    /// with its overlays and requests.
    pub fn stop(&mut self) -> Result<(), HostError> {
        self.expect("stop", &[Phase::Serving])?;
        self.listener = None;
        self.pending.clear();
        self.conns.clear();
        self.diagnostics.clear();
        self.severed.clear();
        self.phase = Phase::Stopping;
        Ok(())
    }

    /// The shutdown removes the socket file, still holding the lock: the path, when it still
    /// names the socket this host bound. Nothing there is fine (it was swept), and another file
    /// is left alone: with the lock held nobody else binds, so this is the model's `unlink`, but
    /// after a lost lock (its path deleted from outside) a new owner may have bound already.
    /// Pinned by `tests/daemon.rs::should_stop_and_leave_the_new_owners_socket_when_the_lock_file_was_deleted`.
    pub fn unlink(&mut self) -> Result<(), HostError> {
        self.expect("unlink", &[Phase::Stopping])?;
        if self.bound.is_some() && FileId::at(self.config.socket.path())? == self.bound {
            remove(&self.config.socket)?;
        }
        self.bound = None;
        self.phase = Phase::Unlinked;
        Ok(())
    }

    /// Releases the lock (the file stays): the daemon exits 0. Allowed from `Stopping` too, so
    /// the negative control can release before the unlink; the run loop never does
    /// ([`Host::shutdown`]).
    pub fn release(&mut self) -> Result<(), HostError> {
        self.expect("release", &[Phase::Unlinked, Phase::Stopping])?;
        self.lock = None;
        self.phase = Phase::Exited;
        Ok(())
    }

    /// The whole shutdown, in its order: stop accepting, unlink, release (never the release
    /// first: the next owner could bind before this unlink removed its socket).
    pub fn shutdown(&mut self) -> Result<(), HostError> {
        if self.phase == Phase::Serving {
            self.stop()?;
        }
        self.unlink()?;
        self.release()
    }

    /// An abort: every descriptor closed, none of the shutdown run. The socket file stays.
    pub fn crash(&mut self) {
        self.lock = None;
        self.listener = None;
        self.bound = None;
        self.pending.clear();
        self.conns.clear();
        self.diagnostics.clear();
        self.phase = Phase::Crashed;
    }

    /// The periodic check (after locking and on every idle tick, obligation 1, and the swept
    /// socket): whether the lock path still names the locked file and the socket path the bound
    /// socket.
    pub fn tick(&self) -> Result<Tick, HostError> {
        if let Some(lock) = &self.lock
            && !lock.still_named()?
        {
            return Ok(Tick::LockLost);
        }
        if let Some(bound) = self.bound
            && self.phase == Phase::Serving
            && FileId::at(self.config.socket.path())? != Some(bound)
        {
            return Ok(Tick::SocketLost);
        }
        Ok(Tick::Fine)
    }

    /// Takes the listener out for the run loop, which accepts outside the host.
    pub(crate) fn take_listener(&mut self) -> Option<Listener> {
        self.listener.take()
    }

    /// The oldest unanswered request of `conn`, if any.
    pub(crate) fn has_request(&self, conn: ConnId) -> bool {
        self.conns.get(&conn).is_some_and(Conn::inflight)
    }

    /// Whether `conn`'s peer closed it.
    pub(crate) fn is_peer_closed(&self, conn: ConnId) -> bool {
        self.conns.get(&conn).is_some_and(|c| c.peer_closed)
    }

    /// Counts a peer refused for its uid.
    pub(crate) fn count_refused(&mut self) {
        self.refused_peers = self.refused_peers.saturating_add(1);
    }
}

/// Removes the socket file at `path`; nothing there is fine.
fn remove(path: &SocketPath) -> Result<(), HostError> {
    match fs::remove_file(path.path()) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The modification time of `uri`'s file now: one stat, when an overlay's text arrives.
fn disk_time(uri: &jfl_protocol::DocumentUri) -> Option<std::time::SystemTime> {
    fs::metadata(uri.file().path())
        .and_then(|m| m.modified())
        .ok()
}

/// Which document notification was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocNote {
    Open,
    Change,
    Close,
}

/// What a notification did, beyond the connection's overlays.
#[derive(Debug)]
enum Noted {
    Nothing,
    /// A request it cancelled.
    Cancelled(RequestId),
    /// A document notification, applied.
    Document(DocumentUri, DocNote),
    /// `jfl/watchDiagnostics`.
    Watch(jfl_domain::PositionEncoding),
}

/// Applies a notification to a connection's state.
fn note_into(state: &mut Conn, note: Note) -> Noted {
    match note.method.as_str() {
        DidOpen::NAME => {
            if let Ok(params) = codec::payload_from_value::<DidOpenParams>(note.params) {
                let disk = disk_time(&params.uri);
                state.overlays.insert(
                    params.uri.clone(),
                    Overlay {
                        version: params.version,
                        text: params.text,
                        disk,
                    },
                );
                return Noted::Document(params.uri, DocNote::Open);
            }
            Noted::Nothing
        }
        DidChange::NAME => {
            if let Ok(params) = codec::payload_from_value::<DidChangeParams>(note.params)
                && let Some(overlay) = state.overlays.get_mut(&params.uri)
            {
                overlay.version = params.version;
                // Full-text changes only: the LSP proxy applies a client's ranged edits to its own
                // copy and sends the whole text (LSP-02).
                if let Some(full) = params.changes.into_iter().rev().find(|c| c.range.is_none()) {
                    overlay.text = full.text;
                    overlay.disk = disk_time(&params.uri);
                }
                return Noted::Document(params.uri, DocNote::Change);
            }
            Noted::Nothing
        }
        DidClose::NAME => {
            if let Ok(params) = codec::payload_from_value::<DidCloseParams>(note.params) {
                state.overlays.remove(&params.uri);
                return Noted::Document(params.uri, DocNote::Close);
            }
            Noted::Nothing
        }
        Cancel::NAME => {
            let Ok(params) = codec::payload_from_value::<CancelParams>(note.params) else {
                return Noted::Nothing;
            };
            let at = state.requests.iter().position(|c| c.id == params.id);
            at.and_then(|at| state.requests.remove(at))
                .map_or(Noted::Nothing, |call| Noted::Cancelled(call.id))
        }
        WatchDiagnostics::NAME => codec::payload_from_value::<WatchDiagnosticsParams>(note.params)
            .map_or(Noted::Nothing, |p| Noted::Watch(p.position_encoding)),
        _ => Noted::Nothing,
    }
}
