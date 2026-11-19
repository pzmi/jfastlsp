//! The daemon host against a reference state machine (ver §6.4; ADR 0007 as amended): generated
//! command sequences (the lock, the socket, connections from three clients, overlays, requests,
//! the shutdown steps, crashes, a swept socket, a rival daemon's lock and restarts) run on the
//! real host and real proxies, in process, with real sockets and locks in a scratch state
//! directory; after every command the host's phase, lock, socket file, binding, listener and each
//! client's connection (state, overlays, request) must equal the reference's. A command the
//! reference does not enable is skipped, so every sequence is valid and shrinks.
//!
//! It also checks what `EventuallyServed` reduces to (`doc/formal-models.md`, Liveness): once the
//! faults stop, a deterministic schedule of system moves (restart a dead daemon, lock, unlink a
//! stale socket, bind, notice a swept socket, reconnect, accept, answer, reap, finish a shutdown)
//! serves every waiting client within [`RECOVERY`] moves and comes to rest.
//!
//! Plain `proptest`, not `proptest-state-machine` (the orchestrator's decision for IDX-12: its
//! default features add 16 crates against the std-only proptest every crate here takes). Seeded
//! (ver §3): every run starts from `SEED`; a failure is shrunk and written to
//! `proptest-regressions/`, committed.

#[cfg(test)]
mod machine {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use jfl_adapter_ipc::{SocketPath, StateDir, current_uid};
    use jfl_composition::daemon::host::{ConnId, HostConfig, Phase, Received, Tick};
    use jfl_composition::daemon::proxy::{Budget, Launcher, Proxy, ProxyPhase, Spawned};
    use jfl_composition::daemon::stepped::SteppedHost;
    use jfl_protocol::DocumentUri;
    use jfl_protocol::messages::{ClientKind, Status, StatusParams};
    use proptest::prelude::*;
    use proptest::test_runner::{FileFailurePersistence, RngSeed};

    /// The fixed seed (ver §3 rule 2).
    pub(crate) const SEED: u64 = 0x1DC0_0012;
    /// The clients.
    pub(crate) const CLIENTS: u8 = 3;
    /// The documents.
    pub(crate) const DOCS: u8 = 2;
    /// The most system moves the recovery may take.
    pub(crate) const RECOVERY: usize = 60;

