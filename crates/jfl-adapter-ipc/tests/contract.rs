//! The `endpoint` contract suite (`jfl_testkit::contracts::endpoint`, ver §5) against the real
//! carrier: the daemon's [`Listener`] and its connections on a current-thread Tokio runtime, and
//! the proxy's [`ClientConn`], over a Unix socket in a scratch state directory.
//!
//! The server runs on a thread of its own, as the daemon does: its reader reads ahead into a
//! channel and its writer takes the frames the suite hands it, so a frame larger than the socket's
//! buffers goes through while the other side is busy, in either direction.
//!
//! Each case is a test of its own (`<case>::should_pass_when_the_carrier_is_the_unix_socket`), so
//! a case that fails or hangs on one platform is named by its test (FND-19: the whole suite was one
//! test, killed at nextest's 120 s on macOS with nothing to say which case hung).
//!
//! Every wait here is bounded (FND-19): a stall of [`real::GENEROUS`] on any one wait fails the case
//! instead of hanging it, with no clock and no sleep: each wait is one `poll(2)` on the socket, or
//! on a [`Bell`] the server thread rings with every event it posts.
//!
//! **Why `open` waits for the accept.** macOS's listener skips a peer that is gone by its accept
//! (XNU answers Tokio's `LOCAL_PEEREPID` only for a live peer: `jfl-adapter-ipc`'s `server`
//! module), and two cases close their client at once (`clean_end_is_final`,
//! `truncated_frame_is_not_clean`). Before FND-19, `open` returned once the client connected, the
//! server thread accepted whenever it ran, and on macOS a case that closed first lost its
//! connection: the server's read waited for ever. Linux keeps a gone peer's credentials, so it
//! never showed there. The harness now answers the uid lookup as XNU does on every platform (a
//! peer gone by its accept is skipped), and the late accept, forced on Linux, loses both cases
//! every time (`should_lose_the_cases_that_close_first_when_the_accept_comes_after_them`).

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use jfl_adapter_ipc::message::End;
use jfl_adapter_ipc::{
    Accepted, Bell, ClientConn, Incoming, Listener, Received, RecvError, Ringer, SocketPath,
    StateDir, current_uid,
};
use jfl_testkit::contracts::endpoint::{
    Connection, EndpointUnderTest, Got, Read, Refused, case_names, check_case,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};

#[cfg(test)]
mod support;
use support::{Scratch, runtime};

#[cfg(test)]
mod real {
    use super::*;

    /// The longest any one wait lasts: a frame already sent, the end of a closed connection, the
    /// accept of a connected client or room in the socket's buffer must come well within it. Each
    /// wait is bounded by it on its own; a case whose bytes keep flowing is never cut short.
    pub(super) const GENEROUS: Duration = Duration::from_secs(20);

    /// Waits at most `wait` for `fd` to be ready for `flags` (or hung up); `false` when it was not.
    /// An interrupted wait counts as not ready (the test process installs no signal handler).
    fn ready(fd: BorrowedFd<'_>, flags: PollFlags, wait: Duration) -> bool {
        let mut fds = [PollFd::new(&fd, flags)];
        let Ok(timeout) = Timespec::try_from(wait) else {
            return false;
        };
        matches!(poll(&mut fds, Some(&timeout)), Ok(n) if n > 0)
    }

    /// Whether the peer of the accepted connection `fd` is gone (hung up): a poll of no events that
    /// reports `POLLHUP` alone, without waiting. It stands in for XNU's `LOCAL_PEEREPID` failing
    /// with `ENOTCONN` for a peer that closed; a half-close (`SHUT_WR`) is no hang-up on either.
    /// It differs for a peer that calls `shutdown(SHUT_RDWR)` without closing: `POLLHUP` on Linux,
    /// still connected (and served) on XNU. No case does that.
    fn peer_gone(fd: BorrowedFd<'_>) -> bool {
        let mut fds = [PollFd::new(&fd, PollFlags::empty())];
        let now = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        matches!(poll(&mut fds, Some(&now)), Ok(n) if n > 0)
            && fds.iter().any(|f| f.revents().contains(PollFlags::HUP))
    }

    /// Events or commands that may wait in a channel: far more than any case leaves unread, so a send
    /// never waits (a full channel would hold the server thread's runtime, and the case would then
    /// fail on its bounded wait).
    const QUEUE: usize = 64;

