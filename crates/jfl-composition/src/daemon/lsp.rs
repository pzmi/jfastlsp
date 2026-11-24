//! `jfl lsp` (plan §2.5, ADR 0034): the stdio language server, a thin proxy over the daemon
//! (ADR 0016). The LSP adapter runs the session ([`jfl_adapter_lsp::serve`]: framing, the main
//! loop, the pool, cancellation bookkeeping); this module is its [`Handler`] and the link to the
//! daemon.
//!
//! - **Lifecycle.** `initialize` is answered on the main loop at once, never waiting on the daemon
//!   or its indexing (plan §2.5): the daemon is started by the first request that needs it. Before
//!   `initialize` every request is answered `ServerNotInitialized` (-32002) and a notification is
//!   dropped (LSP 3.17); after it an unknown request is `MethodNotFound`. `shutdown` answers
//!   `null`, after which every request is `InvalidRequest`; `exit` ends the session, and the
//!   process exits 0 when `shutdown` came first and 1 otherwise; an input that ends (or whose
//!   framing is lost) without `exit` is the same ([`exit_code`]). Pinned by the lifecycle
//!   transcripts of `fixtures/lsp/`.
//! - **Encodings.** `utf-8` when the client offers it, `utf-16` otherwise
//!   ([`translate::negotiate`]); every position the client sends and every range answered is in
//!   it, converted only by `jfl-domain`.
//! - **Document sync** (plan §4.4). The session keeps each open document's text, applies
//!   `didChange`'s ranged edits to it ([`Document::apply`]) and sends the daemon the whole new
//!   text with the client's version, so the daemon's overlays never apply an edit themselves. A
//!   change that does not fit the text leaves the document *parted*: the daemon's overlay is
//!   dropped and every request on it is answered `ContentModified` until the client opens it
//!   again. A column past its line's end is clamped to the end for an open document (LSP 3.17);
//!   for any other file the daemon refuses it.
//! - **The link.** One thread owns the daemon proxy ([`Proxy`]) and takes notifications and
//!   requests from one bounded queue ([`LINK_QUEUE`]) in the order the main loop read them, so a
//!   request is asked after every change the client sent before it. A request is queued on the
//!   main loop and its job on the pool waits for the link's answer; the link asks the daemon one
//!   request at a time (the daemon serves them one at a time anyway). A cancel for a request still
//!   waiting for the link is answered `RequestCancelled` at once and the request is never asked
//!   (the main loop sets its flag and wakes its job, [`Handler::cancelled`]); one cancelled while
//!   the daemon works on it is answered `RequestCancelled` when the daemon is done; a cancel after
//!   the answer changes nothing. Pinned by `tests/lsp.rs` (each order forced) and
//!   `fixtures/lsp/cancel-after-completion.jsonl`. A daemon that dies is reconnected to, or started again, and
//!   the open documents are sent again before the request (the proxy's reconnect, plan §5.2;
//!   pinned by `crates/jfl-cli/tests/lsp_daemon.rs`).
//! - **Deadlines** (plan §2.7). The daemon's query answers within its internal deadline (300 ms
//!   for a definition), partly and saying so. Each request has one deadline, [`CALL_BUDGET`]
//!   from its dispatch on the main loop, below the plugin's `requestTimeout`: the link asks the
//!   daemon with what is left of it (every wait of the proxy's call, a daemon start included, is
//!   cut there), and a request whose deadline passed while it waited behind others is not asked
//!   at all. Either way it is answered `ServerCancelled` (-32802, LSP 3.17: the client may
//!   retry). The link takes one request at a time and each ends by its own deadline, so a request
//!   queued behind others is answered by its deadline too (theirs come no later). Pinned by
//!   `lsp::tests::should_answer_server_cancelled_without_asking_when_the_deadline_passed_in_the_queue`
//!   and `::should_answer_server_cancelled_when_the_call_ended_on_a_held_spawn_lock`.
//!
//! - **Pushed diagnostics** (LSP-04, plan §2.5, §3.2). A client whose `initialize` declares
//!   `textDocument.publishDiagnostics` ([`translate::wants_diagnostics`]) gets its open documents'
//!   syntax diagnostics as `textDocument/publishDiagnostics`, with the version they were computed
//!   for (none for the disk's content) and ranges in the negotiated encoding, counted by the daemon
//!   on the very text it compiled. The link asks the daemon for them on every connection
//!   (`jfl/watchDiagnostics`) and connects, starting the daemon, at the first `didOpen` rather than
//!   the first request. Between items it waits on the daemon's connection and on its queue at
//!   once ([`jfl_adapter_ipc::Bell`]: the session rings it with every item it queues), and hands on
//!   each publication the proxy did not find stale through the session's [`Notifier`]. It clears a
//!   closed document itself, whether or not the close reached a daemon. Pinned by the transcripts
//!   `fixtures/lsp/diagnostics-*.jsonl` and
//!   `tests/pushed_diagnostics.rs::should_publish_the_empty_set_when_a_document_closes_and_no_daemon_can_be_reached`.
//!
//! `textDocument/definition` is the one navigation method served here; its LSP result is the
//! `exact` locations, or a proved owner's overload set, or nothing ([`translate::definition`]).
//! Hover, references and the others are LSP-03's, and no capability is announced for them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use jfl_adapter_ipc::{Bell, LockError, Ringer, SocketPath, StateDir};
use jfl_adapter_lsp::document::Document;
use jfl_adapter_lsp::translate::{self, Echo, UriBook};
use jfl_adapter_lsp::transport::End;
use jfl_adapter_lsp::{
    Config, Dispatch, Ended, Handler, Job, Notification, Notifier, Reply, Request, RequestId, serve,
};
use jfl_domain::{FileUri, PositionEncoding};
use jfl_protocol::messages::{ClientKind, Definition, PositionParams};
use jfl_protocol::rpc::{ErrorCode, Outcome};
use jfl_protocol::{DocumentUri, WireEnvelope};

