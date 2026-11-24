//! The daemon as a process (ver §2, integration): spawned on demand through `jfl-process`'s
//! detached mode by real proxies, in a scratch state directory. Two clients racing start one
//! daemon; a second daemon on the same state directory exits 1 with the lock named; a client
//! reconnects to a new daemon after a crash; SIGTERM, `jfl/stop` and the idle period each leave no
//! socket and no lock; the modes are `0700` and `0600`.
//!
//! Every daemon a test starts is killed by its guard ([`given::Daemons`]), whatever the outcome.
//! A crash's ghost-overlay half (the new daemon holds no overlay of a dead connection) is the
//! daemon driver's `daemon-crash-with-open-documents` trace, on the same host code in process.

#[cfg(test)]
mod given {
    use std::fs;
    use std::io;
    use std::os::fd::{BorrowedFd, OwnedFd};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use jfl_adapter_ipc::{SocketPath, StateDir};
    use jfl_composition::daemon::proxy::{Budget, Launcher, Proxy, Spawned};
    use jfl_composition::daemon::{DetachedDaemon, ProcessLauncher};
    use jfl_protocol::messages::ClientKind;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// The test daemon binary.
    pub(crate) const PROGRAM: &str = env!("CARGO_BIN_EXE_jfl-composition-test-daemon");

    /// A daemon a test started: its handle, shared with the proxy that reaps it.
    pub(crate) type Child = Arc<Mutex<DetachedDaemon>>;

    /// A scratch state directory under the temporary directory (short, so its socket paths fit
    /// `sun_path` with room to spare), and every daemon started in it: on drop, each one still
    /// running (unreaped, so its pid cannot have been reused) is killed, and the directory removed.
    pub(crate) struct Daemons {
        root: PathBuf,
        pub(crate) state: StateDir,
        pub(crate) socket: SocketPath,
        children: Arc<Mutex<Vec<(u32, Child)>>>,
    }

    impl Daemons {
        pub(crate) fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("jfl-daemon-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let daemons = Self {
                state: StateDir::open(&root.join("s")).unwrap(),
                socket: SocketPath::new(&root.join("s").join("sock")).unwrap(),
                root,
                children: Arc::new(Mutex::new(Vec::new())),
            };
            SocketPath::of_state(&daemons.state).unwrap();
            daemons
        }

        /// A proxy named `name` whose spawns are recorded here.
        pub(crate) fn proxy(&self, name: &str, idle: Option<Duration>) -> Proxy<Recording> {
            let mut launcher = ProcessLauncher::new(Path::new(PROGRAM), self.state.clone());
            if let Some(idle) = idle {
                launcher = launcher.with_idle(idle);
            }
            Proxy::new(
                name,
                ClientKind::Cli,
                self.state.clone(),
                self.socket.clone(),
                Recording {
                    launcher,
                    children: Arc::clone(&self.children),
                },
                Budget {
                    reply: Duration::from_secs(10),
                    spawn: Duration::from_secs(20),
                },
            )
        }

        /// The scratch directory the state directory is in.
        pub(crate) fn root(&self) -> &Path {
            &self.root
        }

        /// The pids spawned so far.
        pub(crate) fn spawned(&self) -> Vec<u32> {
            self.children
                .lock()
                .unwrap()
                .iter()
                .map(|(pid, _)| *pid)
                .collect()
        }

        /// Starts a daemon directly (no proxy); its handle reads its exit.
        pub(crate) fn spawn(&self, notify: &Path) -> io::Result<Child> {
            let mut launcher = ProcessLauncher::new(Path::new(PROGRAM), self.state.clone());
            let child = launcher.spawn(notify)?;
            let id = child.id();
            let child = Arc::new(Mutex::new(child));
            self.children.lock().unwrap().push((id, Arc::clone(&child)));
            Ok(child)
        }

        /// `pid`'s exit, once it has exited.
        pub(crate) fn exit_of(&self, pid: u32) -> Option<jfl_process::Exit> {
            let children = self.children.lock().unwrap();
            let (_, child) = children.iter().find(|(p, _)| *p == pid)?;
            child.lock().unwrap().try_exit().unwrap()
        }

        /// Waits for `pid`'s exit and gives it. The exit is the only signal that the daemon has
        /// exited: its shutdown removes the socket and releases the lock before it returns from
        /// `run` and the process ends (`daemon/run.rs`), so neither says it has exited. Pinned
        /// by `should_not_have_exited_when_its_socket_is_gone_and_its_lock_free_while_it_runs`.
        pub(crate) fn wait_exit(
            &self,
            pid: u32,
            deadline: Duration,
        ) -> Result<jfl_process::Exit, jfl_testkit::poll::PollTimeout> {
            let mut exit = None;
            jfl_testkit::poll::wait_until(deadline, "the daemon's exit", || {
                exit = self.exit_of(pid);
                exit.is_some()
            })?;
            Ok(exit.expect("the wait ended on an exit"))
        }

