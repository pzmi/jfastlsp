//! The daemon process (plan §5.2): `setsid` first, then the start order lock, unlink, bind, ready
//! (each failure reported on the notify socket, exit 1), then the serve loop until SIGTERM,
//! `jfl/stop`, the idle period, a lost lock path or a swept socket, then the shutdown order: the
//! proxies' spawn lock (waited for), stop accepting, unlink, release the lock and then the spawn
//! lock, exit 0; an accepted connection's end comes after the release.
//!
//! **Why** (FND-18): a proxy refused between the stop and the release would spawn a daemon that
//! can only fail the lock still held, and retry at once, for as long as the shutdown lasts (the
//! old `daemon-idle-exit-races-connect`, and the version mismatch's reconnect: the old daemon's
//! end of file came before its release). The spawn lock makes such a proxy wait for the release.
//! The daemon waits for the spawn lock at most [`SPAWN_LOCK_WAIT`], or until a second SIGTERM, and
//! then goes on without it ([`Warning`]): a held spawn lock delays the exit, never prevents it.
//! The writers, which end the accepted connections, are drained only after it: the stop closes
//! their outboxes, and on this one-thread runtime none of them runs again before the release, as
//! nothing awaits in between. So a client that reads its connection's end (the version
//! mismatch's proxy, after `jfl/stop`) finds the lock free, and the spawn lock is held for the
//! shutdown's own steps, never for a peer's write deadline. Pinned by
//! `tests/daemon.rs::should_keep_its_listener_until_it_holds_the_spawn_lock_and_release_both_before_the_end_when_stopped`.
//!
//! The loop owns the listener and, per connection, a reader task feeding one bounded channel and a
//! writer task draining the connection's bounded outbox; the host applies every frame and queues
//! its answer at once, so one task owns all the state (fnd §4.4: message passing to an owning
//! task, no lock held across an await) and the loop never waits on a peer. Every wait on a peer
//! has a deadline: a connection that sends no handshake within [`HANDSHAKE_DEADLINE`] is dropped;
//! a reply not written within [`WRITE_DEADLINE`], or an outbox full ([`OUTBOX`]), severs its
//! connection: its tasks end and its socket closes, so no half-written frame is followed by
//! another. A failed accept (out of descriptors) pauses accepting until the next tick, without
//! spinning. Every timer is in [`super::timer`].
//!
//! **Between requests** the loop runs the service's background work ([`Service::background`]:
//! the views' `settle` in slices after a request that opened one, then their `write_back` in
//! slices), one step at a time and only when nothing else is ready (the `select!` is biased, the
//! work last), so a waiting frame, a signal or a tick is taken before the next step. A step runs
//! to its end once started: a slice dropped half way would leave its view's build set for good
//! (IDX-09's wiring notes); each is bounded instead (IDX-09d), so the loop is held for one slice.
//!
//! Pinned by `tests/daemon.rs`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use jfl_adapter_ipc::notify::{self, Notice};
use jfl_adapter_ipc::{
    Accepted, Call, ConnReader, ConnWriter, Flock, Incoming, SocketPath, StateDir, current_uid,
};
use jfl_protocol::messages::{Handshake, Method};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

use super::DaemonConfig;
use super::host::{ConnId, Host, HostConfig, Outbox, Phase, Received, Tick};
use super::service::Service;
use super::timer;

/// How often the loop checks the lock path, the socket path and the idle period.
pub const TICK: Duration = Duration::from_secs(2);

/// How long a new connection has to send its handshake request.
pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);

/// How long writing one reply may take before its connection is severed.
pub const WRITE_DEADLINE: Duration = Duration::from_secs(30);

/// Replies waiting for one connection's writer: a client that reads them has at most a few
/// outstanding; one that stops reading fills this and is severed instead of growing it.
pub const OUTBOX: usize = 64;

/// The frames waiting for the host: enough for a burst from a few clients; a full channel holds
/// the reader tasks back (backpressure on the sockets), never drops a frame.
const EVENTS: usize = 256;

enum Event {
    Hello(Call, Box<ConnReader>, ConnWriter),
    Frame(ConnId, Incoming),
    WriteFailed(ConnId),
}

/// The loop's tasks: readers and handshakes (cancelled at the end) and writers (drained at the
/// end, so a `jfl/stop` answer is written), with each connection's handles for its severing.
struct Tasks {
    readers: JoinSet<()>,
    writers: JoinSet<()>,
    of: BTreeMap<ConnId, [AbortHandle; 2]>,
}