use super::ProcessLauncher;
use super::proxy::{Budget, Proxy, ProxyError};

/// Items waiting for the link: a client that sends faster than the daemon answers is held back
/// (the main loop waits to queue), never buffered without bound.
pub const LINK_QUEUE: usize = 64;

/// The longest one request waits for the daemon, a daemon start included: under Claude Code's
/// `requestTimeout` of 10 s (plan §2.6), so the client sees this error rather than its own
/// timeout.
pub const CALL_BUDGET: Duration = Duration::from_secs(8);

/// LSP 3.17's `ServerCancelled`: the server gave up on the request (here: its deadline passed),
/// and the client may send it again.
pub const SERVER_CANCELLED: ErrorCode = ErrorCode::Other(-32802);

/// The requests served, by method; any other is `MethodNotFound` (after `initialize`).
const METHODS: [&str; 3] = ["initialize", "shutdown", "textDocument/definition"];

/// Why the daemon gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The error code the client gets.
    pub code: ErrorCode,
    /// The message it gets.
    pub message: String,
}

impl Failure {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The daemon as the link drives it: the proxy ([`ProxyDaemon`]), or a test's stand-in.
pub trait Daemon {
    /// The client opened `uri` (or reopened it) with `text`.
    fn open(&mut self, uri: DocumentUri, version: i32, text: String);
    /// The client's document `uri` is now `text`.
    fn change(&mut self, uri: DocumentUri, version: i32, text: String);
    /// The client closed `uri`, or it parted from the client's text.
    fn close(&mut self, uri: &DocumentUri);
    /// `jfl/definition`, answered within `budget`.
    fn definition(
        &mut self,
        params: PositionParams,
        budget: Duration,
    ) -> Result<WireEnvelope, Failure>;

    /// The client takes pushed diagnostics, ranges in `encoding`, sent through `notifier`. None
    /// by default.
    fn watch(&mut self, encoding: PositionEncoding, notifier: Notifier) {
        let _ = (encoding, notifier);
    }

    /// Hands on the diagnostics the daemon published since the last call, without waiting. None
    /// by default.
    fn publish(&mut self) {}