        /// Whether the lock is free: a probe takes and releases it.
        pub(crate) fn lock_free(&self) -> bool {
            jfl_adapter_ipc::Flock::try_acquire(&self.state.lock_path()).is_ok()
        }
    }

    impl Drop for Daemons {
        fn drop(&mut self) {
            for (pid, child) in self.children.lock().map(|c| c.clone()).unwrap_or_default() {
                let running = child
                    .lock()
                    .map(|mut c| matches!(c.try_exit(), Ok(None)))
                    .unwrap_or(false);
                if running {
                    let _ = jfl_testkit::signal::send(pid, "KILL");
                }
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// The daemon's pid in a status answer.
    pub(crate) fn pid_of(
        response: &jfl_protocol::rpc::Response<jfl_protocol::messages::StatusResult>,
    ) -> u32 {
        match &response.outcome {
            jfl_protocol::rpc::Outcome::Result(status) => status.daemon.pid,
            jfl_protocol::rpc::Outcome::Error(e) => panic!("status failed: {}", e.message),
        }
    }

    /// [`ProcessLauncher`], recording each spawned daemon.
    pub(crate) struct Recording {
        launcher: ProcessLauncher,
        children: Arc<Mutex<Vec<(u32, Child)>>>,
    }

    /// The proxy's side of a recorded daemon: its exit read through the shared handle, its exit
    /// descriptor a duplicate of the handle's.
    struct Shared {
        child: Child,
        exit: Option<OwnedFd>,
    }

    impl Spawned for Shared {
        fn exited(&mut self) -> bool {
            self.child
                .lock()
                .map(|mut c| !matches!(c.try_exit(), Ok(None)))
                .unwrap_or(true)
        }

        fn exit_fd(&self) -> Option<BorrowedFd<'_>> {
            use std::os::fd::AsFd;
            self.exit.as_ref().map(AsFd::as_fd)
        }
    }

    impl Launcher for Recording {
        fn launch(&mut self, notify: &Path) -> io::Result<Box<dyn Spawned>> {
            let child = self.launcher.spawn(notify)?;
            let id = child.id();
            let exit = child
                .exit_fd()
                .map(|fd| fd.try_clone_to_owned())
                .transpose()?;
            let child = Arc::new(Mutex::new(child));
            if let Ok(mut children) = self.children.lock() {
                children.push((id, Arc::clone(&child)));
            }
            Ok(Box::new(Shared { child, exit }))
        }
    }
}

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::Duration;

use given::{Daemons, pid_of};
use jfl_adapter_ipc::notify::{Notice, NotifyListener, Waited};
use jfl_protocol::DocumentUri;
use jfl_protocol::messages::{Status, StatusParams, Stop, StopParams};
use jfl_protocol::rpc::Outcome;
use jfl_testkit::poll::wait_until;

const BUDGET: Duration = Duration::from_secs(30);

#[test]
fn should_start_one_daemon_when_two_clients_race_to_spawn_it() {
    // given
    let daemons = Daemons::new();

    // when
    let pids: Vec<u32> = std::thread::scope(|scope| {
        let racers: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|name| {
                let daemons = &daemons;
                scope.spawn(move || {
                    let mut proxy = daemons.proxy(name, None);
                    proxy.call::<Status>(StatusParams { worktree: None }, BUDGET)
                })
            })
            .collect();
        racers
            .into_iter()
            .map(|racer| pid_of(&racer.join().unwrap().unwrap()))
            .collect()
    });

    // then
    assert_eq!(
        daemons.spawned().len(),
        1,
        "spawned {:?}",
        daemons.spawned()
    );
    assert_eq!(pids.first(), pids.get(1));
    assert_eq!(pids.first(), daemons.spawned().first());
    let mode = |p: &std::path::Path| fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(daemons.state.path()), 0o700);
    assert_eq!(mode(daemons.socket.path()), 0o600);
    assert_eq!(mode(&daemons.state.lock_path()), 0o600);
    assert_eq!(
        fs::metadata(daemons.state.path()).unwrap().uid(),
        jfl_adapter_ipc::current_uid()
    );
}

