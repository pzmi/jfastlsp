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
            self.proxy_with(
                name,
                idle,
                Budget {
                    reply: Duration::from_secs(10),
                    spawn: Duration::from_secs(20),
                },
            )
        }

        /// A proxy named `name` with the waits' `budget`, whose spawns are recorded here.
        pub(crate) fn proxy_with(
            &self,
            name: &str,
            idle: Option<Duration>,
            budget: Budget,
        ) -> Proxy<Recording> {
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
                budget,
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

        /// The spawn lock, taken by the test. A proxy's spawn released it; under `cargo test` a
        /// child another test thread starts may hold a copy until its exec, so this waits for it
        /// as a proxy would.
        pub(crate) fn spawn_lock(&self) -> jfl_adapter_ipc::Flock {
            let mut lock = None;
            jfl_testkit::poll::wait_until(Duration::from_secs(20), "the spawn lock", || {
                lock = jfl_adapter_ipc::Flock::try_acquire(&self.state.spawn_lock_path()).ok();
                lock.is_some()
            })
            .unwrap();
            lock.expect("the wait ended on the lock")
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

    /// Whether a process waits blocked in `flock(2)` for the lock file at `path`: `/proc/locks`
    /// lists a blocked waiter as `-> FLOCK` with the file's inode.
    #[cfg(target_os = "linux")]
    pub(crate) fn lock_waiter_blocked(path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        let Ok(ino) = fs::metadata(path).map(|m| m.ino()) else {
            return false;
        };
        let suffix = format!(":{ino}");
        fs::read_to_string("/proc/locks").is_ok_and(|locks| {
            locks.lines().any(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                fields.get(1) == Some(&"->")
                    && fields.get(2) == Some(&"FLOCK")
                    && fields.get(6).is_some_and(|id| id.ends_with(&suffix))
            })
        })
    }

    /// Whether a process waits blocked for the lock file at `path`: not observable here, so the
    /// test that asks releases the lock at once (the wake itself is pinned by `jfl-adapter-ipc`'s
    /// `lock::tests`).
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn lock_waiter_blocked(_path: &Path) -> bool {
        true
    }

    /// Whether a process waits blocked for the lock file at `path` now: `false` where it cannot
    /// be seen (the test then relies on the lock alone).
    #[cfg(target_os = "linux")]
    pub(crate) fn lock_waiter_blocked_now(path: &Path) -> bool {
        lock_waiter_blocked(path)
    }

    /// Whether a process waits blocked for the lock file at `path` now: `false` where it cannot
    /// be seen (the test then relies on the lock alone).
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn lock_waiter_blocked_now(_path: &Path) -> bool {
        false
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
fn should_wait_for_a_stopping_daemon_and_start_one_that_serves_when_a_proxy_finds_none() {
    // given: a host that took its shutdown's first step: it no longer accepts, and it still holds
    // the lock (FND-18's window, held here as long as the test likes)
    use jfl_composition::daemon::host::HostConfig;
    use jfl_composition::daemon::proxy::ProxyPhase;
    use jfl_composition::daemon::stepped::SteppedHost;
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
    host.stop().unwrap();
    // A child another test thread of this process starts holds a copy of each of its descriptors
    // until its exec (close-on-exec), the closed listener's too (under `cargo test`; nextest runs
    // each test in a process of its own): wait until nothing listens at the path.
    wait_until(
        Duration::from_secs(20),
        "the stopped listener's close",
        || !jfl_adapter_ipc::client::listening(daemons.socket.path()),
    )
    .unwrap();
    let mut proxy = daemons.proxy("a", None);

    // when: a proxy finds no daemon while it stops, then asks again once it has released its lock
    let refused = proxy.connect().unwrap();
    let during = proxy.take_spawn_lock().unwrap();
    let spawned_during = daemons.spawned().len();
    host.unlink().unwrap();
    host.release().unwrap();
    let served = proxy.call::<Status>(StatusParams { worktree: None }, BUDGET);

    // then: nothing was spawned while the lock was held (before FND-18 the proxy took the spawn
    // lock here and its daemon failed the lock); the daemon spawned after serves
    assert_eq!(
        (refused, during),
        (ProxyPhase::NoDaemon, ProxyPhase::Disconnected)
    );
    assert_eq!(spawned_during, 0);
    assert_eq!(
        served.map(|r| pid_of(&r)).ok(),
        daemons.spawned().last().copied()
    );
}

#[test]
fn should_end_the_call_at_its_deadline_naming_the_spawn_lock_when_its_holder_never_lets_go() {
    // given: no daemon, and the spawn lock held by someone who never lets go (a spawner
    // suspended mid-spawn); a call with a short budget
    use jfl_composition::daemon::proxy::{ProxyError, ProxyPhase};
    let daemons = Daemons::new();
    let held = daemons.spawn_lock();

    // when: a proxy calls on a thread of its own, the test waiting for its end
    let (ended, phase, waited) = std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let daemons = &daemons;
        scope.spawn(move || {
            let mut proxy = daemons.proxy("held", None);
            let call =
                proxy.call::<Status>(StatusParams { worktree: None }, Duration::from_secs(2));
            let _sent = tx.send((call, proxy.phase()));
        });
        let mut got = None;
        let waited = wait_until(Duration::from_secs(20), "the call's end", || {
            got = rx.try_recv().ok();
            got.is_some()
        });
        // A proxy waiting without bound (before IDX-12b) goes on once the lock is free.
        drop(held);
        let (call, phase) = got.unwrap_or_else(|| rx.recv().unwrap());
        (call, phase, waited)
    });

    // then: the call ended at its deadline (within the test's 20 s for a 2 s budget; before
    // IDX-12b it waited for the lock for ever) with the lock file named, nothing spawned
    waited.unwrap();
    assert!(
        matches!(
            &ended,
            Err(ProxyError::Lock(jfl_adapter_ipc::LockError::Held { path }))
                if *path == daemons.state.spawn_lock_path()
        ),
        "{ended:?}"
    );
    assert_eq!(phase, ProxyPhase::Disconnected);
    assert!(daemons.spawned().is_empty(), "{:?}", daemons.spawned());
}