    /// Waits until the session queues an item or the daemon sends something; `false` when this
    /// daemon cannot wait so (the link then waits on its queue alone). `false` by default.
    fn idle(&mut self) -> bool {
        false
    }
}

/// What the main loop hands the link, in the order the client sent it.
pub enum Item {
    /// A document opened.
    Open(DocumentUri, i32, String),
    /// A document's whole new text.
    Change(DocumentUri, i32, String),
    /// A document closed.
    Close(DocumentUri),
    /// The client takes pushed diagnostics.
    Watch(PositionEncoding, Notifier),
    /// A definition: its params, its deadline, its cancel flag, and where the answer goes.
    Definition {
        /// The daemon's params.
        params: PositionParams,
        /// When the request must be answered by: [`CALL_BUDGET`] after its dispatch.
        deadline: Instant,
        /// Set by the session's main loop when `$/cancelRequest` names the request.
        cancel: Arc<AtomicBool>,
        /// The answer's way back to the job.
        reply: SyncSender<Result<WireEnvelope, Failure>>,
    },
}

/// The link's loop: every item in order, until the session drops its end of the queue; between
/// items, the daemon's publications handed on ([`Daemon::publish`]), waiting on the queue and the
/// daemon at once where the daemon can ([`Daemon::idle`]).
pub fn link<D: Daemon>(items: &Receiver<Item>, daemon: &mut D) {
    loop {
        loop {
            match items.try_recv() {
                Ok(item) => apply(item, daemon),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        daemon.publish();
        if !daemon.idle() {
            match items.recv() {
                Ok(item) => apply(item, daemon),
                Err(_) => return,
            }
        }
    }
}

fn apply<D: Daemon>(item: Item, daemon: &mut D) {
    match item {
        Item::Open(uri, version, text) => daemon.open(uri, version, text),
        Item::Change(uri, version, text) => daemon.change(uri, version, text),
        Item::Close(uri) => daemon.close(&uri),
        Item::Watch(encoding, notifier) => daemon.watch(encoding, notifier),
        Item::Definition {
            params,
            deadline,
            cancel,
            reply,
        } => {
            let left = deadline.saturating_duration_since(Instant::now());
            let answer = if cancel.load(Ordering::Acquire) {
                Err(Failure::new(
                    ErrorCode::RequestCancelled,
                    "the request was cancelled",
                ))
            } else if left.is_zero() {
                Err(Failure::new(
                    SERVER_CANCELLED,
                    "the request's deadline passed while it waited for the daemon: ask again",
                ))
            } else {
                daemon.definition(params, left)
            };
            // A job that is gone was cancelled before it ran: nobody waits.
            let _ = reply.send(answer);
        }
    }
}

/// The session's phase, shared with [`exit_code`] once the session is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    /// Before `initialize`.
    Starting = 0,
    /// Initialized.
    Running = 1,
    /// After `shutdown`.
    ShutDown = 2,
    /// `exit` after `shutdown`.
    ExitedClean = 3,
    /// `exit` without `shutdown`.
    ExitedEarly = 4,
}

impl Phase {
    fn of(value: u8) -> Self {
        match value {
            1 => Self::Running,
            2 => Self::ShutDown,
            3 => Self::ExitedClean,
            4 => Self::ExitedEarly,
            _ => Self::Starting,
        }
    }
}

/// The process's exit code once its session `ended` in `phase`: 0 only when `shutdown` came first
/// (then `exit`, or the input's end between frames), 1 otherwise (plan §2.5).
///
/// Pinned by: `lsp::tests::should_exit_zero_only_after_shutdown_when_the_session_ends` and the
/// lifecycle transcripts.
#[must_use]
pub fn exit_code(phase: Phase, ended: &Ended) -> u8 {
    let clean_end = matches!(ended.input, End::Exit | End::Eof) && ended.output.is_none();
    match phase {
        Phase::ExitedClean | Phase::ShutDown if clean_end => 0,
        _ => 1,
    }
}

/// The session's state: the [`Handler`] the adapter drives.
pub struct Session {
    phase: Arc<AtomicU8>,
    encoding: PositionEncoding,
    version: String,
    docs: HashMap<FileUri, Document>,
    parted: HashSet<FileUri>,
    book: Arc<UriBook>,
    link: SyncSender<Item>,
    /// The cancel flag and the answer's way back of each request queued for the link, by id
    /// (pruned once the link and the job let theirs go).
    flags: HashMap<RequestId, Waiting>,
    /// Rung after every item queued, for a link that waits on the daemon too.
    ringer: Option<Ringer>,
    /// The session's own notifications, once the adapter hands it over.
    notifier: Option<Notifier>,
}

/// A request queued for the link, as the main loop can still reach it.
type Waiting = (Arc<AtomicBool>, SyncSender<Result<WireEnvelope, Failure>>);

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("phase", &self.phase())
            .field("encoding", &self.encoding)
            .field("docs", &self.docs.len())
            .finish_non_exhaustive()
    }
}

impl Session {
    /// A session before `initialize`, handing the daemon's work to `link`, reporting its phase in
    /// `phase` (for [`exit_code`]) and naming itself `version` to the client.
    #[must_use]
    pub fn new(link: SyncSender<Item>, phase: Arc<AtomicU8>, version: &str) -> Self {
        phase.store(Phase::Starting as u8, Ordering::Release);
        Self {
            phase,
            encoding: PositionEncoding::Utf16,
            version: version.to_owned(),
            docs: HashMap::new(),
            parted: HashSet::new(),
            book: Arc::new(UriBook::default()),
            link,
            flags: HashMap::new(),
            ringer: None,
            notifier: None,
        }
    }

