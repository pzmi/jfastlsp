//! The driver of `formal/daemon.qnt` (IDX-12; plan §7.2, `doc/formal-models.md`): the model's
//! stimuli applied to `jfl-composition`'s daemon host ([`SteppedHost`], one per daemon slot) and
//! proxies ([`Proxy`], one per proxy), in process, with real Unix sockets, `flock`s and files in a
//! fresh state directory under the system's temporary directory; the real state observed after
//! every step.
//!
//! A spawn starts an in-process host in the model's first free slot (its launcher records the
//! notify path; the host reports on it); a crash is [`SteppedHost::crash`], which closes every
//! descriptor and runs none of the shutdown. The initial stale state is what a crash leaves: a
//! throwaway host locks, binds and crashes, so the socket file stays and the lock file is unlocked.
//!
//! What is observed from the real code: each host's phase, the inode its lock descriptor holds
//! (numbered in the order the driver first sees an inode) and whether it listens; the inode at
//! `daemon.lock`; whether `sock` exists and which host bound its inode (or, with no file, which
//! live host still holds a bound socket the path no longer names: the model's swept socket);
//! which proxy, or which host shutting down, holds the spawn lock; each accepted connection's
//! overlays, requests and EOF; each proxy's phase, documents and request. What is the driver's
//! record: which model slot a
//! connection is (the proxy's handshake id names it, so the host is told which queued connection
//! the model accepts), the daemon slot a proxy awaits, and the step count `t`. A connection the
//! host has not opened is `backlog` while its host listens and `closed` once it stopped (the
//! kernel resets a stopped listener's backlog).
//!
//! Every type of the observation, nested ones included, has `deny_unknown_fields` and a derived
//! `PartialEq` (IDX-12's obligation 10).
//!
//! The negative control ([`control`], plan §7.2): a host that releases the lock before removing
//! its socket. It diverges deterministically at the first shutdown step of a trace (`sigterm`,
//! `idleExit` or `noticeSwept`), where the model's daemon still holds its lock and the faulty
//! host has released it; the committed `daemon-sigterm-after-stale-start`,
//! `daemon-idle-exit-races-connect` and `daemon-swept-socket-stops` traces each have one. Pinned
//! by tests/replay.rs::should_diverge_at_the_shutdown_when_the_daemon_control_releases_first.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use jfl_adapter_ipc::{FileId, SocketPath, StateDir, current_uid};
use jfl_composition::daemon::host::{ConnId, HostConfig, Phase, Received, Tick};
use jfl_composition::daemon::proxy::{Budget, Handshook, Launcher, Proxy, ProxyPhase, Spawned};
use jfl_composition::daemon::stepped::SteppedHost;
use jfl_protocol::DocumentUri;
use jfl_protocol::messages::{ClientKind, Status, StatusParams};
use jfl_protocol::rpc::RequestId;
use serde::{Deserialize, Serialize};

use crate::driver::{Driver, Step, StepError};
use crate::replay::Replayer;

const DAEMONS: [&str; 3] = ["d1", "d2", "d3"];
const CONNS: [&str; 3] = ["c1", "c2", "c3"];
const PROXIES: [&str; 2] = ["p1", "p2"];

/// A daemon slot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonObs {
    /// The model's phase name.
    pub phase: String,
    /// The inode its lock descriptor holds, numbered; 0 for none.
    pub lock: i64,
    /// Its shutdown began without the spawn lock (its wait for it expired).
    pub alone: bool,
}

/// `<state>/sock`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SockObs {
    /// A file is at the path.
    pub present: bool,
    /// The slot that bound it (or, with no file, whose bound socket was swept); `""` for none.
    pub owner: String,
}

/// A connection slot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnObs {
    /// `free`, `backlog`, `open`, `peerClosed` or `closed`.
    pub state: String,
    /// Its proxy.
    pub proxy: String,
    /// Its daemon slot.
    pub daemon: String,
    /// The daemon's overlays for it, by document name.
    pub overlays: BTreeSet<String>,
    /// It holds an unanswered request.
    pub inflight: bool,
}