#[test]
fn should_exit_1_naming_the_lock_when_a_second_daemon_starts_on_the_same_state_directory() {
    // given: a daemon serving
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    proxy
        .call::<Status>(StatusParams { worktree: None }, BUDGET)
        .unwrap();
    let notify = daemons.state.notify_path(0xabc);
    let listener = NotifyListener::bind(&notify).unwrap();

    // when
    let second = daemons.spawn(&notify).unwrap();
    let notice = listener.recv(Duration::from_secs(20), None).unwrap();
    let mut exit = None;
    wait_until(Duration::from_secs(20), "the second daemon's exit", || {
        exit = second.lock().unwrap().try_exit().unwrap();
        exit.is_some()
    })
    .unwrap();

    // then
    let Waited::Notice(Notice::Failed { reason }) = notice else {
        panic!("the second daemon reported {notice:?}");
    };
    assert!(
        reason.contains("daemon.lock is held by another process"),
        "{reason}"
    );
    assert_eq!(exit, Some(jfl_process::Exit::Code(1)));
}

#[test]
fn should_reconnect_to_a_new_daemon_with_its_document_when_the_daemon_crashed() {
    // given: a client with an open document, answered by a first daemon
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    proxy.did_open(
        DocumentUri::parse("file:///w/A.java").unwrap(),
        3,
        "class A {}".to_owned(),
    );
    let first = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );

    // when: the daemon is killed (its socket file stays, stale)
    assert!(jfl_testkit::signal::send(first, "KILL"));
    wait_until(Duration::from_secs(20), "the crashed daemon's lock", || {
        daemons.lock_free()
    })
    .unwrap();
    let stale = daemons.socket.path().exists();
    let second = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );

    // then
    assert!(stale, "a crash leaves its socket file");
    assert_ne!(first, second);
    assert_eq!(daemons.spawned(), vec![first, second]);
}

#[test]
fn should_remove_the_socket_and_release_the_lock_when_the_daemon_gets_sigterm() {
    // given
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );

    // when
    assert!(jfl_testkit::signal::send(pid, "TERM"));
    let gone = wait_until(Duration::from_secs(20), "the socket's removal", || {
        !daemons.socket.path().exists()
    });

    // then
    assert!(gone.is_ok(), "{gone:?}");
    wait_until(Duration::from_secs(20), "the lock's release", || {
        daemons.lock_free()
    })
    .unwrap();
    assert!(
        daemons.state.lock_path().exists(),
        "the lock file is never removed"
    );
}

#[test]
fn should_answer_then_shut_down_when_a_client_sends_stop() {
    // given
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    proxy
        .call::<Status>(StatusParams { worktree: None }, BUDGET)
        .unwrap();

    // when
    let stopped = proxy.call::<Stop>(StopParams {}, BUDGET).unwrap();

    // then
    assert!(matches!(stopped.outcome, Outcome::Result(_)));
    wait_until(Duration::from_secs(20), "the shutdown", || {
        !daemons.socket.path().exists() && daemons.lock_free()
    })
    .unwrap();
}

#[test]
fn should_exit_when_no_client_connects_for_the_idle_period() {
    // given: a daemon with a one-second idle period, its only client gone
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", Some(Duration::from_secs(1)));
    proxy
        .call::<Status>(StatusParams { worktree: None }, BUDGET)
        .unwrap();

    // when
    proxy.quit();

    // then
    wait_until(Duration::from_secs(30), "the idle exit", || {
        !daemons.socket.path().exists() && daemons.lock_free()
    })
    .unwrap();
}

#[test]
fn should_stop_and_leave_the_new_owners_socket_when_the_lock_file_was_deleted() {
    // given: a serving host whose lock file is deleted from outside, and a second host that then
    // locks the new file and binds
    use jfl_composition::daemon::host::{HostConfig, Tick};
    use jfl_composition::daemon::stepped::SteppedHost;
    let daemons = Daemons::new();
    let config = || HostConfig {
        state: daemons.state.clone(),
        socket: daemons.socket.clone(),
        uid: jfl_adapter_ipc::current_uid(),
        notify: None,
    };
    let mut first = SteppedHost::new(config()).unwrap();
    first.lock().unwrap();
    first.listen().unwrap();
    fs::remove_file(daemons.state.lock_path()).unwrap();
    let mut second = SteppedHost::new(config()).unwrap();
    second.lock().unwrap();
    second.unlink_stale().unwrap();
    second.listen().unwrap();

    // when
    let tick = first.tick().unwrap();
    first.stop().unwrap();
    first.unlink().unwrap();
    first.release().unwrap();

    // then: the first noticed, and its shutdown removed nothing of the second's
    assert_eq!(tick, Tick::LockLost);
    assert_eq!(
        jfl_adapter_ipc::FileId::at(daemons.socket.path()).unwrap(),
        second.host().bound()
    );
    assert_eq!(second.tick().unwrap(), Tick::Fine);
}