    /// The same session ringing `ringer` after every item it queues for the link.
    #[must_use]
    pub fn with_ringer(mut self, ringer: Ringer) -> Self {
        self.ringer = Some(ringer);
        self
    }

    fn phase(&self) -> Phase {
        Phase::of(self.phase.load(Ordering::Acquire))
    }

    fn set(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    fn send(&self, item: Item) -> bool {
        let sent = self.link.send(item).is_ok();
        if let Some(ringer) = &self.ringer {
            ringer.ring();
        }
        sent
    }

    fn definition(&mut self, id: RequestId, params: serde_json::Value) -> Dispatch<UriBook> {
        let now = |code, message: &str| Dispatch::Now(Reply::error(code, message));
        let Some((uri, position)) = translate::text_document_position(params) else {
            return now(ErrorCode::InvalidParams, "not a text document position");
        };
        let file = uri.file().clone();
        if self.parted.contains(&file) {
            return now(
                ErrorCode::ContentModified,
                "the document's changes did not fit its text: open it again",
            );
        }
        let at = match self.docs.get(&file) {
            Some(doc) => match doc.clamp(position, self.encoding) {
                Ok(at) => at,
                Err(e) => return now(ErrorCode::InvalidParams, &e.to_string()),
            },
            None => jfl_domain::Position::new(position.line, position.character),
        };
        let params = PositionParams {
            uri: uri.clone(),
            line: at.line(),
            col: at.column(),
            position_encoding: self.encoding,
            clamp: true,
        };
        let (reply, answer) = sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.flags
            .retain(|_, (flag, _)| Arc::strong_count(flag) > 1);
        self.flags.insert(id, (Arc::clone(&cancel), reply.clone()));
        let item = Item::Definition {
            params,
            deadline: Instant::now()
                .checked_add(CALL_BUDGET)
                .unwrap_or_else(Instant::now),
            cancel: Arc::clone(&cancel),
            reply,
        };
        if !self.send(item) {
            return now(ErrorCode::InternalError, "the link to the daemon is gone");
        }
        Dispatch::Pool(Job::new(move |book: &UriBook, _| {
            let _held = cancel;
            match answer.recv() {
                Ok(Ok(envelope)) => {
                    let echo = Echo {
                        named: Some(&uri),
                        opened: Some(book),
                    };
                    Reply::result(&translate::definition(&envelope, &echo))
                }
                // No name at the position: LSP's "no definition".
                Ok(Err(failure)) if failure.code == ErrorCode::NoSymbol => {
                    Reply::result(&serde_json::Value::Null)
                }
                Ok(Err(failure)) => Reply::error(failure.code, failure.message),
                Err(_) => Reply::error(ErrorCode::InternalError, "the link to the daemon is gone"),
            }
        }))
    }

    fn opened(&mut self, params: serde_json::Value) {
        let Some((uri, version, text)) = translate::did_open(params) else {
            return;
        };
        let file = uri.file().clone();
        Arc::make_mut(&mut self.book).open(&uri);
        match Document::open(uri.clone(), version, text.clone()) {
            Ok(doc) => {
                self.docs.insert(file.clone(), doc);
                self.parted.remove(&file);
                self.send(Item::Open(uri, version, text));
            }
            Err(_) => {
                self.docs.remove(&file);
                self.parted.insert(file);
                self.send(Item::Close(uri));
            }
        }
    }

    fn changed(&mut self, params: serde_json::Value) {
        let Some((uri, version, changes)) = translate::did_change(params) else {
            return;
        };
        let file = uri.file().clone();
        let Some(doc) = self.docs.get_mut(&file) else {
            return;
        };
        match doc.apply(version, changes, self.encoding) {
            Ok(()) => {
                let item = Item::Change(doc.uri.clone(), version, doc.text().to_owned());
                self.send(item);
            }
            Err(_) => {
                let uri = doc.uri.clone();
                self.docs.remove(&file);
                self.parted.insert(file);
                self.send(Item::Close(uri));
            }
        }
    }

    fn closed(&mut self, params: serde_json::Value) {
        let Some(uri) = translate::did_close(params) else {
            return;
        };
        let file = uri.file().clone();
        Arc::make_mut(&mut self.book).close(&file);
        self.parted.remove(&file);
        let sent = self.docs.remove(&file).map_or(uri, |doc| doc.uri);
        self.send(Item::Close(sent));
    }
}

impl Handler for Session {
    type Snapshot = UriBook;