/// A proxy.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyObs {
    /// The model's phase name.
    pub phase: String,
    /// Its connection slot, or `""`.
    pub conn: String,
    /// The daemon slot it awaits, or `""`.
    pub daemon: String,
    /// Its client's open documents.
    pub docs: BTreeSet<String>,
    /// Its client waits for an answer.
    pub want: bool,
}

/// The daemon model's seven variables, as the driver observes them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DaemonState {
    /// The daemon slots.
    pub daemons: BTreeMap<String, DaemonObs>,
    /// The inode at `daemon.lock`, numbered; 0 for none.
    pub lock_file: i64,
    /// The socket file.
    pub sock: SockObs,
    /// The proxy, or the daemon slot shutting down, holding the spawn lock; `""` for none.
    pub spawn_lock: String,
    /// The connection slots.
    pub conns: BTreeMap<String, ConnObs>,
    /// The steps taken.
    pub t: i64,
    /// The proxies.
    pub proxies: BTreeMap<String, ProxyObs>,
}

/// The faults: none for the driver, one for the negative control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    /// The shutdown releases the lock as it stops, before the unlink.
    ReleaseBeforeUnlink,
}

/// A spawn the proxy asked for: its notify path and the flag its child reads.
type Launch = (PathBuf, Rc<Cell<bool>>);

/// The launcher the driver's proxies use: it records each spawn for the driver to start.
struct InProcess(Rc<RefCell<Vec<Launch>>>);

struct Child(Rc<Cell<bool>>);

impl Spawned for Child {
    fn exited(&mut self) -> bool {
        self.0.get()
    }
}

impl Launcher for InProcess {
    fn launch(&mut self, notify: &Path) -> std::io::Result<Box<dyn Spawned>> {
        let dead = Rc::new(Cell::new(false));
        self.0
            .borrow_mut()
            .push((notify.to_owned(), Rc::clone(&dead)));
        Ok(Box::new(Child(dead)))
    }
}

struct Slot {
    host: SteppedHost,
    dead: Rc<Cell<bool>>,
}

/// The driver's record of one connection slot.
#[derive(Debug, Clone)]
struct ConnRec {
    proxy: String,
    daemon: String,
    hello: RequestId,
    host: Option<ConnId>,
}

struct World {
    root: PathBuf,
    state: StateDir,
    socket: SocketPath,
}

impl Drop for World {
    fn drop(&mut self) {
        // The driver's own directory under the temporary directory.
        let _ignored = std::fs::remove_dir_all(&self.root);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Drives the daemon host and proxies by `daemon.qnt`'s actions.
pub struct DaemonDriver {
    fault: Fault,
    slots: BTreeMap<String, Slot>,
    proxies: BTreeMap<String, Proxy<InProcess>>,
    launches: Rc<RefCell<Vec<Launch>>>,
    awaiting: BTreeMap<String, String>,
    conns: BTreeMap<String, ConnRec>,
    inodes: RefCell<BTreeMap<FileId, i64>>,
    sockets: BTreeMap<FileId, String>,
    t: i64,
    // Last: dropped after every descriptor in it.
    world: Option<World>,
}

fn refused(e: impl ToString) -> StepError {
    StepError::Refused(e.to_string())
}

fn harness(step: &Step, why: &str) -> StepError {
    StepError::Harness(format!("step {}: `{}`: {why}", step.index(), step.action()))
}

fn pick(step: &Step, name: &str) -> Result<String, StepError> {
    step.pick::<String>(name)?
        .ok_or_else(|| harness(step, &format!("no `{name}` pick")))
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Started => "started",
        Phase::Locked => "locked",
        Phase::Serving => "serving",
        Phase::Stopping => "stopping",
        Phase::Unlinked => "unlinked",
        Phase::Exited => "exited",
        Phase::Failed => "failed",
        Phase::Crashed => "crashed",
    }
}

fn proxy_phase(phase: ProxyPhase) -> &'static str {
    match phase {
        ProxyPhase::Disconnected => "disconnected",
        ProxyPhase::NoDaemon => "noDaemon",
        ProxyPhase::Spawning => "spawning",
        ProxyPhase::Awaiting => "awaiting",
        ProxyPhase::Connecting => "connecting",
        ProxyPhase::Ready => "ready",
    }
}