#[test]
fn should_name_why_when_the_spawned_daemon_cannot_start() {
    // given: a state directory whose lock another process holds, and no daemon serving
    let daemons = Daemons::new();
    let holder = jfl_adapter_ipc::Flock::try_acquire(&daemons.state.lock_path()).unwrap();
    let mut proxy = daemons.proxy("a", None);

    // when
    let refused = proxy.call::<Status>(StatusParams { worktree: None }, Duration::from_secs(2));
    drop(holder);

    // then
    let error = refused.unwrap_err().to_string();
    assert!(
        error.contains("daemon.lock is held by another process"),
        "{error}"
    );
    assert!(!daemons.spawned().is_empty());
}

#[cfg(test)]
mod frames {
    use jfl_protocol::Version;
    use jfl_protocol::codec;
    use jfl_protocol::messages::{
        ClientInfo, ClientKind, Handshake, HandshakeParams, Status, StatusParams, Stop, StopParams,
    };
    use jfl_protocol::rpc::RequestId;

    pub(crate) fn hello(id: &str, major: u16) -> Vec<u8> {
        codec::encode(&codec::request::<Handshake>(
            RequestId::from(id),
            HandshakeParams {
                protocol: Version { major, minor: 0 },
                client: ClientInfo {
                    kind: ClientKind::Cli,
                    version: "test".to_owned(),
                },
            },
        ))
        .unwrap()
    }

    pub(crate) fn status(id: i32) -> Vec<u8> {
        codec::encode(&codec::request::<Status>(
            RequestId::Number(id),
            StatusParams { worktree: None },
        ))
        .unwrap()
    }

    pub(crate) fn stop(id: i32) -> Vec<u8> {
        codec::encode(&codec::request::<Stop>(
            RequestId::Number(id),
            StopParams {},
        ))
        .unwrap()
    }

    pub(crate) fn plenty() -> impl FnMut() -> Option<std::time::Duration> {
        || Some(std::time::Duration::from_secs(10))
    }
}

#[test]
fn should_keep_serving_others_and_sigterm_when_a_client_stops_reading() {
    // given: a serving daemon, and a client that floods requests and never reads an answer
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let mut slow = jfl_adapter_ipc::ClientConn::connect(daemons.socket.path()).unwrap();
    slow.send(&frames::hello("slow", 1), &mut frames::plenty())
        .unwrap();

    // when: its unread answers fill its socket, then its outbox, and the daemon severs it
    let request = frames::status(1);
    let severed = (0..200_000).any(|_| slow.send(&request, &mut frames::plenty()).is_err());
    let answered = proxy.call::<Status>(StatusParams { worktree: None }, Duration::from_secs(5));
    assert!(jfl_testkit::signal::send(pid, "TERM"));
    let exit = daemons.wait_exit(pid, Duration::from_secs(20));

    // then: it exited 0, and its shutdown, which comes before the exit, left no socket and no lock
    assert!(severed, "the daemon kept a client that never reads");
    assert_eq!(answered.map(|r| pid_of(&r)).ok(), Some(pid));
    assert_eq!(exit, Ok(jfl_process::Exit::Code(0)));
    assert!(!daemons.socket.path().exists());
    assert!(daemons.lock_free());
}

#[test]
fn should_not_have_exited_when_its_socket_is_gone_and_its_lock_free_while_it_runs() {
    // given: a serving daemon, stopped (SIGSTOP, seen by `waitid`), its socket removed and its
    // lock file replaced: the state its shutdown leaves just before it exits, held while it can
    // neither run nor exit
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let raw = Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    assert!(jfl_testkit::signal::send(pid, "STOP"));
    wait_until(Duration::from_secs(20), "the daemon's stop", || {
        let stopped = waitid(
            WaitId::Pid(raw),
            WaitIdOptions::STOPPED | WaitIdOptions::NOHANG,
        );
        matches!(stopped, Ok(Some(status)) if status.stopped())
    })
    .unwrap();
    fs::remove_file(daemons.socket.path()).unwrap();
    fs::remove_file(daemons.state.lock_path()).unwrap();

    // when: the signals the SIGTERM test used to wait on, then the exit read at once, as it did
    let signalled = !daemons.socket.path().exists() && daemons.lock_free();
    let read_at_once = daemons.exit_of(pid);
    assert!(jfl_testkit::signal::send(pid, "CONT"));
    let waited = daemons.wait_exit(pid, Duration::from_secs(20));

    // then: both signals held while it had not exited; the wait on the exit itself sees it exit
    // 0 (its next tick finds its lock path and socket gone)
    assert!(signalled);
    assert_eq!(read_at_once, None);
    assert_eq!(waited, Ok(jfl_process::Exit::Code(0)));
}