    fn attach(&mut self, notifier: Notifier) {
        self.notifier = Some(notifier);
    }

    fn snapshot(&self) -> Arc<UriBook> {
        Arc::clone(&self.book)
    }

    fn request(&mut self, request: Request) -> Dispatch<UriBook> {
        let method = request.method.as_str();
        let error = |code, message: &str| Dispatch::Now(Reply::error(code, message));
        match (self.phase(), method) {
            (Phase::Starting, "initialize") => {
                self.encoding = translate::negotiate(&request.params);
                if translate::wants_diagnostics(&request.params)
                    && let Some(notifier) = self.notifier.clone()
                {
                    self.send(Item::Watch(self.encoding, notifier));
                }
                self.set(Phase::Running);
                Dispatch::Now(Reply::result(&translate::initialize_result(
                    self.encoding,
                    &self.version,
                )))
            }
            // Every request before `initialize`, known or not (LSP 3.17).
            (Phase::Starting, _) => error(
                ErrorCode::HandshakeRequired,
                "the server is not initialized: send initialize first",
            ),
            (_, _) if !METHODS.contains(&method) => Dispatch::MethodNotFound,
            (Phase::Running, "initialize") => error(
                ErrorCode::InvalidRequest,
                "the server is already initialized",
            ),
            (Phase::Running, "shutdown") => {
                self.set(Phase::ShutDown);
                Dispatch::Now(Reply::result(&serde_json::Value::Null))
            }
            (Phase::Running, _) => self.definition(request.id, request.params),
            (Phase::ShutDown | Phase::ExitedClean | Phase::ExitedEarly, _) => {
                error(ErrorCode::InvalidRequest, "the server is shut down")
            }
        }
    }

    fn notification(&mut self, notification: Notification) {
        let phase = self.phase();
        match notification.method.as_str() {
            "exit" => self.set(if phase == Phase::ShutDown {
                Phase::ExitedClean
            } else {
                Phase::ExitedEarly
            }),
            // Before `initialize` every other notification is dropped (LSP 3.17).
            _ if phase == Phase::Starting => {}
            "textDocument/didOpen" => self.opened(notification.params),
            "textDocument/didChange" => self.changed(notification.params),
            "textDocument/didClose" => self.closed(notification.params),
            _ => {}
        }
    }

    fn cancelled(&mut self, id: &RequestId) {
        if let Some((flag, reply)) = self.flags.remove(id) {
            flag.store(true, Ordering::Release);
            // Wakes a job still waiting for the link; a full channel already holds its answer.
            let _ = reply.try_send(Err(Failure::new(
                ErrorCode::RequestCancelled,
                "the request was cancelled",
            )));
        }
    }

    fn ended(&self) -> bool {
        matches!(self.phase(), Phase::ExitedClean | Phase::ExitedEarly)
    }
}

/// The daemon through a [`Proxy`], built on first use (the state directory is opened and checked
/// then, so a session that asks nothing never touches it), starting the daemon on demand.
pub struct ProxyDaemon {
    config: LspConfig,
    program: PathBuf,
    name: String,
    proxy: Option<Proxy<ProcessLauncher>>,
    docs: BTreeMap<DocumentUri, (i32, String)>,
    /// Pushed diagnostics: their encoding and the session's notifier.
    watch: Option<(PositionEncoding, Notifier)>,
    bell: Option<Bell>,
    retry: Backoff,
}

impl std::fmt::Debug for ProxyDaemon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyDaemon")
            .field("name", &self.name)
            .field("proxy", &self.proxy)
            .finish_non_exhaustive()
    }
}

impl ProxyDaemon {
    /// The daemon of `config`'s state directory, started as `program daemon …` when none runs;
    /// `name` names this client's requests (unique among the state directory's clients).
    #[must_use]
    pub fn new(config: LspConfig, program: &Path, name: &str) -> Self {
        Self {
            config,
            program: program.to_owned(),
            name: name.to_owned(),
            proxy: None,
            docs: BTreeMap::new(),
            watch: None,
            bell: None,
            retry: Backoff::default(),
        }
    }

    /// The same daemon, its link waiting on `bell` and the daemon's connection at once.
    #[must_use]
    pub fn with_bell(mut self, bell: Bell) -> Self {
        self.bell = Some(bell);
        self
    }