fn uri(doc: &str) -> Result<DocumentUri, StepError> {
    DocumentUri::parse(&format!("file:///w/{doc}.java")).map_err(refused)
}

fn doc_name(uri: &DocumentUri) -> String {
    uri.as_str()
        .trim_start_matches("file:///w/")
        .trim_end_matches(".java")
        .to_owned()
}

impl DaemonDriver {
    fn new(fault: Fault) -> Self {
        Self {
            fault,
            slots: BTreeMap::new(),
            proxies: BTreeMap::new(),
            launches: Rc::new(RefCell::new(Vec::new())),
            awaiting: BTreeMap::new(),
            conns: BTreeMap::new(),
            inodes: RefCell::new(BTreeMap::new()),
            sockets: BTreeMap::new(),
            t: 0,
            world: None,
        }
    }

    fn world(&self) -> Result<&World, StepError> {
        self.world
            .as_ref()
            .ok_or_else(|| StepError::Harness("a step before `init`".to_owned()))
    }

    fn config(&self, notify: Option<PathBuf>) -> Result<HostConfig, StepError> {
        let world = self.world()?;
        Ok(HostConfig {
            state: world.state.clone(),
            socket: world.socket.clone(),
            uid: current_uid(),
            notify,
        })
    }

    fn init(&mut self, stale: bool) -> Result<(), StepError> {
        self.proxies.clear();
        self.slots.clear();
        self.conns.clear();
        self.awaiting.clear();
        self.sockets.clear();
        self.inodes.borrow_mut().clear();
        self.launches.borrow_mut().clear();
        self.t = 0;
        self.world = None;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("jflc-{}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&root);
        let state = StateDir::open(&root.join("s")).map_err(refused)?;
        let socket = SocketPath::of_state(&state).map_err(refused)?;
        self.world = Some(World {
            root,
            state,
            socket,
        });
        if stale {
            let mut crashed = SteppedHost::new(self.config(None)?).map_err(refused)?;
            crashed.lock().map_err(refused)?;
            crashed.listen().map_err(refused)?;
            crashed.crash();
        }
        let (state, socket) = {
            let world = self.world()?;
            (world.state.clone(), world.socket.clone())
        };
        for name in PROXIES {
            self.proxies.insert(
                name.to_owned(),
                Proxy::new(
                    name,
                    ClientKind::Lsp,
                    state.clone(),
                    socket.clone(),
                    InProcess(Rc::clone(&self.launches)),
                    Budget {
                        reply: Duration::from_secs(10),
                        spawn: Duration::from_secs(10),
                    },
                ),
            );
        }
        Ok(())
    }

    fn proxy(&mut self, name: &str) -> Result<&mut Proxy<InProcess>, StepError> {
        self.proxies
            .get_mut(name)
            .ok_or_else(|| StepError::Harness(format!("no proxy `{name}`")))
    }

    fn slot(&mut self, name: &str) -> Result<&mut Slot, StepError> {
        self.slots
            .get_mut(name)
            .ok_or_else(|| StepError::Refused(format!("no daemon in slot `{name}`")))
    }

    /// The model slot of `proxy`'s current connection.
    fn conn_of(&self, proxy: &str) -> Option<String> {
        let hello = self.proxies.get(proxy)?.hello()?;
        self.conns
            .iter()
            .find(|(_, rec)| rec.proxy == proxy && &rec.hello == hello)
            .map(|(slot, _)| slot.clone())
    }

    /// Lets the daemon read `n` frames `proxy` just sent on its open connection.
    fn pump(&mut self, proxy: &str, n: usize) -> Result<(), StepError> {
        let Some(slot) = self.conn_of(proxy) else {
            return Ok(());
        };
        let Some(rec) = self.conns.get(&slot).cloned() else {
            return Ok(());
        };
        let Some(conn) = rec.host else {
            return Ok(());
        };
        let host = &mut self.slot(&rec.daemon)?.host;
        for _ in 0..n {
            host.receive(conn).map_err(refused)?;
        }
        Ok(())
    }

    fn number(&self, id: FileId) -> i64 {
        let mut inodes = self.inodes.borrow_mut();
        let next = i64::try_from(inodes.len())
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        *inodes.entry(id).or_insert(next)
    }

    fn phase_of(&self, slot: &str) -> &'static str {
        self.slots
            .get(slot)
            .map_or("none", |s| phase_name(s.host.host().phase()))
    }

