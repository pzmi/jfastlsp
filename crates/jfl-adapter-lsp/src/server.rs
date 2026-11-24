//! The session: a synchronous main loop, a bounded request pool and the writer (ADR 0020, the
//! rust-analyzer and ruff scheduler shape).
//!
//! - **One main loop** takes the client's messages in order. A notification is applied to the
//!   [`Handler`] there, before the next message is read, so `didChange` is never reordered against
//!   a request. A request is either answered on the spot ([`Dispatch::Now`], for what needs no
//!   work: an unknown method, `initialize`), or handed to the pool with a snapshot of the state
//!   taken at that moment ([`Dispatch::Pool`]): a later notification never changes what an earlier
//!   request sees.
//! - **The pool** is a fixed number of threads behind a bounded queue: a full queue stops the main
//!   loop, which stops reading, so a client that floods requests is slowed, never buffered without
//!   bound.
//! - **The writer** owns the output, so frames never interleave. Every reply goes through it.
//! - **Cancellation is bookkeeping, not preemption.** `$/cancelRequest` marks the request's
//!   [`Cancel`] flag if it is still in flight (a job may check it and stop early); a request whose
//!   flag is set when its job returns is answered `RequestCancelled` (-32800) instead of its result.
//!   A cancel that arrives after the answer was sent, or names an id never seen, is ignored.
//! - **Every request is answered exactly once**: by its reply, `RequestCancelled` (a request
//!   cancelled while queued never starts), `MethodNotFound`, or `InternalError` when the handler,
//!   the snapshot or the job panics. A request that reuses an id still in flight is answered
//!   `InvalidRequest` with a null id, so the client's pending request under that id is not settled
//!   by it.
//! - **A panic in the handler does not end or hang the session.** `request`, `snapshot` and
//!   `notification` run under `catch_unwind` like the jobs; a handler that panics mid-update may
//!   leave its own state half applied, which is its bug to fix, but the session keeps serving
//!   (fnd §4.3 rule 3).
//!
//! - **The server's own notifications** (`textDocument/publishDiagnostics`, LSP-04) go through the
//!   same writer: [`Handler::attach`] hands the handler a [`Notifier`] before the first message,
//!   which any thread may use, so they never interleave with a reply. It holds the writer's queue
//!   only while the session runs: once [`serve`] is returning, a notification is refused, so a
//!   thread that keeps its notifier can never keep the session from ending.
//! - **The handler may end the session** ([`Handler::ended`], LSP's `exit`, plan §2.5): checked
//!   after each notification; the loop then stops reading, and [`serve`] returns once every job
//!   finished and every reply was written, with [`End::Exit`].
//!
//! The pool and the writer are scoped threads: [`serve`] returns only after every job finished and
//! every reply was written. The reader is not: it blocks in `read`, which nothing can interrupt
//! portably, so a session the handler ended returns without waiting for its client to close the
//! input (the reader then stops at its next read, whose message has no one to take it; in a
//! process that exits, at once). Pinned by `tests/server.rs` and
//! `tests/properties.rs::should_answer_every_request_once_when_traffic_is_mixed`.

use std::collections::HashMap;
use std::io::{self, BufWriter, Read, Write};
use std::num::NonZeroUsize;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use jfl_protocol::frame::{DEFAULT_MAX_FRAME_LEN, Oversized};
use jfl_protocol::rpc::{ErrorCode, ErrorObject};
use lsp_server::{Message, Notification, Request, RequestId};
use serde::{Deserialize, Serialize};

use crate::transport::{End, FrameReader, Incoming, Outgoing, write_one};

/// Messages read ahead of the main loop: none. The reader hands each parsed message over and only
/// then reads the next, as rust-analyzer's reader does, so input held in memory is bounded by
/// frames, not by a queue: at most the frame being decoded (`max_frame_len`) plus two parsed
/// messages (the one handed over and the one the main loop applies). For a 16 MiB limit that is
/// about 48 MiB of text when the frames are documents (`didOpen`: one long string), and in the
/// worst case, a body of tiny array elements whose parsed `serde_json::Value` is up to about 16
/// times its text, about 0.5 GiB. Queued pool requests hold only what the handler keeps of their
/// params (at most `Config::queue` of them).
const INCOMING_CAPACITY: usize = 0;
/// Replies queued for the writer before a worker or the main loop waits on the client to read.
/// Each holds a serialized result (about 1.4 MB for 10,000 locations).
const OUTGOING_CAPACITY: usize = 16;