impl Tasks {
    fn end(&mut self, conn: ConnId) {
        if let Some(handles) = self.of.remove(&conn) {
            for handle in handles {
                handle.abort();
            }
        }
    }
}

/// Why the loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// SIGTERM.
    Terminated,
    /// A client asked (`jfl/stop`).
    Stopped,
    /// No connection for the idle period.
    Idle,
    /// The lock path no longer names the locked file.
    LockLost,
    /// The socket path no longer names the bound socket.
    SocketLost,
}

/// How long a shutdown waits for the proxies' spawn lock before it stops without it: a proxy
/// holds the lock at most for its spawn wait ([`super::proxy::Budget`]'s `spawn`, 10 s by
/// default) while it runs, plus a margin. A holder that is suspended or wedged, or another process
/// that locks the file, delays the shutdown by this much, never stops it (FND-18's review). Pinned
/// by `tests/daemon.rs::should_exit_once_its_wait_for_the_spawn_lock_expires_when_the_holder_never_lets_go`.
pub const SPAWN_LOCK_WAIT: Duration = Duration::from_secs(15);

/// A condition the shutdown handled that deserves a look in volume (fnd §5, WARN): no tracing
/// subscriber is installed yet, so `jfl daemon` writes [`Warning::event`] to its log (stderr).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// The wait for the spawn lock reached [`SPAWN_LOCK_WAIT`]: the shutdown went on without it.
    SpawnLockWaitExpired,
    /// A second SIGTERM ended the wait for the spawn lock: the shutdown went on without it.
    SpawnLockWaitInterrupted,
    /// The spawn lock could not be opened: the shutdown went on without it.
    SpawnLockUnavailable,
}

impl Warning {
    /// The event's fixed `snake_case` name.
    #[must_use]
    pub const fn event(self) -> &'static str {
        match self {
            Self::SpawnLockWaitExpired => "spawn_lock_wait_expired",
            Self::SpawnLockWaitInterrupted => "spawn_lock_wait_interrupted",
            Self::SpawnLockUnavailable => "spawn_lock_unavailable",
        }
    }
}

/// How the daemon stopped (exit 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped {
    /// Why.
    pub ended: Ended,
    /// What the shutdown handled on its way, if anything.
    pub warning: Option<Warning>,
}

/// Runs the daemon of `config` until it shuts down: `Ok` with why it stopped (exit 0), or why it
/// could not start or stop cleanly (exit 1; the reason was reported on the notify socket too, for a
/// proxy that spawned it). `make` builds the request service once the lock is held, inside the
/// runtime (its blocking pool is the adapters'), with the checked state directory (where the
/// views' summary store lives, IDX-09); a service that cannot be built fails the start.
pub fn run<S: Service>(
    config: &DaemonConfig,
    make: impl FnOnce(&StateDir) -> Result<S, String>,
) -> Result<Stopped, String> {
    // The first act, before the lock: a signal to the launcher's group can then only reach a
    // daemon that holds nothing yet (obligation 3). EPERM: already a session leader.
    let _leader = rustix::process::setsid();
    let fail = |reason: String| {
        if let Some(path) = &config.notify {
            let _ignored = notify::send(
                path,
                &Notice::Failed {
                    reason: reason.clone(),
                },
            );
        }
        Err(reason)
    };
    let state = match StateDir::open(&config.state_dir) {
        Ok(state) => state,
        Err(e) => return fail(e.to_string()),
    };
    let socket = match SocketPath::of_state(&state) {
        Ok(socket) => socket,
        Err(e) => return fail(e.to_string()),
    };
    let rt = match super::ports::runtime() {
        Ok(rt) => rt,
        Err(e) => return fail(format!("cannot start the runtime: {e}")),
    };
    let mut host = Host::new(HostConfig {
        state: state.clone(),
        socket,
        uid: current_uid(),
        notify: config.notify.clone(),
    });
    let result = rt.block_on(async {
        let sigterm = match signal(SignalKind::terminate()) {
            Ok(sigterm) => sigterm,
            Err(e) => return fail(format!("cannot handle SIGTERM: {e}")),
        };
        // The host reports its own failures on the notify socket.
        host.lock().map_err(|e| e.to_string())?;
        let mut service = match make(&state) {
            Ok(service) => service,
            Err(e) => return fail(e),
        };
        host.unlink_stale().map_err(|e| e.to_string())?;
        host.listen().map_err(|e| e.to_string())?;
        let (ended, warning, mut writers) =
            serve(&mut host, &mut service, sigterm, config.idle).await;
        // Nothing awaits between the stop (in `serve`) and the release, so no writer ends its
        // connection before the release; they drain what was queued (a `jfl/stop` answer) now,
        // within the write deadline.
        let shutdown = host.shutdown();
        let _drained = timer::within(WRITE_DEADLINE, async {
            while writers.join_next().await.is_some() {}
        })
        .await;
        writers.shutdown().await;
        shutdown.map_err(|e| e.to_string())?;
        Ok(Stopped { ended, warning })
    });
    // Not a plain drop, which waits for every blocking task: a wait for the spawn lock that
    // expired is still blocked in `flock(2)` there, for as long as its holder keeps it.
    rt.shutdown_background();
    result
}