#[test]
fn should_serve_only_stop_when_the_handshake_announced_another_major() {
    // given: a host, and a client of major 2
    use jfl_adapter_ipc::{ClientConn, Received};
    use jfl_composition::daemon::host::{HostConfig, Received as Read};
    use jfl_composition::daemon::stepped::SteppedHost;
    use jfl_protocol::rpc::{ErrorCode, Outcome};
    let daemons = Daemons::new();
    let mut host = SteppedHost::new(HostConfig {
        state: daemons.state.clone(),
        socket: daemons.socket.clone(),
        uid: jfl_adapter_ipc::current_uid(),
        notify: None,
    })
    .unwrap();
    host.lock().unwrap();
    host.listen().unwrap();
    let mut client = ClientConn::connect(daemons.socket.path()).unwrap();
    client
        .send(&frames::hello("v2", 2), &mut frames::plenty())
        .unwrap();
    let error = |client: &mut ClientConn| match client.recv(&mut frames::plenty()).unwrap() {
        Received::Frame(body) => {
            match jfl_protocol::codec::decode_response::<Status>(&body)
                .unwrap()
                .outcome
            {
                Outcome::Error(e) => Some(e.code),
                Outcome::Result(_) => None,
            }
        }
        Received::Eof => None,
    };

    // when
    let conn = host
        .accept(&jfl_protocol::rpc::RequestId::from("v2"))
        .unwrap();
    let refused = error(&mut client);
    client
        .send(&frames::status(1), &mut frames::plenty())
        .unwrap();
    let status = host.receive(conn).unwrap();
    let refused_again = error(&mut client);
    client
        .send(&frames::stop(2), &mut frames::plenty())
        .unwrap();
    let stop = host.receive(conn).unwrap();
    let stops = host.answer(conn).unwrap();

    // then
    assert!(host.host().connections()[&conn].mismatched());
    assert_eq!(refused, Some(ErrorCode::VersionMismatch));
    assert_eq!(
        (status, refused_again),
        (Read::Refused, Some(ErrorCode::VersionMismatch))
    );
    assert_eq!(stop, Read::Request);
    assert!(stops, "jfl/stop is honoured on a mismatched connection");
}