    fn sock(&self) -> SockObs {
        let Some(world) = self.world.as_ref() else {
            return SockObs {
                present: false,
                owner: String::new(),
            };
        };
        match FileId::at(world.socket.path()) {
            Ok(Some(id)) => SockObs {
                present: true,
                owner: self.sockets.get(&id).cloned().unwrap_or_default(),
            },
            _ => SockObs {
                present: false,
                owner: self
                    .slots
                    .iter()
                    .find(|(_, s)| s.host.host().bound().is_some())
                    .map(|(name, _)| name.clone())
                    .unwrap_or_default(),
            },
        }
    }

    fn conn_obs(&self, rec: &ConnRec) -> ConnObs {
        let host = self.slots.get(&rec.daemon).map(|s| s.host.host());
        let open = rec
            .host
            .and_then(|id| host.and_then(|h| h.connections().get(&id)));
        let (state, overlays, inflight) = match (rec.host, open) {
            (Some(_), Some(conn)) => (
                if conn.peer_closed() {
                    "peerClosed"
                } else {
                    "open"
                },
                conn.overlays().keys().map(doc_name).collect(),
                conn.inflight(),
            ),
            (Some(_), None) => ("closed", BTreeSet::new(), false),
            (None, _) if host.is_some_and(|h| h.listening()) => ("backlog", BTreeSet::new(), false),
            (None, _) => ("closed", BTreeSet::new(), false),
        };
        ConnObs {
            state: state.to_owned(),
            proxy: rec.proxy.clone(),
            daemon: rec.daemon.clone(),
            overlays,
            inflight,
        }
    }