#[test]
fn should_hold_no_spawn_lock_while_idle_when_its_call_ended_on_a_held_one() {
    // given: a call that ended at its deadline while another process held the spawn lock, its
    // proxy kept (as `jfl lsp` keeps its own between requests) and asking nothing more
    use jfl_composition::daemon::proxy::ProxyError;
    let daemons = Daemons::new();
    let path = daemons.state.spawn_lock_path();
    let held = daemons.spawn_lock();
    let mut proxy = daemons.proxy("idle", None);
    let ended = proxy.call::<Status>(StatusParams { worktree: None }, Duration::from_secs(2));

    // when: the holder lets go while the proxy is idle; another descriptor asks for the lock,
    // twice, each time once no waiter is blocked on it (so a thread of the proxy that was woken
    // has taken it by then, or never will)
    drop(held);
    let free = || {
        wait_until(
            Duration::from_secs(20),
            "the spawn lock free while the proxy is idle",
            || {
                !given::lock_waiter_blocked_now(&path)
                    && jfl_adapter_ipc::Flock::try_acquire(&path).is_ok()
            },
        )
    };
    let first = free();
    let second = free();

    // then: the lock is free both times (before IDX-12b's review, the proxy's wait outlived the
    // call, took the lock once its holder let go and kept it until the proxy's next call)
    assert!(
        matches!(
            &ended,
            Err(ProxyError::Lock(jfl_adapter_ipc::LockError::Held { .. }))
        ),
        "{ended:?}"
    );
    first.unwrap();
    second.unwrap();
    drop(proxy);
}