#[test]
fn should_stop_the_daemon_of_another_major_and_start_its_own_when_a_proxy_connects() {
    // given: a daemon serving, and a proxy announcing major 2 (its own spawns are this build,
    // major 1: the call ends refused, after it stopped the old daemon and started one)
    let daemons = Daemons::new();
    let mut old = daemons.proxy("a", None);
    let first = pid_of(
        &old.call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let mut newer = daemons
        .proxy("b", None)
        .with_protocol(jfl_protocol::Version { major: 2, minor: 0 });

    // when
    let refused = newer.call::<Status>(StatusParams { worktree: None }, BUDGET);

    // then: the first daemon exited 0 on the newer proxy's jfl/stop, and a second was spawned
    assert!(
        matches!(
            refused,
            Err(jfl_composition::daemon::proxy::ProxyError::Incompatible)
        ),
        "{refused:?}"
    );
    wait_until(Duration::from_secs(20), "the old daemon's exit", || {
        daemons.exit_of(first).is_some()
    })
    .unwrap();
    assert_eq!(daemons.exit_of(first), Some(jfl_process::Exit::Code(0)));
    assert_eq!(daemons.spawned().len(), 2, "{:?}", daemons.spawned());
}

#[test]
fn should_answer_a_definition_from_the_views_with_an_honest_envelope_when_a_client_asks() {
    // given: the LSP fixture worktree (an astral line before every name asked about)
    use jfl_protocol::messages::{Definition, PositionParams};
    let daemons = Daemons::new();
    let root = daemons.root().join("wt");
    jfl_testkit::fixtures::git::Repo::init(
        &root,
        &[
            (
                "src/demo/Main.java",
                include_bytes!("../../../fixtures/lsp/worktree/src/demo/Main.java"),
            ),
            (
                "src/demo/greet/Greeter.java",
                include_bytes!("../../../fixtures/lsp/worktree/src/demo/greet/Greeter.java"),
            ),
        ],
    )
    .unwrap();
    let uri = |file: &str| {
        DocumentUri::from(
            jfl_domain::FileUri::from_path(&root.join(file).to_string_lossy()).unwrap(),
        )
    };
    let at = |file: &str, line, col| PositionParams {
        uri: uri(file),
        line,
        col,
        position_encoding: jfl_domain::PositionEncoding::Utf16,
        clamp: false,
    };
    let head = jfl_testkit::fixtures::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
    let mut proxy = daemons.proxy("defs", None);

    // when: a call on a receiver the syntax cannot type, the cursor on a declaration's own name,
    // whitespace, and a column past the line's end
    let call = proxy
        .call::<Definition>(at("src/demo/Main.java", 12, 68), BUDGET)
        .unwrap();
    let own = proxy
        .call::<Definition>(at("src/demo/greet/Greeter.java", 10, 29), BUDGET)
        .unwrap();
    let blank = proxy
        .call::<Definition>(at("src/demo/Main.java", 12, 2), BUDGET)
        .unwrap();
    let past = proxy
        .call::<Definition>(at("src/demo/Main.java", 12, 400), BUDGET)
        .unwrap();

    // then: `exact` only where proved (the declaration's own name), the unproved call `possible`
    // with its reason and never complete, the view naming the worktree's HEAD
    let (call, own) = match (call.outcome, own.outcome) {
        (Outcome::Result(call), Outcome::Result(own)) => (call, own),
        refused => panic!("not answered: {refused:?}"),
    };
    let wave = jfl_protocol::envelope::WireRange(
        jfl_domain::Range::new(
            jfl_domain::Position::new(10, 28),
            jfl_domain::Position::new(10, 32),
        )
        .unwrap(),
    );
    assert!(call.exact.is_empty());
    assert!(call.target.is_none());
    assert_eq!(
        call.possible
            .iter()
            .map(|p| (&p.uri, p.range))
            .collect::<Vec<_>>(),
        [(&uri("src/demo/greet/Greeter.java"), wave)]
    );
    assert!(!call.completeness.complete());
    assert!(!call.completeness.incomplete_because().is_empty());
    assert_eq!(
        call.view.head.as_ref().map(ToString::to_string),
        Some(head.trim().to_owned())
    );
    assert_eq!(
        own.exact
            .iter()
            .map(|l| (&l.uri, l.range))
            .collect::<Vec<_>>(),
        [(&uri("src/demo/greet/Greeter.java"), wave)]
    );
    assert!(matches!(
        own.target.map(|t| t.status),
        Some(jfl_protocol::envelope::WireTargetStatus::Exact)
    ));
    assert!(
        matches!(blank.outcome, Outcome::Error(e) if e.code == jfl_protocol::rpc::ErrorCode::NoSymbol)
    );
    assert!(
        matches!(past.outcome, Outcome::Error(e) if e.code == jfl_protocol::rpc::ErrorCode::PositionOutOfRange)
    );
}

#[test]
fn should_answer_in_the_request_s_spelling_when_the_worktree_is_reached_through_a_link() {
    // given: the LSP fixture worktree at `real/wt`, asked about as `via/wt` through a link to
    // `real`: macOS's temporary directory is such a spelling (`/var` is a link to `/private/var`),
    // and the repository probe names the worktree by its canonical root
    use jfl_protocol::messages::{Definition, PositionParams};
    let daemons = Daemons::new();
    let base = fs::canonicalize(daemons.root()).unwrap();
    let real = base.join("real").join("wt");
    jfl_testkit::fixtures::git::Repo::init(
        &real,
        &[
            (
                "src/demo/Main.java",
                include_bytes!("../../../fixtures/lsp/worktree/src/demo/Main.java"),
            ),
            (
                "src/demo/greet/Greeter.java",
                include_bytes!("../../../fixtures/lsp/worktree/src/demo/greet/Greeter.java"),
            ),
        ],
    )
    .unwrap();
    std::os::unix::fs::symlink(base.join("real"), base.join("via")).unwrap();
    let spelled = base.join("via").join("wt");
    let uri = |file: &str| {
        DocumentUri::from(
            jfl_domain::FileUri::from_path(&spelled.join(file).to_string_lossy()).unwrap(),
        )
    };
    let at = |file: &str, line, col| PositionParams {
        uri: uri(file),
        line,
        col,
        position_encoding: jfl_domain::PositionEncoding::Utf16,
        clamp: false,
    };
    let mut proxy = daemons.proxy("link", None);

    // when: the call the syntax cannot type, and the declaration's own name
    let call = proxy
        .call::<Definition>(at("src/demo/Main.java", 12, 68), BUDGET)
        .unwrap();
    let own = proxy
        .call::<Definition>(at("src/demo/greet/Greeter.java", 10, 29), BUDGET)
        .unwrap();

    // then: answered, the view and every hit named as the request spelled the worktree
    let (call, own) = match (call.outcome, own.outcome) {
        (Outcome::Result(call), Outcome::Result(own)) => (call, own),
        refused => panic!("not answered: {refused:?}"),
    };
    let greeter = uri("src/demo/greet/Greeter.java");
    assert_eq!(
        call.possible.iter().map(|p| &p.uri).collect::<Vec<_>>(),
        [&greeter]
    );
    assert_eq!(
        own.exact.iter().map(|l| &l.uri).collect::<Vec<_>>(),
        [&greeter]
    );
    assert!(matches!(
        own.target.map(|t| t.status),
        Some(jfl_protocol::envelope::WireTargetStatus::Exact)
    ));
    let worktree =
        DocumentUri::from(jfl_domain::FileUri::from_path(&spelled.to_string_lossy()).unwrap());
    assert_eq!(call.view.worktree, worktree);
    assert_eq!(own.view.worktree, worktree);
}

/// The worktrees and the modified document of the spelling tests (FND-17).
#[cfg(test)]
mod spelled {
    use super::{BUDGET, Daemons, DocumentUri, Outcome};

    /// The LSP fixture worktree (Main.java and Greeter.java), committed at `at`.
    pub(crate) fn lsp_fixture(at: &std::path::Path) {
        jfl_testkit::fixtures::git::Repo::init(
            at,
            &[
                (
                    "src/demo/Main.java",
                    include_bytes!("../../../fixtures/lsp/worktree/src/demo/Main.java"),
                ),
                (
                    "src/demo/greet/Greeter.java",
                    include_bytes!("../../../fixtures/lsp/worktree/src/demo/greet/Greeter.java"),
                ),
            ],
        )
        .unwrap();
    }

    /// A definition asked from `main` (Main.java, on the call whose `possible` hit is in
    /// Greeter.java) while the client holds `greeter` (Greeter.java) open with a line the disk does
    /// not have: the outcome.
    pub(crate) fn definition_with_greeter_modified(
        daemons: &Daemons,
        main: &std::path::Path,
        greeter: &std::path::Path,
    ) -> Outcome<jfl_protocol::WireEnvelope> {
        use jfl_protocol::messages::{Definition, PositionParams};
        let doc = |path: &std::path::Path| {
            DocumentUri::from(jfl_domain::FileUri::from_path(&path.to_string_lossy()).unwrap())
        };
        let text = format!(
            "// a line the disk does not have\n{}",
            include_str!("../../../fixtures/lsp/worktree/src/demo/greet/Greeter.java")
        );
        let mut proxy = daemons.proxy("modified", None);
        proxy.did_open(doc(greeter), 1, text);
        let at = PositionParams {
            uri: doc(main),
            line: 12,
            col: 68,
            position_encoding: jfl_domain::PositionEncoding::Utf16,
            clamp: false,
        };
        proxy.call::<Definition>(at, BUDGET).unwrap().outcome
    }

    pub(crate) fn content_modified(outcome: &Outcome<jfl_protocol::WireEnvelope>) -> bool {
        matches!(outcome, Outcome::Error(e) if e.code == jfl_protocol::rpc::ErrorCode::ContentModified)
    }
}
use spelled::{content_modified, definition_with_greeter_modified, lsp_fixture};

#[test]
fn should_refuse_a_hit_in_a_modified_document_when_it_was_opened_through_a_link() {
    // given: the worktree at `real/wt`, the client spelling it `via/wt` (a link above it, as
    // macOS's `/var`), Greeter.java open and modified in that spelling
    let daemons = Daemons::new();
    let base = fs::canonicalize(daemons.root()).unwrap();
    lsp_fixture(&base.join("real/wt"));
    std::os::unix::fs::symlink(base.join("real"), base.join("via")).unwrap();

    // when
    let outcome = definition_with_greeter_modified(
        &daemons,
        &base.join("via/wt/src/demo/Main.java"),
        &base.join("via/wt/src/demo/greet/Greeter.java"),
    );

    // then: the hit in the modified document is refused, never placed in the disk's text
    assert!(content_modified(&outcome), "{outcome:?}");
}

#[test]
fn should_refuse_a_hit_in_a_modified_document_when_the_request_goes_through_a_link_inside_the_worktree()
 {
    // given: as above, and the request spelled through `alias`, a link inside the worktree to
    // `src/demo`, so the request's path does not end with its file's path in the view
    let daemons = Daemons::new();
    let base = fs::canonicalize(daemons.root()).unwrap();
    lsp_fixture(&base.join("real/wt"));
    std::os::unix::fs::symlink(base.join("real"), base.join("via")).unwrap();
    std::os::unix::fs::symlink(base.join("real/wt/src/demo"), base.join("real/wt/alias")).unwrap();

    // when
    let outcome = definition_with_greeter_modified(
        &daemons,
        &base.join("via/wt/alias/Main.java"),
        &base.join("via/wt/src/demo/greet/Greeter.java"),
    );

    // then
    assert!(content_modified(&outcome), "{outcome:?}");
}

#[test]
fn should_refuse_a_hit_in_a_modified_document_when_a_link_leads_from_another_worktree() {
    // given: the request spelled as the worktree's own path, and Greeter.java open and modified as
    // spelled through `ln`, a link from another worktree into this one: no spelling of the
    // request names the document's
    let daemons = Daemons::new();
    let base = fs::canonicalize(daemons.root()).unwrap();
    lsp_fixture(&base.join("wt"));
    jfl_testkit::fixtures::git::Repo::init(
        &base.join("other"),
        &[("Other.java", b"class Other {}\n")],
    )
    .unwrap();
    std::os::unix::fs::symlink(base.join("wt"), base.join("other/ln")).unwrap();

    // when
    let outcome = definition_with_greeter_modified(
        &daemons,
        &base.join("wt/src/demo/Main.java"),
        &base.join("other/ln/src/demo/greet/Greeter.java"),
    );

    // then
    assert!(content_modified(&outcome), "{outcome:?}");
}

#[test]
fn should_answer_a_request_between_the_slices_of_a_settle_when_the_view_is_large() {
    // given: a worktree of more files than an open reads whole: the request's own package (two
    // files) and 1,100 files elsewhere, which the open defers and the settle reads in slices of
    // `SETTLE_SLICE` (512): three slices, so three generations after the open's
    use jfl_composition::daemon::views::SETTLE_SLICE;
    use jfl_protocol::messages::{Definition, PositionParams};
    let daemons = Daemons::new();
    let root = daemons.root().join("wt");
    let main = b"package a;\nclass Main { Other other; }\n".to_vec();
    let other = b"package a;\nclass Other {}\n".to_vec();
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("a/Main.java".to_owned(), main),
        ("a/Other.java".to_owned(), other),
    ];
    for n in 0..1100 {
        files.push((
            format!("b/p{}/C{n}.java", n % 10),
            format!("package b;\nclass C{n} {{}}\n").into_bytes(),
        ));
    }
    let borrowed: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    jfl_testkit::fixtures::git::Repo::init(&root, &borrowed).unwrap();
    let at = PositionParams {
        uri: DocumentUri::from(
            jfl_domain::FileUri::from_path(&root.join("a/Main.java").to_string_lossy()).unwrap(),
        ),
        line: 1,
        col: 6,
        position_encoding: jfl_domain::PositionEncoding::Utf16,
        clamp: false,
    };
    let mut proxy = daemons.proxy("slices", None);
    let mut ask = || match proxy
        .call::<Definition>(at.clone(), BUDGET)
        .unwrap()
        .outcome
    {
        Outcome::Result(found) => (found.view.generation, found.completeness.complete()),
        Outcome::Error(e) => panic!("not answered: {e:?}"),
    };

    // when: the first request (on a declaration's own name: exact, so only the view can make it
    // incomplete) opens the view; the next ones are asked until one is complete
    let (opened, first_complete) = ask();
    let mut seen = Vec::new();
    let waited = wait_until(Duration::from_secs(60), "a complete answer", || {
        let answer = ask();
        seen.push(answer);
        answer.1
    });
    seen.dedup();
    waited.map_err(|e| format!("{e:?}: {seen:?}")).unwrap();

    // then: every answer before the last slice was incomplete, and the view was complete at the
    // third generation after the open's: one per slice
    let slices = u64::try_from(1100_usize.div_ceil(SETTLE_SLICE)).unwrap();
    assert_eq!(slices, 3);
    assert!(!first_complete);
    let (last, _) = seen.last().copied().unwrap();
    assert_eq!(last, opened + slices, "{seen:?}");
    assert!(
        seen.iter().all(|(g, complete)| *complete == (*g == last)),
        "{seen:?}"
    );
}