    /// When the server thread accepts the connection.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Accept {
        /// Before `open` returns: the contract's rule.
        BeforeTheCase,
        /// At the case's first server read, after the case's first steps: the order `open` allowed
        /// before FND-19, forced. Linux only, as its one test.
        #[cfg(target_os = "linux")]
        AtTheFirstRead,
    }

    /// What the server thread tells the test.
    enum Event {
        Bound(Result<(), String>),
        Accepted,
        Read(Incoming),
    }

    /// Why no event came.
    #[derive(Debug, PartialEq, Eq)]
    enum Silence {
        /// The server thread ended: nothing more will come.
        Ended,
        /// Nothing within [`GENEROUS`].
        TimedOut,
    }

    /// The server thread's events, each posted then rung, so the test waits on the bell with a
    /// bound.
    struct Inbox {
        events: mpsc::Receiver<Event>,
        bell: Bell,
    }

    impl Inbox {
        fn next(&self) -> Result<Event, Silence> {
            loop {
                match self.events.try_recv() {
                    Ok(event) => return Ok(event),
                    Err(TryRecvError::Disconnected) => return Err(Silence::Ended),
                    Err(TryRecvError::Empty) => {
                        let woken = self
                            .bell
                            .wait(None, GENEROUS)
                            .map_err(|_| Silence::TimedOut)?;
                        if !woken.rang {
                            return Err(Silence::TimedOut);
                        }
                    }
                }
            }
        }
    }

    /// The server thread's side of the inbox.
    #[derive(Clone)]
    struct Outbox {
        events: mpsc::SyncSender<Event>,
        ringer: Ringer,
    }

    impl Outbox {
        fn post(&self, event: Event) -> bool {
            let sent = self.events.send(event).is_ok();
            self.ringer.ring();
            sent
        }
    }

    /// Rings once the server thread is done, after every sender is gone, so the test's wait sees
    /// the end at once.
    struct RingAtTheEnd(Ringer);

    impl Drop for RingAtTheEnd {
        fn drop(&mut self) {
            self.0.ring();
        }
    }

    enum Command {
        Send(Vec<u8>),
        Close,
    }

    /// The real carrier: one scratch state directory and listener per connection.
    pub(super) struct Real(pub(super) Accept);

    impl EndpointUnderTest for Real {
        fn open(&mut self) -> Result<Box<dyn Connection>, String> {
            RealConn::open(self.0).map(|conn| Box::new(conn) as Box<dyn Connection>)
        }
    }

    struct RealConn {
        client: Option<ClientConn>,
        inbox: Inbox,
        go: Option<tokio::sync::oneshot::Sender<()>>,
        commands: Option<tokio::sync::mpsc::Sender<Command>>,
        server: Option<JoinHandle<()>>,
        _scratch: Scratch,
    }

    impl RealConn {
        fn open(accept: Accept) -> Result<Self, String> {
            let scratch = Scratch::new();
            let at = |e: &dyn std::fmt::Display| format!("{}: {e}", scratch.path().display());
            let state = StateDir::open(&scratch.state()).map_err(|e| at(&e))?;
            let path = SocketPath::of_state(&state).map_err(|e| at(&e))?;
            let (ringer, bell) = jfl_adapter_ipc::bell::bell().map_err(|e| at(&e))?;
            let (events, received) = mpsc::sync_channel(QUEUE);
            let out = Outbox { events, ringer };
            let (go, gone) = tokio::sync::oneshot::channel::<()>();
            let (commands, command) = tokio::sync::mpsc::channel(QUEUE);
            let socket = path.clone();
            let server = std::thread::spawn(move || {
                let _end = RingAtTheEnd(out.ringer.clone());
                runtime().block_on(serve(socket, out, gone, command));
            });
            let mut conn = Self {
                client: None,
                inbox: Inbox {
                    events: received,
                    bell,
                },
                go: Some(go),
                commands: Some(commands),
                server: Some(server),
                _scratch: scratch,
            };
            match conn.inbox.next() {
                Ok(Event::Bound(Ok(()))) => {}
                Ok(Event::Bound(Err(e))) => return Err(e),
                Ok(_) => return Err("the server thread posted before its bind".to_owned()),
                Err(silence) => return Err(format!("no bind from the server thread: {silence:?}")),
            }
            conn.client = Some(ClientConn::connect(path.path()).map_err(|e| e.to_string())?);
            if accept == Accept::BeforeTheCase {
                conn.accept()?;
            }
            Ok(conn)
        }

        /// Lets the server thread accept and waits for its accept, at most [`GENEROUS`].
        fn accept(&mut self) -> Result<(), String> {
            let Some(go) = self.go.take() else {
                return Ok(());
            };
            let _sent = go.send(());
            match self.inbox.next() {
                Ok(Event::Accepted) => Ok(()),
                Ok(_) => Err("the server read before its accept".to_owned()),
                Err(silence) => Err(format!("the server accepted nothing: {silence:?}")),
            }
        }

        fn command(&self, command: Command) {
            if let Some(commands) = &self.commands {
                let _sent = commands.try_send(command);
            }
        }

        /// A duplicate of the client's descriptor, to wait on while the client is borrowed.
        fn client_fd(&self) -> Option<OwnedFd> {
            self.client
                .as_ref()
                .and_then(|client| client.as_fd().try_clone_to_owned().ok())
        }
    }

    /// The server thread: binds, waits for its go, accepts one peer (skipping it when it is gone, as
    /// macOS's uid lookup does), reads ahead into the inbox and writes what the test hands it.
    async fn serve(
        socket: SocketPath,
        out: Outbox,
        go: tokio::sync::oneshot::Receiver<()>,
        mut command: tokio::sync::mpsc::Receiver<Command>,
    ) {
        let listener = match Listener::bind(&socket, current_uid()) {
            Ok(listener) => listener,
            Err(e) => {
                out.post(Event::Bound(Err(e.to_string())));
                return;
            }
        };
        out.post(Event::Bound(Ok(())));
        if go.await.is_err() {
            return;
        }
        let Ok(Accepted::Peer(mut reader, mut writer)) = listener.accept().await else {
            return;
        };
        // XNU's `LOCAL_PEEREPID` fails for a peer gone by its accept, and the listener then skips it
        // (`server` module); Linux keeps the credentials and serves it. The same answer on every
        // platform: this listener has no other peer, so the server ends.
        if peer_gone(reader.as_fd()) {
            return;
        }
        out.post(Event::Accepted);
        let reads = out.clone();
        let reading = tokio::spawn(async move {
            loop {
                let incoming = reader.next().await;
                let ended = matches!(incoming, Incoming::End(_));
                if !reads.post(Event::Read(incoming)) {
                    return;
                }
                if ended {
                    // The read after an end, which must be the same end.
                    reads.post(Event::Read(reader.next().await));
                    return;
                }
            }
        });
        while let Some(Command::Send(body)) = command.recv().await {
            if writer.send(&body).await.is_err() {
                break;
            }
        }
        drop(writer);
        reading.abort();
    }

    impl Drop for RealConn {
        fn drop(&mut self) {
            self.client = None;
            self.go = None;
            self.commands = None;
            // The thread ends once its peer and its commands are gone; one that does not within the
            // bound is left behind rather than joined for ever (the case has failed by then).
            loop {
                match self.inbox.next() {
                    Ok(_) => {}
                    Err(Silence::Ended) => {
                        if let Some(server) = self.server.take() {
                            let _joined = server.join();
                        }
                        return;
                    }
                    Err(Silence::TimedOut) => return,
                }
            }
        }
    }

    /// Writes `bytes` whole to the non-blocking socket `fd`, waiting for room at most [`GENEROUS`]
    /// at a time.
    fn write_raw(fd: BorrowedFd<'_>, mut bytes: &[u8]) {
        use rustix::io::Errno;
        while !bytes.is_empty() {
            match rustix::io::write(fd, bytes) {
                Ok(n) => bytes = bytes.get(n..).unwrap_or_default(),
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => assert!(
                    ready(fd, PollFlags::OUT, GENEROUS),
                    "the server took nothing of a raw write for {GENEROUS:?}, {} bytes left",
                    bytes.len()
                ),
                Err(e) => panic!("raw write: {e}"),
            }
        }
    }

    impl Connection for RealConn {
        fn client_send(&mut self, body: &[u8]) -> Result<(), Refused> {
            let fd = self.client_fd();
            let client = self
                .client
                .as_mut()
                .ok_or(Refused::Failed("closed".into()))?;
            // Each wait for room is one poll of at most GENEROUS; the client's own poll then finds it.
            let mut remaining = || {
                fd.as_ref()
                    .is_some_and(|fd| ready(fd.as_fd(), PollFlags::OUT, GENEROUS))
                    .then_some(Duration::ZERO)
            };
            match client.send(body, &mut remaining) {
                Ok(()) => Ok(()),
                Err(RecvError::Framing(jfl_protocol::FrameError::TooLarge { .. })) => {
                    Err(Refused::TooLarge)
                }
                Err(e) => Err(Refused::Failed(e.to_string())),
            }
        }

        fn client_write_raw(&mut self, bytes: &[u8]) {
            if let Some(client) = &self.client {
                write_raw(client.as_fd(), bytes);
            }
        }

        fn client_recv(&mut self, wait: Duration) -> Got {
            let fd = self.client_fd();
            let Some(client) = self.client.as_mut() else {
                return Got::Failed("closed".to_owned());
            };
            // Each wait for bytes is one poll of at most `wait`: a short one times out once nothing
            // comes, a generous one for as long as bytes keep coming.
            let mut remaining = || {
                fd.as_ref()
                    .is_some_and(|fd| ready(fd.as_fd(), PollFlags::IN, wait))
                    .then_some(Duration::ZERO)
            };
            match client.recv(&mut remaining) {
                Ok(Received::Frame(body)) => Got::Frame(body),
                Ok(Received::Eof) => Got::Eof,
                Err(RecvError::TimedOut) => Got::TimedOut,
                Err(e) => Got::Failed(e.to_string()),
            }
        }

        fn client_close(&mut self) {
            self.client = None;
        }

        fn server_next(&mut self) -> Read {
            if self.accept().is_err() {
                return Read::Stalled;
            }
            match self.inbox.next() {
                Ok(Event::Read(Incoming::Call(call))) => Read::Call {
                    id: call.id,
                    method: call.method,
                    params: call.params,
                },
                Ok(Event::Read(Incoming::Note(note))) => Read::Note {
                    method: note.method,
                    params: note.params,
                },
                Ok(Event::Read(Incoming::Malformed(malformed))) => Read::Malformed {
                    id: malformed.id,
                    code: malformed.code,
                },
                Ok(Event::Read(Incoming::Oversized(oversized))) => Read::Oversized {
                    declared: oversized.declared,
                },
                Ok(Event::Read(Incoming::End(end))) => Read::End {
                    clean: end == End::Eof,
                },
                Ok(Event::Bound(_) | Event::Accepted) | Err(_) => Read::Stalled,
            }
        }

        fn server_send(&mut self, body: &[u8]) {
            self.command(Command::Send(body.to_vec()));
        }

        fn server_close(&mut self) {
            self.command(Command::Close);
        }
    }
}
use real::{Accept, Real};