    /// The model's first free daemon slot.
    fn free_daemon(&self) -> Option<&'static str> {
        let sock = self.sock();
        DAEMONS.into_iter().find(|d| {
            matches!(self.phase_of(d), "none" | "exited" | "failed" | "crashed")
                && !(sock.present && sock.owner == *d)
                && !self.conns.values().any(|c| c.daemon == *d)
                && !self.awaiting.values().any(|a| a == d)
        })
    }

    /// After every step: a child's death is what its proxy's notify wait sees; a connection
    /// gone from both ends leaves its slot.
    fn settle(&mut self) {
        for slot in self.slots.values() {
            let phase = slot.host.host().phase();
            slot.dead.set(matches!(
                phase,
                Phase::Exited | Phase::Failed | Phase::Crashed
            ));
        }
        let gone: Vec<String> = self
            .conns
            .iter()
            .filter(|(slot, rec)| {
                let client = self
                    .proxies
                    .get(&rec.proxy)
                    .and_then(|p| p.hello())
                    .is_some_and(|h| h == &rec.hello);
                let host = rec.host.is_some_and(|id| {
                    self.slots
                        .get(&rec.daemon)
                        .is_some_and(|s| s.host.host().connections().contains_key(&id))
                });
                let _ = slot;
                !client && !host
            })
            .map(|(slot, _)| slot.clone())
            .collect();
        for slot in gone {
            self.conns.remove(&slot);
        }
    }

    fn shutdown_step(&mut self, step: &Step, daemon: &str, how: &str) -> Result<(), StepError> {
        let fault = self.fault;
        let host = &mut self.slot(daemon)?.host;
        match how {
            "sigterm" => host.stop().map_err(refused)?,
            "idleExit" => host.idle_exit().map_err(refused)?,
            "noticeSwept" => match host.tick().map_err(refused)? {
                Tick::SocketLost => host.stop().map_err(refused)?,
                other => {
                    return Err(refused(format!(
                        "the tick found {other:?}, not a swept socket"
                    )));
                }
            },
            _ => return Err(StepError::unknown(step)),
        }
        if fault == Fault::ReleaseBeforeUnlink {
            host.release().map_err(refused)?;
        }
        Ok(())
    }

    fn apply(&mut self, step: &Step) -> Result<(), StepError> {
        match step.action() {
            "init" | "initClean" | "initStale" => {
                let stale = step
                    .pick::<bool>("stale")?
                    .ok_or_else(|| harness(step, "no `stale` pick"))?;
                self.init(stale)
            }
            "request" => {
                let name = pick(step, "proxy")?;
                let sent = self
                    .proxy(&name)?
                    .request::<Status>(StatusParams { worktree: None })
                    .map_err(refused)?;
                self.pump(&name, usize::from(sent))
            }
            "didOpen" | "didClose" => {
                let name = pick(step, "proxy")?;
                let doc = uri(&pick(step, "doc")?)?;
                let proxy = self.proxy(&name)?;
                let sent = if step.action() == "didOpen" {
                    proxy.did_open(doc, 1, "class X {}".to_owned())
                } else {
                    proxy.did_close(&doc)
                };
                self.pump(&name, usize::from(sent))
            }
            "quit" => {
                let name = pick(step, "proxy")?;
                let slot = self.conn_of(&name);
                self.proxy(&name)?.quit();
                self.awaiting.remove(&name);
                if let Some(slot) = slot
                    && let Some(rec) = self.conns.get(&slot).cloned()
                {
                    let open = rec.host.filter(|id| {
                        self.slots
                            .get(&rec.daemon)
                            .is_some_and(|s| s.host.host().connections().contains_key(id))
                    });
                    match open {
                        Some(conn) => {
                            let read = self
                                .slot(&rec.daemon)?
                                .host
                                .receive(conn)
                                .map_err(refused)?;
                            if read != Received::Closed {
                                return Err(refused(format!("read {read:?}, not the EOF")));
                            }
                        }
                        None => {
                            self.conns.remove(&slot);
                        }
                    }
                }
                Ok(())
            }
            "connect" => {
                let name = pick(step, "proxy")?;
                let daemon = self.sock().owner;
                let free = CONNS
                    .into_iter()
                    .find(|c| !self.conns.contains_key(*c))
                    .map(str::to_owned);
                let proxy = self.proxy(&name)?;
                if proxy.connect().map_err(refused)? == ProxyPhase::Connecting {
                    let hello = proxy
                        .hello()
                        .cloned()
                        .ok_or_else(|| refused("connected without a handshake id"))?;
                    let slot = free.ok_or_else(|| refused("no free connection slot"))?;
                    self.conns.insert(
                        slot,
                        ConnRec {
                            proxy: name,
                            daemon,
                            hello,
                            host: None,
                        },
                    );
                }
                Ok(())
            }
            "takeSpawnLock" => {
                let name = pick(step, "proxy")?;
                self.proxy(&name)?.take_spawn_lock().map_err(refused)?;
                Ok(())
            }
            "spawn" => {
                let name = pick(step, "proxy")?;
                let free = self.free_daemon();
                if self.proxy(&name)?.spawn().map_err(refused)? == ProxyPhase::Awaiting {
                    let (notify, dead) = self
                        .launches
                        .borrow_mut()
                        .pop()
                        .ok_or_else(|| harness(step, "the proxy spawned without a launch"))?;
                    let slot = free.ok_or_else(|| refused("no free daemon slot"))?;
                    let host = SteppedHost::new(self.config(Some(notify))?).map_err(refused)?;
                    self.slots.insert(slot.to_owned(), Slot { host, dead });
                    self.awaiting.insert(name, slot.to_owned());
                }
                Ok(())
            }
            "handshake" | "spawnTimeout" => {
                let name = pick(step, "proxy")?;
                let outcome = self
                    .proxy(&name)?
                    .await_notice(Duration::ZERO)
                    .map_err(refused)?;
                self.awaiting.remove(&name);
                match (step.action(), outcome) {
                    ("handshake", Handshook::TimedOut) => Err(refused(
                        "the notify wait expired where the model's handshake answers",
                    )),
                    ("spawnTimeout", o) if o != Handshook::TimedOut => Err(refused(format!(
                        "the notify wait gave {o:?} where the model's expires"
                    ))),
                    _ => Ok(()),
                }
            }
            "notice" => {
                let name = pick(step, "proxy")?;
                let slot = self.conn_of(&name);
                self.proxy(&name)?.notice().map_err(refused)?;
                if let Some(slot) = slot {
                    self.conns.remove(&slot);
                }
                Ok(())
            }
            "lock" => {
                let daemon = pick(step, "daemon")?;
                match self.slot(&daemon)?.host.lock() {
                    Ok(()) | Err(jfl_composition::daemon::host::HostError::Lock(_)) => Ok(()),
                    Err(e) => Err(refused(e)),
                }
            }
            "unlinkStale" => {
                let daemon = pick(step, "daemon")?;
                self.slot(&daemon)?.host.unlink_stale().map_err(refused)
            }
            "listen" => {
                let daemon = pick(step, "daemon")?;
                let host = &mut self.slot(&daemon)?.host;
                host.listen().map_err(refused)?;
                let bound = host
                    .host()
                    .bound()
                    .ok_or_else(|| refused("listening with no bound socket"))?;
                self.sockets.insert(bound, daemon);
                Ok(())
            }
            "accept" => {
                let daemon = pick(step, "daemon")?;
                let slot = pick(step, "conn")?;
                let rec = self
                    .conns
                    .get(&slot)
                    .cloned()
                    .ok_or_else(|| refused(format!("no connection in `{slot}`")))?;
                let id = self
                    .slot(&daemon)?
                    .host
                    .accept(&rec.hello)
                    .map_err(refused)?;
                if let Some(rec) = self.conns.get_mut(&slot) {
                    rec.host = Some(id);
                }
                let sent = self.proxy(&rec.proxy)?.on_accepted().map_err(refused)?;
                self.pump(&rec.proxy, sent)
            }
            "answer" => {
                let daemon = pick(step, "daemon")?;
                let slot = pick(step, "conn")?;
                let rec = self
                    .conns
                    .get(&slot)
                    .cloned()
                    .ok_or_else(|| refused(format!("no connection in `{slot}`")))?;
                let conn = rec
                    .host
                    .ok_or_else(|| refused("answering a queued connection"))?;
                self.slot(&daemon)?.host.answer(conn).map_err(refused)?;
                self.proxy(&rec.proxy)?
                    .read_answer::<Status>()
                    .map_err(refused)?;
                Ok(())
            }
            "reap" => {
                let daemon = pick(step, "daemon")?;
                let slot = pick(step, "conn")?;
                let conn = self
                    .conns
                    .get(&slot)
                    .and_then(|r| r.host)
                    .ok_or_else(|| refused(format!("no open connection in `{slot}`")))?;
                self.slot(&daemon)?.host.reap(conn).map_err(refused)?;
                self.conns.remove(&slot);
                Ok(())
            }
            "sigterm" | "idleExit" | "noticeSwept" => {
                let daemon = pick(step, "daemon")?;
                self.shutdown_step(step, &daemon, step.action())
            }
            "spawnLockWaitExpires" => {
                let daemon = pick(step, "daemon")?;
                self.slot(&daemon)?.host.stop_alone().map_err(refused)
            }
            "noticeSweptAlone" => {
                let daemon = pick(step, "daemon")?;
                let host = &mut self.slot(&daemon)?.host;
                match host.tick().map_err(refused)? {
                    Tick::SocketLost => host.stop_alone().map_err(refused),
                    other => Err(refused(format!(
                        "the tick found {other:?}, not a swept socket"
                    ))),
                }
            }
            "unlink" => {
                let daemon = pick(step, "daemon")?;
                self.slot(&daemon)?.host.unlink().map_err(refused)
            }
            "exit" => {
                let daemon = pick(step, "daemon")?;
                self.slot(&daemon)?.host.release().map_err(refused)
            }
            "crash" => {
                let daemon = pick(step, "daemon")?;
                self.slot(&daemon)?.host.crash();
                Ok(())
            }
            "sweep" => {
                let world = self.world()?;
                std::fs::remove_file(world.socket.path()).map_err(refused)
            }
            _ => Err(StepError::unknown(step)),
        }
    }
}