/// Serves until a reason to stop, then takes the spawn lock and stops accepting: returns why,
/// what the shutdown handled, and the connections' writers still to drain, for the caller's
/// unlink and release.
async fn serve(
    host: &mut Host,
    service: &mut dyn Service,
    mut sigterm: tokio::signal::unix::Signal,
    idle: Duration,
) -> (Ended, Option<Warning>, JoinSet<()>) {
    let Some(listener) = host.take_listener() else {
        return (Ended::Stopped, None, JoinSet::new());
    };
    let (events, mut inbox) = mpsc::channel::<Event>(EVENTS);
    let mut tasks = Tasks {
        readers: JoinSet::new(),
        writers: JoinSet::new(),
        of: BTreeMap::new(),
    };
    let mut tick = timer::ticker(TICK.min(idle.max(Duration::from_millis(10))));
    let mut idle_since = Some(Instant::now());
    let mut accepting = true;
    let mut compiled = host.take_compiled();
    // Whether the service may have background work: set by every frame handled, cleared when it
    // says it has none.
    let mut working = true;
    let ended = loop {
        tokio::select! {
            biased;
            _ = sigterm.recv() => break Ended::Terminated,
            accepted = listener.accept(), if accepting => {
                accepting = accept(accepted, host, &events, &mut tasks);
            }
            Some(event) = inbox.recv() => {
                let ended = handle(event, host, service, &events, &mut tasks).await;
                working = true;
                for conn in host.take_severed() {
                    tasks.end(conn);
                }
                if let Some(ended) = ended {
                    break ended;
                }
                if host.idle() {
                    idle_since.get_or_insert_with(Instant::now);
                } else {
                    idle_since = None;
                }
            }
            Some(done) = next_compiled(compiled.as_mut()) => {
                host.diagnosed(done).await;
                for conn in host.take_severed() {
                    tasks.end(conn);
                }
            }
            _ = tick.tick() => {
                accepting = true;
                host.observe_disk();
                match host.tick() {
                    Ok(Tick::Fine) => {}
                    Ok(Tick::LockLost) | Err(_) => break Ended::LockLost,
                    Ok(Tick::SocketLost) => break Ended::SocketLost,
                }
                if idle_since.is_some_and(|since| since.elapsed() >= idle) {
                    break Ended::Idle;
                }
            }
            () = std::future::ready(()), if working => {
                match service.background() {
                    Some(step) => step.await,
                    None => working = false,
                }
            }
        }
    };
    // The proxies' spawn lock first, the listener still open (a connect meanwhile waits in its
    // backlog, then is reset). A running proxy holds it at most for its spawn wait, but a
    // suspended one, or any process that locks the file, for ever: so the wait has a deadline,
    // and a second SIGTERM ends it too; either way the shutdown goes on without it, as before
    // FND-18 (wasteful, never unsafe). An expired wait stays blocked on the blocking pool until
    // the holder lets go, then drops the lock at once (its output has no reader); `run` does not
    // wait for it at exit. The deadline also ends the one wait cycle through the socket: a proxy
    // holding the spawn lock whose second check connects into this daemon's full backlog.
    let path = host.config().state.spawn_lock_path();
    let waited = tokio::select! {
        biased;
        acquired = timer::within(
            SPAWN_LOCK_WAIT,
            tokio::task::spawn_blocking(move || Flock::acquire(&path)),
        ) => match acquired {
            Some(Ok(Ok(lock))) => Ok(lock),
            Some(_) => Err(Warning::SpawnLockUnavailable),
            None => Err(Warning::SpawnLockWaitExpired),
        },
        _ = sigterm.recv() => Err(Warning::SpawnLockWaitInterrupted),
    };
    let warning = match waited {
        Ok(lock) => {
            host.hold_spawn_lock(lock);
            None
        }
        Err(warning) => Some(warning),
    };
    drop(listener);
    // Readers and handshakes are cancelled and joined; the stop closes every connection, which
    // ends each outbox. The writers are drained by the caller, after the release.
    tasks.readers.shutdown().await;
    if host.phase() == Phase::Serving {
        let _stopped = if host.holds_spawn_lock() {
            host.stop()
        } else {
            host.stop_alone()
        };
    }
    (ended, warning, tasks.writers)
}