/// How a session is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// The largest frame body read (plan §2.5: 16 MiB by default); a larger one is skipped and
    /// answered with an error.
    pub max_frame_len: usize,
    /// The threads that run requests.
    pub workers: NonZeroUsize,
    /// Requests waiting for a worker before the main loop stops reading.
    pub queue: usize,
}

impl Default for Config {
    /// 16 MiB frames, four workers (the proxy's work is mostly waiting on the daemon) and a queue
    /// of 16 requests. Not measured yet: LSP-02 sizes them against a daemon.
    fn default() -> Self {
        Self {
            max_frame_len: DEFAULT_MAX_FRAME_LEN,
            workers: NonZeroUsize::new(4).unwrap_or(NonZeroUsize::MIN),
            queue: 16,
        }
    }
}

/// The state a session serves: it applies notifications and turns requests into work.
pub trait Handler {
    /// What a request on the pool reads: an immutable view of the state when it arrived.
    type Snapshot: Send + Sync + 'static;

    /// The state as it is now, for a request handed to the pool.
    fn snapshot(&self) -> Arc<Self::Snapshot>;

    /// What to do with a request (any but `$/cancelRequest`, which the loop handles). Runs on the
    /// main loop, so it must not block.
    fn request(&mut self, request: Request) -> Dispatch<Self::Snapshot>;

    /// Applies a notification, in order. One the handler does not know is ignored, never answered
    /// (plan §2.5).
    fn notification(&mut self, notification: Notification);

    /// `$/cancelRequest` marked request `id`, still in flight, cancelled: told on the main loop, in
    /// order with the other messages, so the handler can drop work it queued for the request
    /// before that work starts. The answer is the loop's business either way (`RequestCancelled`).
    ///
    /// Pinned by: `tests/server.rs::should_tell_the_handler_when_a_request_in_flight_is_cancelled`.
    fn cancelled(&mut self, id: &RequestId) {
        let _ = id;
    }

    /// The session's notifier, handed over once before the first message is read: how the server
    /// sends the client notifications of its own, from any thread. Ignored by default.
    ///
    /// Pinned by: `tests/server.rs::should_write_the_handlers_notifications_and_refuse_them_when_the_session_has_ended`.
    fn attach(&mut self, notifier: Notifier) {
        let _ = notifier;
    }

    /// Whether the session ends now, checked after each notification (LSP's `exit`): the loop
    /// reads nothing more, lets every request in flight finish and its answer be written, and
    /// [`serve`] returns [`End::Exit`].
    ///
    /// Pinned by: `tests/server.rs::should_end_the_session_and_answer_what_is_in_flight_when_the_handler_ends_it`.
    fn ended(&self) -> bool {
        false
    }
}

/// What becomes of a request.
pub enum Dispatch<S> {
    /// Answered on the main loop, in order with the notifications around it.
    Now(Reply),
    /// Run on the pool against a snapshot taken now.
    Pool(Job<S>),
    /// No such method: answered `MethodNotFound` (-32601).
    MethodNotFound,
}

/// The work of one request on the pool: reads the snapshot, may check the cancel flag.
type JobFn<S> = dyn FnOnce(&S, &Cancel) -> Reply + Send;

/// The work of one request on the pool.
pub struct Job<S>(Box<JobFn<S>>);

impl<S> Job<S> {
    /// A job: reads the snapshot, may check the cancel flag, returns the reply.
    pub fn new(work: impl FnOnce(&S, &Cancel) -> Reply + Send + 'static) -> Self {
        Self(Box::new(work))
    }
}

/// A request's cancel flag, set by `$/cancelRequest` while it is in flight.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// Whether the client cancelled the request.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn set(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// A request's answer.
#[derive(Debug)]
pub struct Reply(ReplyKind);

#[derive(Debug)]
enum ReplyKind {
    Result(Vec<u8>),
    Error(ErrorObject),
}