impl Driver for DaemonDriver {
    type State = DaemonState;

    fn step(&mut self, step: &Step) -> Result<(), StepError> {
        let applied = self.apply(step);
        if !matches!(step.action(), "init" | "initClean" | "initStale") {
            self.t = self.t.saturating_add(1);
        }
        self.settle();
        applied
    }

    fn observe(&self) -> DaemonState {
        let daemons = DAEMONS
            .into_iter()
            .map(|d| {
                let lock = self
                    .slots
                    .get(d)
                    .and_then(|s| s.host.host().lock_id())
                    .map_or(0, |id| self.number(id));
                (
                    d.to_owned(),
                    DaemonObs {
                        phase: self.phase_of(d).to_owned(),
                        lock,
                        alone: self.slots.get(d).is_some_and(|s| s.host.host().alone()),
                    },
                )
            })
            .collect();
        let lock_file = self
            .world
            .as_ref()
            .and_then(|w| FileId::at(&w.state.lock_path()).ok().flatten())
            .map_or(0, |id| self.number(id));
        let spawn_lock = self
            .proxies
            .iter()
            .find(|(_, p)| p.holds_spawn_lock())
            .map(|(name, _)| name.clone())
            .or_else(|| {
                DAEMONS
                    .into_iter()
                    .find(|d| {
                        self.slots
                            .get(*d)
                            .is_some_and(|s| s.host.host().holds_spawn_lock())
                    })
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let conns = CONNS
            .into_iter()
            .map(|c| {
                let obs = self.conns.get(c).map_or_else(
                    || ConnObs {
                        state: "free".to_owned(),
                        proxy: String::new(),
                        daemon: String::new(),
                        overlays: BTreeSet::new(),
                        inflight: false,
                    },
                    |rec| self.conn_obs(rec),
                );
                (c.to_owned(), obs)
            })
            .collect();
        let proxies = PROXIES
            .into_iter()
            .map(|p| {
                let obs = self.proxies.get(p).map_or_else(
                    || ProxyObs {
                        phase: "disconnected".to_owned(),
                        conn: String::new(),
                        daemon: String::new(),
                        docs: BTreeSet::new(),
                        want: false,
                    },
                    |proxy| ProxyObs {
                        phase: proxy_phase(proxy.phase()).to_owned(),
                        conn: self.conn_of(p).unwrap_or_default(),
                        daemon: self.awaiting.get(p).cloned().unwrap_or_default(),
                        docs: proxy.docs().map(doc_name).collect(),
                        want: proxy.wants(),
                    },
                );
                (p.to_owned(), obs)
            })
            .collect();
        DaemonState {
            daemons,
            lock_file,
            sock: self.sock(),
            spawn_lock,
            conns,
            t: self.t,
            proxies,
        }
    }
}

/// The daemon driver.
pub fn driver() -> Replayer {
    Replayer::new(DaemonDriver::new(Fault::None))
}

/// The negative control (plan §7.2): the shutdown releases the lock before removing the socket.
/// It diverges at a trace's first shutdown step.
pub fn control() -> Replayer {
    Replayer::new(DaemonDriver::new(Fault::ReleaseBeforeUnlink))
}
