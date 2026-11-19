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
    let stopped = wait_until(Duration::from_secs(20), "the shutdown", || {
        !daemons.socket.path().exists() && daemons.lock_free()
    });

    // then
    assert!(severed, "the daemon kept a client that never reads");
    assert_eq!(answered.map(|r| pid_of(&r)).ok(), Some(pid));
    assert!(stopped.is_ok(), "{stopped:?}");
    assert_eq!(daemons.exit_of(pid), Some(jfl_process::Exit::Code(0)));
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