impl Reply {
    /// A result, serialized from its typed value straight to JSON: never through a
    /// `serde_json::Value` (plan §5.3). A value that cannot be written as JSON is an
    /// `InternalError`.
    ///
    /// Pinned by: `server::tests::should_serialize_the_typed_value_when_a_result_is_made`.
    pub fn result<T: Serialize + ?Sized>(value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self(ReplyKind::Result(body)),
            Err(_) => Self::error(ErrorCode::InternalError, "the result cannot be written"),
        }
    }

    /// An error.
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self(ReplyKind::Error(ErrorObject {
            code,
            message: message.into(),
            data: None,
        }))
    }

    fn into_outgoing(self, id: RequestId) -> Outgoing {
        match self.0 {
            ReplyKind::Result(body) => Outgoing::Result { id, body },
            ReplyKind::Error(error) => Outgoing::Error {
                id: Some(id),
                error,
            },
        }
    }
}

/// Sends the client notifications of the server's own, from any thread, through the session's
/// writer. A send waits while the writer's queue is full (a client that does not read holds the
/// sender back, never a buffer without bound) and is refused once the session has ended.
#[derive(Debug, Clone)]
pub struct Notifier(Arc<Mutex<Option<SyncSender<Outgoing>>>>);

impl Notifier {
    /// Sends `method` with `params`, serialized from their typed value: whether the session took
    /// it (`false` once it has ended, or when the params cannot be written as JSON).
    pub fn notify<T: Serialize + ?Sized>(&self, method: &str, params: &T) -> bool {
        let Ok(body) = serde_json::to_vec(params) else {
            return false;
        };
        let outgoing = Outgoing::Notification {
            method: method.to_owned(),
            body,
        };
        // Cloned out of the lock, so a send that waits on a full queue never holds the session's
        // end (`close`) behind it.
        let out = lock(&self.0).clone();
        out.is_some_and(|out| out.send(outgoing).is_ok())
    }

    fn close(&self) {
        *lock(&self.0) = None;
    }
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// Why the input ended.
    pub input: End,
    /// The first write that failed, if the client stopped reading.
    pub output: Option<io::ErrorKind>,
}

/// Serves one session: reads `input` until it ends, writes every answer to `output`, and returns
/// once the last one is written. A framing error ends the session after a `ParseError` with a null
/// id; an oversized frame or a malformed body is answered and the session goes on.
pub fn serve<R, W, H>(input: R, output: W, mut handler: H, config: &Config) -> Ended
where
    R: Read + Send + 'static,
    W: Write + Send,
    H: Handler,
{
    // The pool's queue and the in-flight book are shared by reference with the workers, so they
    // live outside the scope; the main loop holds the queue's only sender.
    let (job_tx, job_rx) = sync_channel::<Work<H::Snapshot>>(config.queue);
    let job_rx = Mutex::new(job_rx);
    let in_flight = Mutex::new(HashMap::<RequestId, Cancel>::new());
    let max_frame_len = config.max_frame_len;

    thread::scope(|scope| {
        // The reader's and writer's channels live inside the scope, and the main loop owns the
        // incoming end, so if the main loop unwinds, the reader's next send fails and it returns:
        // the scope's join then ends instead of waiting on a reader blocked for ever (LSP-01
        // review).
        let (in_tx, in_rx) = sync_channel::<Incoming>(INCOMING_CAPACITY);
        let (out_tx, out_rx) = sync_channel::<Outgoing>(OUTGOING_CAPACITY);
        let writer = scope.spawn(move || write_all(output, &out_rx));
        // Detached (module documentation): a session ended by its handler must not wait for the
        // client to close the input. Failing to start it is an input that ended at once.
        let reader = thread::Builder::new()
            .name("jfl-lsp-reader".to_owned())
            .spawn(move || read_all(input, max_frame_len, &in_tx));
        for _ in 0..config.workers.get() {
            let out_tx = out_tx.clone();
            let (job_rx, in_flight) = (&job_rx, &in_flight);
            scope.spawn(move || work(job_rx, in_flight, &out_tx));
        }
        let notifier = Notifier(Arc::new(Mutex::new(Some(out_tx.clone()))));
        {
            let handler = &mut handler;
            let attached = notifier.clone();
            let _ = panic::catch_unwind(AssertUnwindSafe(|| handler.attach(attached)));
        }
        let mut main = MainLoop {
            handler: &mut handler,
            out: out_tx,
            jobs: job_tx,
            in_flight: &in_flight,
        };
        let input = match reader {
            Ok(_detached) => main.run(in_rx),
            Err(err) => End::Read(err.kind()),
        };
        drop(main);
        // The writer ends once every sender is gone: the handler's notifier must not keep it.
        notifier.close();
        let output = writer.join().unwrap_or(Some(io::ErrorKind::Other));
        Ended { input, output }
    })
}