#[test]
fn should_spawn_and_serve_when_the_holder_of_the_spawn_lock_lets_go_during_the_wait() {
    // given: no daemon, the spawn lock held, and a proxy whose spawn budget outlasts its call, so
    // its one wait for the lock ends with the call unless the release wakes it
    use jfl_composition::daemon::proxy::Budget;
    let daemons = Daemons::new();
    let held = daemons.spawn_lock();
    let budget = Budget {
        reply: Duration::from_secs(10),
        spawn: Duration::from_secs(120),
    };

    // when: the proxy calls on a thread of its own; once it waits for the lock, the holder lets go
    let served = std::thread::scope(|scope| {
        let daemons = &daemons;
        let caller = scope.spawn(move || {
            let mut proxy = daemons.proxy_with("woken", None, budget);
            proxy.call::<Status>(StatusParams { worktree: None }, BUDGET)
        });
        let blocked = wait_until(
            Duration::from_secs(20),
            "a waiter blocked on the spawn lock",
            || given::lock_waiter_blocked(&daemons.state.spawn_lock_path()),
        );
        drop(held);
        blocked.unwrap();
        caller.join().unwrap()
    });

    // then: the release woke the wait (else it would have run to the call's end), and the daemon
    // spawned then serves
    assert_eq!(
        served.map(|r| pid_of(&r)).ok(),
        daemons.spawned().last().copied()
    );
    assert_eq!(daemons.spawned().len(), 1, "{:?}", daemons.spawned());
}

#[test]
fn should_keep_its_listener_until_it_holds_the_spawn_lock_and_release_both_before_the_end_when_stopped()
 {
    // given: a serving daemon, a client past its handshake, and the spawn lock held, as a proxy
    // holds it while it spawns
    use jfl_adapter_ipc::{ClientConn, Flock, Received};
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let mut client = ClientConn::connect(daemons.socket.path()).unwrap();
    client
        .send(&frames::hello("c", 1), &mut frames::plenty())
        .unwrap();
    let hello = client.recv(&mut frames::plenty()).unwrap();
    let spawning = daemons.spawn_lock();

    // when: the client stops the daemon and reads its answer (written while the daemon waits for
    // the spawn lock); another client connects; then the spawn lock is let go and the first client
    // reads its connection's end
    client
        .send(&frames::stop(2), &mut frames::plenty())
        .unwrap();
    let answer = client.recv(&mut frames::plenty()).unwrap();
    let late = ClientConn::connect(daemons.socket.path());
    drop(spawning);
    let end = client.recv(&mut frames::plenty()).unwrap();
    let released_at_end = (
        daemons.socket.path().exists(),
        daemons.lock_free(),
        Flock::try_acquire(&daemons.state.spawn_lock_path()).is_ok(),
    );
    let exit = daemons.wait_exit(pid, Duration::from_secs(20));

    // then: while the stop waited for the spawn lock the listener still queued connections (before
    // FND-18 it was closed before the answer was written: the late connect was refused), and by
    // its end the daemon had removed its socket and released both locks
    assert!(matches!(hello, Received::Frame(_)));
    match answer {
        Received::Frame(body) => assert!(matches!(
            jfl_protocol::codec::decode_response::<Stop>(&body)
                .unwrap()
                .outcome,
            Outcome::Result(_)
        )),
        Received::Eof => panic!("no answer to jfl/stop"),
    }
    assert!(late.is_ok(), "{:?}", late.err());
    assert_eq!(end, Received::Eof);
    assert_eq!(released_at_end, (false, true, true));
    assert_eq!(exit, Ok(jfl_process::Exit::Code(0)));
}

#[test]
fn should_exit_once_its_wait_for_the_spawn_lock_expires_when_the_holder_never_lets_go() {
    // given: a serving daemon, and the spawn lock held for good (a suspended proxy's, or any
    // process that locks the file)
    use jfl_composition::daemon::run::SPAWN_LOCK_WAIT;
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let holder = daemons.spawn_lock();

    // when
    let signalled = std::time::Instant::now();
    assert!(jfl_testkit::signal::send(pid, "TERM"));
    let exit = daemons.wait_exit(pid, SPAWN_LOCK_WAIT + Duration::from_secs(20));
    let waited = signalled.elapsed();

    // then: the held spawn lock delayed the exit by the whole wait, and did not prevent it: exit
    // 0, its socket removed and its lock released, the spawn lock still the holder's
    assert_eq!(exit, Ok(jfl_process::Exit::Code(0)));
    assert!(waited >= SPAWN_LOCK_WAIT, "{waited:?}");
    assert!(!daemons.socket.path().exists());
    assert!(daemons.lock_free());
    drop(holder);
}