    /// With diagnostics watched and a document open, connects (starting the daemon) so the
    /// daemon has the documents and publishes: at the first `didOpen`, and again after the
    /// daemon went away. A daemon that cannot be reached is tried again at an item after a
    /// backoff ([`ATTACH_BACKOFF`], doubled at each failure up to [`ATTACH_BACKOFF_MAX`], reset by
    /// a success), so a daemon that cannot start does not cost every edit a whole wait. The
    /// backoff is a deadline compared at the next item, never a sleep.
    ///
    /// Pinned by: `lsp::tests::should_back_off_doubling_and_reset_when_attaching_fails_then_succeeds`.
    fn attach(&mut self) {
        if self.watch.is_none() || self.docs.is_empty() {
            return;
        }
        if self.retry.blocks(Instant::now()) {
            return;
        }
        let Ok(proxy) = self.proxy() else {
            return;
        };
        if proxy.phase() == super::proxy::ProxyPhase::Ready {
            return;
        }
        if proxy.attach(CALL_BUDGET).is_ok() {
            self.retry.succeeded();
        } else {
            proxy.abandon();
            self.retry.failed(Instant::now());
        }
    }

    fn proxy(&mut self) -> Result<&mut Proxy<ProcessLauncher>, Failure> {
        if self.proxy.is_none() {
            let unusable = |e: &dyn std::fmt::Display| {
                Failure::new(
                    ErrorCode::InternalError,
                    format!("the daemon's state directory is unusable: {e}"),
                )
            };
            let state = StateDir::open(&self.config.state_dir).map_err(|e| unusable(&e))?;
            let socket = SocketPath::of_state(&state).map_err(|e| unusable(&e))?;
            let mut launcher = ProcessLauncher::new(&self.program, state.clone());
            if let Some(idle) = self.config.idle {
                launcher = launcher.with_idle(idle);
            }
            let budget = Budget {
                reply: CALL_BUDGET,
                spawn: CALL_BUDGET,
            };
            let mut proxy =
                Proxy::new(&self.name, ClientKind::Lsp, state, socket, launcher, budget);
            if let Some((encoding, _)) = &self.watch {
                proxy = proxy.with_diagnostics(*encoding);
            }
            for (uri, (version, text)) in &self.docs {
                proxy.did_open(uri.clone(), *version, text.clone());
            }
            self.proxy = Some(proxy);
        }
        self.proxy
            .as_mut()
            .ok_or_else(|| Failure::new(ErrorCode::InternalError, "no proxy"))
    }
}

impl Daemon for ProxyDaemon {
    fn open(&mut self, uri: DocumentUri, version: i32, text: String) {
        self.docs.insert(uri.clone(), (version, text.clone()));
        if let Some(proxy) = &mut self.proxy {
            proxy.did_open(uri, version, text);
        }
        self.attach();
    }

    fn change(&mut self, uri: DocumentUri, version: i32, text: String) {
        self.docs.insert(uri.clone(), (version, text.clone()));
        if let Some(proxy) = &mut self.proxy {
            proxy.did_change(uri, version, text);
        }
    }

    fn close(&mut self, uri: &DocumentUri) {
        self.docs.remove(uri);
        if let Some(proxy) = &mut self.proxy {
            proxy.did_close(uri);
        }
        // Whether or not the close reached a daemon (it may be down, or the link between
        // connections), the client's diagnostics of the document are cleared now (plan §3.2); the
        // daemon's own empty set is dropped by the proxy, which prunes the document at its close.
        if let Some((_, notifier)) = &self.watch {
            let _ = notifier.notify("textDocument/publishDiagnostics", &translate::cleared(uri));
        }
    }

    fn definition(
        &mut self,
        params: PositionParams,
        budget: Duration,
    ) -> Result<WireEnvelope, Failure> {
        let proxy = self.proxy()?;
        match proxy.call::<Definition>(params, budget) {
            Ok(response) => match response.outcome {
                Outcome::Result(envelope) => Ok(envelope),
                Outcome::Error(e) => Err(Failure::new(e.code, e.message)),
            },
            Err(e) => {
                proxy.abandon();
                Err(call_failure(&e))
            }
        }
    }

    fn watch(&mut self, encoding: PositionEncoding, notifier: Notifier) {
        self.watch = Some((encoding, notifier));
        self.attach();
    }

    fn publish(&mut self) {
        let Some((encoding, notifier)) = &self.watch else {
            return;
        };
        let Some(proxy) = &mut self.proxy else {
            return;
        };
        proxy.poll();
        for published in proxy.take_published() {
            if published.position_encoding != *encoding {
                continue;
            }
            let echo = Echo {
                named: Some(&published.uri),
                opened: None,
            };
            let params = translate::publish_diagnostics(&published, &echo);
            // A session that has ended takes nothing more: nobody reads it.
            let _ = notifier.notify("textDocument/publishDiagnostics", &params);
        }
        if proxy.phase() == super::proxy::ProxyPhase::Disconnected {
            self.attach();
        }
    }