/// A request handed to the pool.
struct Work<S> {
    id: RequestId,
    snapshot: Arc<S>,
    cancel: Cancel,
    job: Job<S>,
}

struct MainLoop<'a, H: Handler> {
    handler: &'a mut H,
    out: SyncSender<Outgoing>,
    jobs: SyncSender<Work<H::Snapshot>>,
    in_flight: &'a Mutex<HashMap<RequestId, Cancel>>,
}

#[derive(Deserialize)]
struct CancelParams {
    id: RequestId,
}

impl<H: Handler> MainLoop<'_, H> {
    fn run(&mut self, incoming: Receiver<Incoming>) -> End {
        for next in incoming {
            match next {
                Incoming::Message(Message::Request(request)) => self.request(request),
                Incoming::Message(Message::Notification(notification)) => {
                    self.notification(notification);
                    let handler = &*self.handler;
                    if panic::catch_unwind(AssertUnwindSafe(|| handler.ended())).unwrap_or(false) {
                        return End::Exit;
                    }
                }
                // The server sends the client no requests yet, so no response is awaited.
                Incoming::Message(Message::Response(_)) => {}
                Incoming::Malformed(malformed) => {
                    let message = match malformed.code {
                        ErrorCode::ParseError => "the message is not JSON",
                        _ => "the message is not a JSON-RPC request or notification",
                    };
                    self.send_error(malformed.id, malformed.code, message, None);
                }
                Incoming::Oversized(Oversized { declared, limit }) => {
                    let data = serde_json::json!({ "declared": declared, "limit": limit });
                    let message = format!(
                        "a frame of {declared} bytes is larger than the limit of {limit} bytes"
                    );
                    self.send_error(None, ErrorCode::InvalidRequest, message, Some(data));
                }
                Incoming::End(end) => {
                    if let End::Framing(err) = &end {
                        let message = format!("the framing is lost ({err}); closing the session");
                        self.send_error(None, ErrorCode::ParseError, message, None);
                    }
                    return end;
                }
            }
        }
        End::Read(io::ErrorKind::BrokenPipe)
    }

    fn request(&mut self, request: Request) {
        let id = request.id.clone();
        if lock(self.in_flight).contains_key(&id) {
            // A null id: under the reused one, the client would settle its first request with
            // this error and drop the real answer as unknown.
            let message = "a request with this id is already in flight";
            self.send_error(None, ErrorCode::InvalidRequest, message, None);
            return;
        }
        let dispatch = if request.method == "$/cancelRequest" {
            Dispatch::MethodNotFound
        } else {
            let handler = &mut *self.handler;
            panic::catch_unwind(AssertUnwindSafe(|| handler.request(request)))
                .unwrap_or_else(|_| Dispatch::Now(failed()))
        };
        match dispatch {
            Dispatch::Now(reply) => self.send(reply.into_outgoing(id)),
            Dispatch::MethodNotFound => {
                let reply = Reply::error(ErrorCode::MethodNotFound, "method not found");
                self.send(reply.into_outgoing(id));
            }
            Dispatch::Pool(job) => {
                let handler = &*self.handler;
                let Ok(snapshot) = panic::catch_unwind(AssertUnwindSafe(|| handler.snapshot()))
                else {
                    self.send(failed().into_outgoing(id));
                    return;
                };
                // Only this loop inserts, and it checked above, so the id is free.
                let cancel = Cancel::default();
                lock(self.in_flight).insert(id.clone(), cancel.clone());
                let work = Work {
                    id,
                    snapshot,
                    cancel,
                    job,
                };
                // The workers live as long as this loop, so the queue is never closed under it.
                let _ = self.jobs.send(work);
            }
        }
    }

    fn notification(&mut self, notification: Notification) {
        if notification.method == "$/cancelRequest" {
            if let Ok(CancelParams { id }) = serde_json::from_value(notification.params) {
                let found = lock(self.in_flight).get(&id).cloned();
                if let Some(cancel) = found {
                    cancel.set();
                    let handler = &mut *self.handler;
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| handler.cancelled(&id)));
                }
            }
            return;
        }
        // A notification has no answer: a handler that panics applying it loses that one
        // notification and the session goes on (fnd §4.3 rule 3).
        let handler = &mut *self.handler;
        let _ = panic::catch_unwind(AssertUnwindSafe(|| handler.notification(notification)));
    }

    fn send_error(
        &self,
        id: Option<RequestId>,
        code: ErrorCode,
        message: impl Into<String>,
        data: Option<serde_json::Value>,
    ) {
        let error = ErrorObject {
            code,
            message: message.into(),
            data,
        };
        self.send(Outgoing::Error { id, error });
    }

    fn send(&self, outgoing: Outgoing) {
        // The writer drains its queue even after a failed write, so this only fails once it is
        // gone, which it is not while this loop runs.
        let _ = self.out.send(outgoing);
    }
}