#[test]
fn should_end_its_wait_for_the_spawn_lock_when_a_second_sigterm_comes() {
    // given: a serving daemon, and the spawn lock held for good
    use jfl_composition::daemon::run::SPAWN_LOCK_WAIT;
    let daemons = Daemons::new();
    let mut proxy = daemons.proxy("a", None);
    let pid = pid_of(
        &proxy
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .unwrap(),
    );
    let holder = daemons.spawn_lock();

    // when: SIGTERM, and again at every look, until it exits; looking no longer than its wait
    // for the spawn lock would take
    let mut exit = None;
    let ended = wait_until(SPAWN_LOCK_WAIT, "the exit, a SIGTERM at every look", || {
        exit = daemons.exit_of(pid);
        if exit.is_none() {
            assert!(jfl_testkit::signal::send(pid, "TERM"));
        }
        exit.is_some()
    });

    // then: a second SIGTERM ended the wait before its deadline; the shutdown went on as usual
    assert!(ended.is_ok(), "{ended:?}");
    assert_eq!(exit, Some(jfl_process::Exit::Code(0)));
    assert!(!daemons.socket.path().exists());
    assert!(daemons.lock_free());
    drop(holder);
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

#[test]
fn should_leave_the_deferred_files_to_the_slices_when_a_commit_in_a_linked_worktree_comes_during_the_settle()
 {
    // given: a linked worktree (`.claude/worktrees/lt`, whose `HEAD` and index live outside its
    // root, so a commit there raises no watcher event) of more files than an open reads whole:
    // the request's own package (two files) and 1,100 elsewhere, which the open defers and the
    // settle reads in slices of `SETTLE_SLICE` (512): three slices. The test daemon holds its
    // background work while `<state>/test-hold-settle` exists, so the commit's request comes
    // before any slice every time
    use jfl_composition::daemon::views::SETTLE_SLICE;
    use jfl_protocol::messages::{Definition, PositionParams};
    const DEFERRED: usize = 1100;
    let daemons = Daemons::new();
    let hold = daemons.state.path().join("test-hold-settle");
    fs::write(&hold, b"").unwrap();
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "a/Main.java".to_owned(),
            b"package a;\nclass Main { Other other; }\n".to_vec(),
        ),
        (
            "a/Other.java".to_owned(),
            b"package a;\nclass Other {}\n".to_vec(),
        ),
    ];
    for n in 0..DEFERRED {
        files.push((
            format!("b/p{}/C{n}.java", n % 10),
            format!("package b;\nclass C{n} {{}}\n").into_bytes(),
        ));
    }
    let borrowed: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let main =
        jfl_testkit::fixtures::git::Repo::init(&daemons.root().join("m"), &borrowed).unwrap();
    let linked = main.add_agent_worktree("lt").unwrap();
    let at = PositionParams {
        uri: DocumentUri::from(
            jfl_domain::FileUri::from_path(&linked.root().join("a/Main.java").to_string_lossy())
                .unwrap(),
        ),
        line: 1,
        col: 6,
        position_encoding: jfl_domain::PositionEncoding::Utf16,
        clamp: false,
    };
    let mut proxy = daemons.proxy("rescan", None);
    let mut ask = || match proxy
        .call::<Definition>(at.clone(), BUDGET)
        .unwrap()
        .outcome
    {
        Outcome::Result(found) => (
            found.view.generation,
            found.completeness.complete(),
            found.view.head.map(|h| h.to_string()),
        ),
        Outcome::Error(e) => panic!("not answered: {e:?}"),
    };

    // when: the first request (on a declaration's own name: exact, so only the view can make it
    // incomplete) opens the view; a commit lands in the linked worktree, and the next request's
    // stat finds it (the rescan); then the settle is let go and requests are asked until one is
    // complete
    let (opened, first_complete, _) = ask();
    linked
        .git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--message",
            "during the settle",
        ])
        .unwrap();
    let head = linked
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_owned();
    let after_commit = ask();
    fs::remove_file(&hold).unwrap();
    let mut seen = Vec::new();
    let waited = wait_until(Duration::from_secs(60), "a complete answer", || {
        let (generation, complete, _) = ask();
        seen.push((generation, complete));
        complete
    });
    seen.dedup();
    waited.map_err(|e| format!("{e:?}: {seen:?}")).unwrap();

    // then: the request that found the commit published the rescan's generation alone, from the
    // new HEAD, and was incomplete with every deferred file left (before IDX-09e its rescan read
    // them all inline and answered complete); every slice was then published after it, so the
    // view was complete only past the open's generation, the rescan's and three slices' (the
    // commit's own `.git` event may set one more rescan, which leaves the files again, before or
    // between the slices: IDX-09e's scenario, its timing the watcher's); every answer before that
    // was incomplete
    let slices = u64::try_from(DEFERRED.div_ceil(SETTLE_SLICE)).unwrap();
    assert_eq!(slices, 3);
    assert!(!first_complete);
    assert_eq!(
        after_commit,
        (opened + 1, false, Some(head)),
        "opened at {opened}: {seen:?}"
    );
    let (last, _) = seen.last().copied().unwrap();
    assert!(last >= opened + 1 + slices, "{seen:?}");
    assert!(
        seen.iter().all(|(g, complete)| *complete == (*g == last)),
        "{seen:?}"
    );
}