    fn idle(&mut self) -> bool {
        let Some(bell) = &self.bell else {
            return false;
        };
        let fd = self.proxy.as_ref().and_then(Proxy::fd);
        // A wait that fails cannot be trusted to block: the link waits on its queue instead, and
        // publications are handed on after the next item.
        bell.wait(fd, IDLE_WAIT).is_ok()
    }
}

/// The first wait after a failed attach before the next is tried.
pub const ATTACH_BACKOFF: Duration = Duration::from_millis(250);

/// The longest wait between attaches.
pub const ATTACH_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// When the next attach may be tried: deadlines only, compared when an item comes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backoff {
    until: Option<Instant>,
    wait: Duration,
}

impl Backoff {
    /// Whether an attach at `now` must wait.
    #[must_use]
    pub fn blocks(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// An attach failed at `now`: the next waits twice as long as the last, within the bounds.
    pub fn failed(&mut self, now: Instant) {
        self.wait = if self.wait.is_zero() {
            ATTACH_BACKOFF
        } else {
            self.wait.saturating_mul(2).min(ATTACH_BACKOFF_MAX)
        };
        self.until = now.checked_add(self.wait);
    }

    /// An attach succeeded: the next failure starts from the first wait.
    pub fn succeeded(&mut self) {
        *self = Self::default();
    }
}

/// The longest single wait of an idle link: it wakes earlier for any item or frame; a wait that
/// ends with neither is only a look around.
const IDLE_WAIT: Duration = Duration::from_secs(3600);

/// What `jfl lsp` runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspConfig {
    /// The daemon's state directory.
    pub state_dir: PathBuf,
    /// The idle period of a daemon this session starts; `None` for the daemon's default.
    pub idle: Option<Duration>,
}

impl LspConfig {
    /// The arguments after `lsp`: `[--state-dir DIR] [--idle-timeout SECONDS]`; without
    /// `--state-dir`, [`super::default_state_dir`].
    pub fn from_args(args: &[OsString]) -> Result<Self, String> {
        let daemon = super::DaemonConfig::from_args(args)?;
        if daemon.notify.is_some() {
            return Err("unknown argument --notify".to_owned());
        }
        let idle = args
            .iter()
            .any(|a| a == "--idle-timeout")
            .then_some(daemon.idle);
        Ok(Self {
            state_dir: daemon.state_dir,
            idle,
        })
    }
}

/// Runs `jfl lsp` over the process's standard input and output until the session ends; the
/// process's exit code ([`exit_code`]).
#[must_use]
pub fn run(config: &LspConfig) -> u8 {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(_) => PathBuf::from("jfl"),
    };
    let (items, queue) = sync_channel(LINK_QUEUE);
    let Ok((ringer, bell)) = jfl_adapter_ipc::bell::bell() else {
        return 1;
    };
    let owned = config.clone();
    // Not joined: when the session ends the link stops after its current call; the process
    // exits meanwhile. The proxy is made on the link's thread, which alone uses it.
    let started = thread::Builder::new()
        .name("jfl-lsp-link".to_owned())
        .spawn(move || {
            let name = format!("lsp{}", std::process::id());
            let mut daemon = ProxyDaemon::new(owned, &program, &name).with_bell(bell);
            link(&queue, &mut daemon);
        });
    if started.is_err() {
        return 1;
    }
    let phase = Arc::new(AtomicU8::new(Phase::Starting as u8));
    let session =
        Session::new(items, Arc::clone(&phase), env!("CARGO_PKG_VERSION")).with_ringer(ringer);
    let ended = serve(io::stdin(), io::stdout(), session, &Config::default());
    exit_code(Phase::of(phase.load(Ordering::Acquire)), &ended)
}