/// The next finished compile of pushed diagnostics ([`super::diagnostics`]); never, without the
/// channel.
async fn next_compiled(
    compiled: Option<&mut mpsc::Receiver<super::diagnostics::Compiled>>,
) -> Option<super::diagnostics::Compiled> {
    match compiled {
        Some(compiled) => compiled.recv().await,
        None => std::future::pending().await,
    }
}

/// Takes one accepted connection; `false` pauses accepting until the next tick (a failed accept,
/// such as `EMFILE`, would fail again at once).
fn accept(
    accepted: std::io::Result<Accepted>,
    host: &mut Host,
    events: &mpsc::Sender<Event>,
    tasks: &mut Tasks,
) -> bool {
    match accepted {
        Ok(Accepted::Peer(mut reader, writer)) => {
            let events = events.clone();
            tasks.readers.spawn(async move {
                let first = timer::within(HANDSHAKE_DEADLINE, reader.next()).await;
                if let Some(Incoming::Call(call)) = first
                    && call.method == Handshake::NAME
                {
                    let _closed = events.send(Event::Hello(call, reader, writer)).await;
                }
            });
            true
        }
        Ok(Accepted::Refused { .. }) => {
            host.count_refused();
            true
        }
        Err(_) => false,
    }
}

async fn handle(
    event: Event,
    host: &mut Host,
    service: &mut dyn Service,
    events: &mpsc::Sender<Event>,
    tasks: &mut Tasks,
) -> Option<Ended> {
    match event {
        Event::Hello(call, reader, writer) => {
            let (queue, outbox) = mpsc::channel::<Vec<u8>>(OUTBOX);
            if let Ok(conn) = host.open_with(call, None, Outbox::Queue(queue)).await {
                let reader_events = events.clone();
                let mut reader = reader;
                let read = tasks.readers.spawn(async move {
                    loop {
                        let incoming = reader.next().await;
                        let end = matches!(incoming, Incoming::End(_));
                        if reader_events
                            .send(Event::Frame(conn, incoming))
                            .await
                            .is_err()
                            || end
                        {
                            break;
                        }
                    }
                });
                let write_events = events.clone();
                let write = tasks
                    .writers
                    .spawn(write_all(conn, writer, outbox, write_events));
                tasks.of.insert(conn, [read, write]);
            }
            None
        }
        Event::WriteFailed(conn) => {
            host.sever(conn);
            None
        }
        Event::Frame(conn, incoming) => {
            match host.apply(conn, incoming).await {
                Ok(Received::Request) => {
                    while host.has_request(conn) {
                        match host.answer(conn, service).await {
                            Ok(true) => return Some(Ended::Stopped),
                            Ok(false) => {}
                            Err(_) => break,
                        }
                    }
                }
                Ok(Received::Closed | Received::Applied | Received::Refused) | Err(_) => {}
            }
            if host.is_peer_closed(conn) && host.phase() == Phase::Serving {
                let _gone = host.reap(conn);
                // Its writer ends with its outbox; its reader has ended at the EOF.
                tasks.of.remove(&conn);
            }
            None
        }
    }
}

/// A connection's writer: each queued reply written whole within [`WRITE_DEADLINE`]. A write
/// that fails or expires ends the task (dropping the write half) and asks the loop to sever the
/// connection; the outbox's end (the connection dropped) ends it too.
async fn write_all(
    conn: ConnId,
    mut writer: ConnWriter,
    mut outbox: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<Event>,
) {
    while let Some(body) = outbox.recv().await {
        let written = timer::within(WRITE_DEADLINE, writer.send(&body)).await;
        if !matches!(written, Some(Ok(()))) {
            let _closed = events.send(Event::WriteFailed(conn)).await;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_outlast_a_proxys_spawn_wait_when_a_shutdown_waits_for_the_spawn_lock() {
        // given
        let spawn = super::super::proxy::Budget::default().spawn;

        // when
        let margin = SPAWN_LOCK_WAIT.checked_sub(spawn);

        // then: a running proxy's hold ends before the shutdown gives up on it
        assert!(
            margin.is_some_and(|m| m >= Duration::from_secs(5)),
            "{margin:?}"
        );
    }
}