fn read_all<R: Read>(input: R, max_frame_len: usize, incoming: &SyncSender<Incoming>) {
    let mut reader = FrameReader::new(input, max_frame_len);
    while let Some(next) = reader.next_incoming() {
        if incoming.send(next).is_err() {
            return;
        }
    }
}

fn work<S>(
    jobs: &Mutex<Receiver<Work<S>>>,
    in_flight: &Mutex<HashMap<RequestId, Cancel>>,
    out: &SyncSender<Outgoing>,
) {
    loop {
        let next = lock(jobs).recv();
        let Ok(Work {
            id,
            snapshot,
            cancel,
            job,
        }) = next
        else {
            return;
        };
        // Cancelled while it waited in the queue: never started.
        let reply = if cancel.is_cancelled() {
            failed()
        } else {
            panic::catch_unwind(AssertUnwindSafe(|| (job.0)(&snapshot, &cancel)))
                .unwrap_or_else(|_| failed())
        };
        // Taken out of the book before the answer is chosen: a cancel that comes later finds
        // nothing and the result stands (it may already be on its way).
        lock(in_flight).remove(&id);
        let reply = if cancel.is_cancelled() {
            Reply::error(ErrorCode::RequestCancelled, "the request was cancelled")
        } else {
            reply
        };
        let _ = out.send(reply.into_outgoing(id));
    }
}

/// Writes every reply; after a failed write it keeps draining, so no sender ever waits on a
/// client that stopped reading. Returns the first failure.
fn write_all<W: Write>(output: W, outgoing: &Receiver<Outgoing>) -> Option<io::ErrorKind> {
    let mut out = BufWriter::new(output);
    let mut failed = None;
    while let Ok(first) = outgoing.recv() {
        let mut next = Some(first);
        while let Some(message) = next.take() {
            if failed.is_none()
                && let Err(err) = write_one(&mut out, message)
            {
                failed = Some(err.kind());
            }
            next = outgoing.try_recv().ok();
        }
        if failed.is_none()
            && let Err(err) = out.flush()
        {
            failed = Some(err.kind());
        }
    }
    failed
}

/// The answer to a request whose handler or job panicked.
fn failed() -> Reply {
    Reply::error(ErrorCode::InternalError, "the request failed")
}

/// A lock that a panicking holder did not poison for good: every holder here leaves the data
/// consistent at each step.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_serialize_the_typed_value_when_a_result_is_made() {
        // given
        let value = vec![gen_lsp_types::Position::new(1, 2)];

        // when
        let reply = Reply::result(&value);

        // then
        match reply.0 {
            ReplyKind::Result(body) => assert_eq!(body, br#"[{"line":1,"character":2}]"#),
            ReplyKind::Error(error) => panic!("{error:?}"),
        }
    }

    #[test]
    fn should_answer_internal_error_when_a_result_cannot_be_written() {
        // given
        let value: HashMap<(u8, u8), u8> = HashMap::from([((1, 2), 3)]);

        // when
        let reply = Reply::result(&value);

        // then
        match reply.0 {
            ReplyKind::Error(error) => assert_eq!(error.code, ErrorCode::InternalError),
            ReplyKind::Result(body) => panic!("{body:?}"),
        }
    }
}