/// What a request whose proxy call failed is answered: `ServerCancelled` (the client may retry)
/// for every end at the request's deadline, a reply's or the spawn lock's another process still
/// held (IDX-12b), `InternalError` for anything else. Pinned by
/// `lsp::tests::should_answer_server_cancelled_when_the_call_ended_on_a_held_spawn_lock`.
fn call_failure(error: &ProxyError) -> Failure {
    let code = match error {
        ProxyError::TimedOut | ProxyError::Lock(LockError::Held { .. }) => SERVER_CANCELLED,
        _ => ErrorCode::InternalError,
    };
    Failure::new(code, format!("the daemon did not answer: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_answer_server_cancelled_when_the_call_ended_on_a_held_spawn_lock() {
        // given: the ends of a call at its deadline, and a failure that is not one
        let held = ProxyError::Lock(LockError::Held {
            path: PathBuf::from("/state/spawn.lock"),
        });
        let timed_out = ProxyError::TimedOut;
        let closed = ProxyError::Closed;

        // when
        let codes = [&held, &timed_out, &closed].map(|e| call_failure(e).code);

        // then
        assert_eq!(
            codes,
            [SERVER_CANCELLED, SERVER_CANCELLED, ErrorCode::InternalError]
        );
    }

    /// A daemon that counts the definitions it is asked and answers each with an error.
    struct Counting(u32);

    impl Daemon for Counting {
        fn open(&mut self, _: DocumentUri, _: i32, _: String) {}
        fn change(&mut self, _: DocumentUri, _: i32, _: String) {}
        fn close(&mut self, _: &DocumentUri) {}
        fn definition(&mut self, _: PositionParams, _: Duration) -> Result<WireEnvelope, Failure> {
            self.0 = self.0.saturating_add(1);
            Err(Failure::new(ErrorCode::NoSymbol, "asked"))
        }
    }

    #[test]
    fn should_answer_server_cancelled_without_asking_when_the_deadline_passed_in_the_queue() {
        // given: one request whose deadline has passed, one with time left
        let (items, queue) = sync_channel(4);
        let params = PositionParams {
            uri: DocumentUri::parse("file:///w/A.java").unwrap(),
            line: 0,
            col: 0,
            position_encoding: PositionEncoding::Utf16,
            clamp: true,
        };
        let mut answers = Vec::new();
        for deadline in [Instant::now(), Instant::now() + CALL_BUDGET] {
            let (reply, answer) = sync_channel(1);
            items
                .send(Item::Definition {
                    params: params.clone(),
                    deadline,
                    cancel: Arc::new(AtomicBool::new(false)),
                    reply,
                })
                .unwrap();
            answers.push(answer);
        }
        drop(items);
        let mut daemon = Counting(0);

        // when
        link(&queue, &mut daemon);

        // then
        let codes: Vec<ErrorCode> = answers
            .iter()
            .map(|a| a.recv().unwrap().unwrap_err().code)
            .collect();
        assert_eq!(codes, [SERVER_CANCELLED, ErrorCode::NoSymbol]);
        assert_eq!(daemon.0, 1);
    }

    fn ended(input: End) -> Ended {
        Ended {
            input,
            output: None,
        }
    }

    #[test]
    fn should_back_off_doubling_and_reset_when_attaching_fails_then_succeeds() {
        // given
        let t0 = Instant::now();
        let mut retry = Backoff::default();
        let free = !retry.blocks(t0);

        // when
        let mut waits = Vec::new();
        for _ in 0..10 {
            retry.failed(t0);
            waits.push(retry.wait);
        }
        let blocked = retry.blocks(t0);
        let after = t0.checked_add(ATTACH_BACKOFF_MAX).map(|t| retry.blocks(t));
        retry.succeeded();

        // then
        assert!(free);
        assert_eq!(
            waits[..3],
            [ATTACH_BACKOFF, ATTACH_BACKOFF * 2, ATTACH_BACKOFF * 4]
        );
        assert_eq!(waits.last(), Some(&ATTACH_BACKOFF_MAX));
        assert!(blocked);
        assert_eq!(after, Some(false));
        assert!(!retry.blocks(t0));
    }

    #[test]
    fn should_exit_zero_only_after_shutdown_when_the_session_ends() {
        // given
        let cases = [
            (Phase::ExitedClean, ended(End::Exit), 0),
            (Phase::ShutDown, ended(End::Eof), 0),
            (Phase::ExitedEarly, ended(End::Exit), 1),
            (Phase::Running, ended(End::Eof), 1),
            (Phase::Starting, ended(End::Eof), 1),
            (
                Phase::ShutDown,
                ended(End::Read(io::ErrorKind::BrokenPipe)),
                1,
            ),
            (
                Phase::ExitedClean,
                Ended {
                    input: End::Exit,
                    output: Some(io::ErrorKind::BrokenPipe),
                },
                1,
            ),
        ];

        // when
        let codes: Vec<u8> = cases.iter().map(|(p, e, _)| exit_code(*p, e)).collect();

        // then
        let expected: Vec<u8> = cases.iter().map(|(_, _, code)| *code).collect();
        assert_eq!(codes, expected);
    }
}