/// One test per case of the suite, named after it, on a connection the server accepted first.
macro_rules! cases {
    ($($case:ident),* $(,)?) => {
        $(
            mod $case {
                #[test]
                fn should_pass_when_the_carrier_is_the_unix_socket() {
                    // given
                    let mut endpoint = super::Real(super::Accept::BeforeTheCase);

                    // when
                    let outcome = super::check_case(&mut endpoint, stringify!($case));

                    // then
                    assert_eq!(outcome, Ok(()));
                }
            }
        )*

        /// The cases this file runs, by name.
        const RUN: &[&str] = &[$(stringify!($case)),*];
    };
}

cases!(
    frames_in_order,
    large_frames_whole,
    malformed_keeps_framing,
    oversized_skipped,
    client_refuses_oversized,
    clean_end_is_final,
    server_close_is_eof,
    timed_out_read_loses_nothing,
    truncated_frame_is_not_clean,
    lost_framing_ends,
);

#[test]
fn should_run_every_case_of_the_endpoint_contract_when_the_carrier_is_the_unix_socket() {
    // given
    let names = case_names();

    // when
    let run = RUN.to_vec();

    // then
    assert_eq!(run, names);
}

/// The macOS hang (FND-19), forced on Linux: the server accepts after the case's first steps, as
/// `open` allowed before, and a peer gone by then is skipped, as macOS's listener does. The two
/// cases that close their client at once lose everything the client sent, every time. Linux only:
/// on macOS the listener's own lookup skips the peer and its accept then waits for another, so the
/// forced order would cost each case two [`real::GENEROUS`] waits (the accept in `server_next`,
/// then the drain in `Drop`), about 80 s for the pair, and leave a server thread parked in its
/// accept; the per-case tests above are the check there.
#[cfg(target_os = "linux")]
#[test]
fn should_lose_the_cases_that_close_first_when_the_accept_comes_after_them() {
    // given
    let closing_first = ["clean_end_is_final", "truncated_frame_is_not_clean"];

    // when
    let outcomes: Vec<_> = closing_first
        .iter()
        .map(|case| check_case(&mut Real(Accept::AtTheFirstRead), case))
        .collect();

    // then: each failed, named, because the server read nothing
    let failed: Vec<_> = outcomes
        .iter()
        .map(|outcome| {
            outcome
                .as_ref()
                .map_err(|v| (v.case, v.detail.starts_with("the server read Stalled,")))
        })
        .collect();
    assert_eq!(
        failed,
        [
            Err(("clean_end_is_final", true)),
            Err(("truncated_frame_is_not_clean", true)),
        ]
    );
}