#[test]
fn should_remove_a_notify_socket_left_by_a_dead_spawner_when_it_spawns() {
    // given: a spawner's notify socket left behind (its proxy died while it waited: its listener
    // never dropped), and a file of another shape
    let daemons = Daemons::new();
    let stale = daemons.state.notify_path(0x00de_ad00_beef);
    std::mem::forget(NotifyListener::bind(&stale).unwrap());
    let other = daemons.state.path().join("n-not-a-token");
    fs::write(&other, b"").unwrap();
    let mut proxy = daemons.proxy("sweeper", None);

    // when: a proxy finds no daemon and spawns one
    let served = proxy.call::<Status>(StatusParams { worktree: None }, BUDGET);

    // then: the dead spawner's socket is gone; nothing else was touched
    assert!(served.is_ok(), "{served:?}");
    assert!(!stale.exists());
    assert!(other.exists());
}

#[test]
fn should_reset_the_log_to_0600_and_empty_it_when_it_grew_past_its_cap() {
    // given: a log left wider than 0600 and past its cap
    use jfl_composition::daemon::LOG_CAP;
    let daemons = Daemons::new();
    let log = daemons.state.log_path();
    fs::write(&log, vec![b'x'; usize::try_from(LOG_CAP).unwrap() + 1]).unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).unwrap();
    let mut proxy = daemons.proxy("log", None);

    // when
    let served = proxy.call::<Status>(StatusParams { worktree: None }, BUDGET);

    // then: 0600, and emptied before the daemon wrote to it (it writes nothing on a healthy start)
    assert!(served.is_ok(), "{served:?}");
    let meta = fs::metadata(&log).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
    assert!(meta.len() < LOG_CAP, "{} bytes", meta.len());
}

#[test]
fn should_keep_the_log_when_it_is_under_its_cap() {
    // given: a log under its cap
    let daemons = Daemons::new();
    let log = daemons.state.log_path();
    fs::write(&log, b"an earlier daemon's warning\n").unwrap();
    let mut proxy = daemons.proxy("log", None);

    // when
    let served = proxy.call::<Status>(StatusParams { worktree: None }, BUDGET);

    // then
    assert!(served.is_ok(), "{served:?}");
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .starts_with("an earlier daemon's warning\n")
    );
}