    pub(crate) fn config() -> ProptestConfig {
        ProptestConfig {
            cases: 256,
            rng_seed: RngSeed::Fixed(SEED),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/host_machine.txt",
            ))),
            ..ProptestConfig::default()
        }
    }

    /// One command.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Cmd {
        Lock,
        UnlinkStale,
        Listen,
        Connect(u8),
        ConnectRefused(u8),
        Accept(u8),
        Open(u8, u8),
        Close(u8, u8),
        Request(u8),
        Answer(u8),
        Quit(u8),
        Notice(u8),
        Reap(u8),
        Stop,
        IdleExit,
        Unlink,
        Release,
        Crash,
        Sweep,
        Tick,
        Restart,
        Rival,
    }

    pub(crate) fn command() -> impl Strategy<Value = Cmd> {
        let client = 0..CLIENTS;
        let doc = 0..DOCS;
        prop_oneof![
            Just(Cmd::Lock),
            Just(Cmd::UnlinkStale),
            Just(Cmd::Listen),
            client.clone().prop_map(Cmd::Connect),
            client.clone().prop_map(Cmd::ConnectRefused),
            client.clone().prop_map(Cmd::Accept),
            (client.clone(), doc.clone()).prop_map(|(c, d)| Cmd::Open(c, d)),
            (client.clone(), doc).prop_map(|(c, d)| Cmd::Close(c, d)),
            client.clone().prop_map(Cmd::Request),
            client.clone().prop_map(Cmd::Answer),
            client.clone().prop_map(Cmd::Quit),
            client.clone().prop_map(Cmd::Notice),
            client.prop_map(Cmd::Reap),
            Just(Cmd::Stop),
            Just(Cmd::IdleExit),
            Just(Cmd::Unlink),
            Just(Cmd::Release),
            Just(Cmd::Crash),
            Just(Cmd::Sweep),
            Just(Cmd::Tick),
            Just(Cmd::Restart),
            Just(Cmd::Rival),
        ]
    }

    /// A client's connection, in the reference.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub(crate) enum RConn {
        #[default]
        None,
        Queued,
        Open {
            overlays: BTreeSet<u8>,
            inflight: bool,
        },
        PeerClosed {
            overlays: BTreeSet<u8>,
        },
        Closed,
    }

    #[derive(Debug, Clone, Default)]
    pub(crate) struct RClient {
        conn: RConn,
        docs: BTreeSet<u8>,
        want: bool,
    }

    /// The socket file at the path.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum RSock {
        Absent,
        Mine,
        Stale,
    }

    /// The reference machine.
    #[derive(Debug, Clone)]
    pub(crate) struct Ref {
        phase: Phase,
        lock: bool,
        sock: RSock,
        bound: bool,
        swept: bool,
        clients: Vec<RClient>,
    }

    impl Ref {
        pub(crate) fn new() -> Self {
            Self {
                phase: Phase::Started,
                lock: false,
                sock: RSock::Absent,
                bound: false,
                swept: false,
                clients: vec![RClient::default(); usize::from(CLIENTS)],
            }
        }

        fn client(&self, c: u8) -> &RClient {
            &self.clients[usize::from(c)]
        }

        fn client_mut(&mut self, c: u8) -> &mut RClient {
            &mut self.clients[usize::from(c)]
        }

        fn listening(&self) -> bool {
            self.phase == Phase::Serving && self.sock == RSock::Mine
        }

        fn dead(&self) -> bool {
            matches!(self.phase, Phase::Exited | Phase::Failed | Phase::Crashed)
        }

        fn idle(&self) -> bool {
            self.clients
                .iter()
                .all(|c| !matches!(c.conn, RConn::Open { .. } | RConn::PeerClosed { .. }))
        }

        /// Whether `cmd` applies.
        pub(crate) fn enabled(&self, cmd: Cmd) -> bool {
            match cmd {
                Cmd::Lock => self.phase == Phase::Started,
                Cmd::UnlinkStale => self.phase == Phase::Locked,
                Cmd::Listen => self.phase == Phase::Locked && self.sock == RSock::Absent,
                Cmd::Connect(c) => self.listening() && self.client(c).conn == RConn::None,
                Cmd::ConnectRefused(c) => {
                    !self.listening() && self.client(c).conn == RConn::None && !self.client(c).want
                }
                Cmd::Accept(c) => {
                    self.phase == Phase::Serving && self.client(c).conn == RConn::Queued
                }
                Cmd::Open(c, d) => !self.client(c).docs.contains(&d),
                Cmd::Close(c, d) => self.client(c).docs.contains(&d),
                Cmd::Request(c) => !self.client(c).want,
                Cmd::Answer(c) => {
                    self.phase == Phase::Serving
                        && matches!(self.client(c).conn, RConn::Open { inflight: true, .. })
                }
                Cmd::Quit(c) => !self.client(c).want,
                Cmd::Notice(c) => self.client(c).conn == RConn::Closed,
                Cmd::Reap(c) => {
                    self.phase == Phase::Serving
                        && matches!(self.client(c).conn, RConn::PeerClosed { .. })
                }
                Cmd::Stop => self.phase == Phase::Serving,
                Cmd::IdleExit => self.phase == Phase::Serving && self.idle(),
                Cmd::Unlink => self.phase == Phase::Stopping,
                Cmd::Release => self.phase == Phase::Unlinked,
                Cmd::Crash => !self.dead(),
                Cmd::Sweep => self.phase == Phase::Serving && self.sock == RSock::Mine,
                Cmd::Tick | Cmd::Rival => true,
                Cmd::Restart => self.dead(),
            }
        }

        fn stop_connections(&mut self) {
            for client in &mut self.clients {
                client.conn = match client.conn {
                    RConn::Open { .. } | RConn::Queued => RConn::Closed,
                    RConn::PeerClosed { .. } => RConn::None,
                    ref other => other.clone(),
                };
            }
        }

        /// Applies an enabled `cmd`.
        pub(crate) fn apply(&mut self, cmd: Cmd) {
            match cmd {
                Cmd::Lock => {
                    self.phase = Phase::Locked;
                    self.lock = true;
                }
                Cmd::UnlinkStale => self.sock = RSock::Absent,
                Cmd::Listen => {
                    self.phase = Phase::Serving;
                    self.sock = RSock::Mine;
                    self.bound = true;
                }
                Cmd::Connect(c) => self.client_mut(c).conn = RConn::Queued,
                Cmd::ConnectRefused(c) => *self.client_mut(c) = RClient::default(),
                Cmd::Accept(c) => {
                    let client = self.client_mut(c);
                    client.conn = RConn::Open {
                        overlays: client.docs.clone(),
                        inflight: client.want,
                    };
                }
                Cmd::Open(c, d) => {
                    let client = self.client_mut(c);
                    client.docs.insert(d);
                    if let RConn::Open { overlays, .. } = &mut client.conn {
                        overlays.insert(d);
                    }
                }
                Cmd::Close(c, d) => {
                    let client = self.client_mut(c);
                    client.docs.remove(&d);
                    if let RConn::Open { overlays, .. } = &mut client.conn {
                        overlays.remove(&d);
                    }
                }
                Cmd::Request(c) => {
                    let client = self.client_mut(c);
                    client.want = true;
                    if let RConn::Open { inflight, .. } = &mut client.conn {
                        *inflight = true;
                    }
                }
                Cmd::Answer(c) => {
                    let client = self.client_mut(c);
                    client.want = false;
                    if let RConn::Open { inflight, .. } = &mut client.conn {
                        *inflight = false;
                    }
                }
                Cmd::Quit(c) => {
                    let client = self.client_mut(c);
                    client.conn = match std::mem::take(&mut client.conn) {
                        RConn::Open { overlays, .. } => RConn::PeerClosed { overlays },
                        _ => RConn::None,
                    };
                    client.docs.clear();
                }
                Cmd::Notice(c) | Cmd::Reap(c) => self.client_mut(c).conn = RConn::None,
                Cmd::Stop | Cmd::IdleExit => {
                    self.phase = Phase::Stopping;
                    self.stop_connections();
                }
                Cmd::Unlink => {
                    self.phase = Phase::Unlinked;
                    self.sock = RSock::Absent;
                    self.bound = false;
                    self.swept = false;
                }
                Cmd::Release => {
                    self.phase = Phase::Exited;
                    self.lock = false;
                }
                Cmd::Crash => {
                    if self.phase == Phase::Serving || self.phase == Phase::Stopping {
                        self.stop_connections();
                    }
                    self.phase = Phase::Crashed;
                    self.lock = false;
                    self.bound = false;
                    self.swept = false;
                    if self.sock == RSock::Mine {
                        self.sock = RSock::Stale;
                    }
                }
                Cmd::Sweep => {
                    self.sock = RSock::Absent;
                    self.swept = true;
                }
                Cmd::Tick | Cmd::Rival => {}
                Cmd::Restart => {
                    self.phase = Phase::Started;
                }
            }
        }

        /// What the tick must find.
        pub(crate) fn tick(&self) -> Tick {
            if self.swept && self.phase == Phase::Serving {
                Tick::SocketLost
            } else {
                Tick::Fine
            }
        }

        /// The next system move of the recovery schedule, or `None` at rest.
        pub(crate) fn system_move(&self) -> Option<Cmd> {
            if self.dead() {
                return Some(Cmd::Restart);
            }
            match self.phase {
                Phase::Started => return Some(Cmd::Lock),
                Phase::Locked if self.sock != RSock::Absent => return Some(Cmd::UnlinkStale),
                Phase::Locked => return Some(Cmd::Listen),
                Phase::Stopping => return Some(Cmd::Unlink),
                Phase::Unlinked => return Some(Cmd::Release),
                Phase::Serving if self.swept => return Some(Cmd::Stop),
                _ => {}
            }
            (0..CLIENTS).find_map(|c| match &self.client(c).conn {
                RConn::Closed => Some(Cmd::Notice(c)),
                RConn::PeerClosed { .. } => Some(Cmd::Reap(c)),
                RConn::None if self.client(c).want => Some(Cmd::Connect(c)),
                RConn::Queued => Some(Cmd::Accept(c)),
                RConn::Open { inflight: true, .. } => Some(Cmd::Answer(c)),
                _ => None,
            })
        }

        /// Whether a client still waits.
        pub(crate) fn waiting(&self) -> bool {
            self.clients.iter().any(|c| c.want)
        }
    }

    /// Clients never spawn here.
    pub(crate) struct NoLaunch;

    impl Launcher for NoLaunch {
        fn launch(&mut self, _notify: &Path) -> io::Result<Box<dyn Spawned>> {
            Err(io::Error::other("the machine test spawns nothing"))
        }
    }

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// A scratch directory under the temporary directory, removed on drop: made first, so a
    /// panic while the rest is set up leaves nothing behind.
    pub(crate) struct Root(PathBuf);

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The real side: a host, a scratch state directory and the clients.
    pub(crate) struct Real {
        _root: Root,
        state: StateDir,
        socket: SocketPath,
        host: SteppedHost,
        clients: Vec<Proxy<NoLaunch>>,
        conns: BTreeMap<u8, ConnId>,
        /// Hosts started so far, and the one each queued client connected to: a restarted host's
        /// listener is not the one a client's queued connection was reset with.
        incarnation: u64,
        queued_on: BTreeMap<u8, u64>,
    }

    fn uri(d: u8) -> DocumentUri {
        DocumentUri::parse(&format!("file:///w/D{d}.java")).unwrap()
    }

    impl Real {
        pub(crate) fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let root =
                Root(std::env::temp_dir().join(format!("jfl-machine-{}-{n}", std::process::id())));
            let _ = fs::remove_dir_all(&root.0);
            fs::create_dir_all(&root.0).unwrap();
            let state = StateDir::open(&root.0.join("s")).unwrap();
            let socket = SocketPath::of_state(&state).unwrap();
            let host = SteppedHost::new(Self::config(&state, &socket)).unwrap();
            let clients = (0..CLIENTS)
                .map(|c| {
                    Proxy::new(
                        &format!("c{c}"),
                        ClientKind::Cli,
                        state.clone(),
                        socket.clone(),
                        NoLaunch,
                        Budget {
                            reply: Duration::from_secs(10),
                            spawn: Duration::from_secs(1),
                        },
                    )
                })
                .collect();
            Self {
                _root: root,
                state,
                socket,
                host,
                clients,
                conns: BTreeMap::new(),
                incarnation: 0,
                queued_on: BTreeMap::new(),
            }
        }

        fn config(state: &StateDir, socket: &SocketPath) -> HostConfig {
            HostConfig {
                state: state.clone(),
                socket: socket.clone(),
                uid: current_uid(),
                notify: None,
            }
        }

        fn pump(&mut self, c: u8, n: usize) {
            if let Some(conn) = self.conns.get(&c).copied()
                && self.host.host().connections().contains_key(&conn)
            {
                for _ in 0..n {
                    self.host.receive(conn).unwrap();
                }
            }
        }

        /// Applies `cmd` to the real code; `expected_tick` is the reference's.
        pub(crate) fn apply(&mut self, cmd: Cmd, expected_tick: Tick, lock_held: bool) {
            let client = |c: u8| usize::from(c);
            match cmd {
                Cmd::Lock => self.host.lock().unwrap(),
                Cmd::UnlinkStale => self.host.unlink_stale().unwrap(),
                Cmd::Listen => self.host.listen().unwrap(),
                Cmd::Connect(c) => {
                    assert_eq!(
                        self.clients[client(c)].connect().unwrap(),
                        ProxyPhase::Connecting
                    );
                    self.queued_on.insert(c, self.incarnation);
                }
                Cmd::ConnectRefused(c) => {
                    assert_eq!(
                        self.clients[client(c)].connect().unwrap(),
                        ProxyPhase::NoDaemon
                    );
                    self.clients[client(c)].quit();
                }
                Cmd::Accept(c) => {
                    let hello = self.clients[client(c)].hello().cloned().unwrap();
                    let conn = self.host.accept(&hello).unwrap();
                    self.conns.insert(c, conn);
                    let sent = self.clients[client(c)].on_accepted().unwrap();
                    self.pump(c, sent);
                }
                Cmd::Open(c, d) => {
                    let sent = self.clients[client(c)].did_open(uri(d), 1, "class D {}".into());
                    self.pump(c, usize::from(sent));
                }
                Cmd::Close(c, d) => {
                    let sent = self.clients[client(c)].did_close(&uri(d));
                    self.pump(c, usize::from(sent));
                }
                Cmd::Request(c) => {
                    let sent = self.clients[client(c)]
                        .request::<Status>(StatusParams { worktree: None })
                        .unwrap();
                    self.pump(c, usize::from(sent));
                }
                Cmd::Answer(c) => {
                    let conn = self.conns[&c];
                    self.host.answer(conn).unwrap();
                    self.clients[client(c)].read_answer::<Status>().unwrap();
                }
                Cmd::Quit(c) => {
                    self.clients[client(c)].quit();
                    if let Some(conn) = self.conns.remove(&c)
                        && self.host.host().connections().contains_key(&conn)
                    {
                        assert_eq!(self.host.receive(conn).unwrap(), Received::Closed);
                        self.conns.insert(c, conn);
                    }
                }
                Cmd::Notice(c) => {
                    self.clients[client(c)].notice().unwrap();
                    self.conns.remove(&c);
                }
                Cmd::Reap(c) => {
                    let conn = self.conns.remove(&c).unwrap();
                    self.host.reap(conn).unwrap();
                }
                Cmd::Stop => self.host.stop().unwrap(),
                Cmd::IdleExit => self.host.idle_exit().unwrap(),
                Cmd::Unlink => self.host.unlink().unwrap(),
                Cmd::Release => self.host.release().unwrap(),
                Cmd::Crash => self.host.crash(),
                Cmd::Sweep => fs::remove_file(self.socket.path()).unwrap(),
                Cmd::Tick => assert_eq!(self.host.tick().unwrap(), expected_tick),
                Cmd::Restart => {
                    self.host = SteppedHost::new(Self::config(&self.state, &self.socket)).unwrap();
                    self.incarnation = self.incarnation.saturating_add(1);
                    // A new host numbers its connections from 0 again: no record names one.
                    self.conns.clear();
                }
                Cmd::Rival => {
                    let mut rival =
                        SteppedHost::new(Self::config(&self.state, &self.socket)).unwrap();
                    let locked = rival.lock();
                    assert_eq!(locked.is_err(), lock_held, "the rival's lock: {locked:?}");
                    rival.crash();
                }
            }
            // A client that quit with its connection queued, or whose closed connection it read:
            // the slot is free again.
            self.conns.retain(|c, conn| {
                self.clients[usize::from(*c)].hello().is_some()
                    || self.host.host().connections().contains_key(conn)
            });
        }

        /// The real side, as the reference's terms.
        pub(crate) fn observe(&self) -> (Phase, bool, RSock, bool, bool, Vec<RConn>) {
            let host = self.host.host();
            let sock = match jfl_adapter_ipc::FileId::at(self.socket.path()).unwrap() {
                None => RSock::Absent,
                Some(id) if Some(id) == host.bound() => RSock::Mine,
                Some(_) => RSock::Stale,
            };
            let conns = (0..CLIENTS)
                .map(|c| {
                    let proxy = &self.clients[usize::from(c)];
                    let open = self.conns.get(&c).and_then(|id| host.connections().get(id));
                    match (proxy.phase(), self.conns.get(&c), open) {
                        (_, Some(_), Some(conn)) => {
                            let overlays: BTreeSet<u8> = conn
                                .overlays()
                                .keys()
                                .map(|u| {
                                    u.as_str()
                                        .trim_start_matches("file:///w/D")
                                        .trim_end_matches(".java")
                                        .parse()
                                        .unwrap()
                                })
                                .collect();
                            if conn.peer_closed() {
                                RConn::PeerClosed { overlays }
                            } else {
                                RConn::Open {
                                    overlays,
                                    inflight: conn.inflight(),
                                }
                            }
                        }
                        (ProxyPhase::Connecting, None, _)
                            if host.listening()
                                && self.queued_on.get(&c) == Some(&self.incarnation) =>
                        {
                            RConn::Queued
                        }
                        (ProxyPhase::Connecting | ProxyPhase::Ready, _, _) => RConn::Closed,
                        _ => RConn::None,
                    }
                })
                .collect();
            (
                host.phase(),
                host.lock_id().is_some(),
                sock,
                host.bound().is_some(),
                host.listening(),
                conns,
            )
        }
    }

    /// The reference's view in the same terms.
    pub(crate) fn expected(reference: &Ref) -> (Phase, bool, RSock, bool, bool, Vec<RConn>) {
        (
            reference.phase,
            reference.lock,
            reference.sock,
            reference.bound,
            reference.phase == Phase::Serving,
            reference.clients.iter().map(|c| c.conn.clone()).collect(),
        )
    }

    /// Runs `cmds` (the enabled ones) on both sides, comparing after each; then the recovery.
    pub(crate) fn run(cmds: &[Cmd]) -> Result<(), TestCaseError> {
        let mut reference = Ref::new();
        let mut real = Real::new();
        for &cmd in cmds {
            if !reference.enabled(cmd) {
                continue;
            }
            real.apply(cmd, reference.tick(), reference.lock);
            reference.apply(cmd);
            prop_assert_eq!(real.observe(), expected(&reference), "after {:?}", cmd);
        }
        // EventuallyServed's reduction: no more faults; system moves only.
        let mut moves = 0;
        while let Some(cmd) = reference.system_move() {
            prop_assert!(
                moves < RECOVERY,
                "not at rest after {} system moves",
                RECOVERY
            );
            real.apply(cmd, reference.tick(), reference.lock);
            reference.apply(cmd);
            prop_assert_eq!(
                real.observe(),
                expected(&reference),
                "recovering, after {:?}",
                cmd
            );
            moves = moves.saturating_add(1);
        }
        prop_assert!(!reference.waiting(), "a client still waits at rest");
        prop_assert_eq!(real.host.host().phase(), Phase::Serving);
        Ok(())
    }
}

use machine::{Cmd, command, config, run};
use proptest::prelude::*;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_match_the_reference_and_serve_every_waiting_client_when_faults_stop(
        cmds in proptest::collection::vec(command(), 1..80),
    ) {
        // given: the generated commands, each run when the reference enables it

        // when
        let outcome = run(&cmds);

        // then: every observation matched, and the recovery served every waiting client
        outcome?;
    }
}

#[test]
fn should_serve_a_waiting_client_after_a_crash_when_the_schedule_recovers() {
    // given: a client waiting on a daemon that crashed with its socket left stale
    let cmds = [
        Cmd::Lock,
        Cmd::Listen,
        Cmd::Open(0, 1),
        Cmd::Request(0),
        Cmd::Connect(0),
        Cmd::Accept(0),
        Cmd::Crash,
        Cmd::Rival,
        Cmd::Sweep,
    ];

    // when
    let outcome = run(&cmds);

    // then
    assert!(outcome.is_ok(), "{outcome:?}");
}
