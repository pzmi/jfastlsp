//! The view registry against a reference machine (ADR 0007, ver §6.4; IDX-09's Accept item 4),
//! over plain proptest (not `proptest-state-machine`: the IDX-09 card's decision).
//!
//! A generated sequence of operations (disk writes and deletes, `HEAD` moves, deliveries,
//! overflows, lost and restored watches, reconciles, requests with their cookies, pins, finishes
//! and `sync`) drives the registry and a small reference machine side by side, with a simulated
//! disk and an in-order event queue. After each step they must agree on whether the step was
//! taken, on the generation published, and at each pin on whether the answer is complete. And a
//! complete answer must be a snapshot (ADR 0037): the pinned files equal one state the disk held
//! between the request's start and its pin, with `HEAD` as it is at the pin.
//!
//! Two checks, two kinds of mutation. The reference machine restates each rule (the late-read
//! rule among them), so a registry that drops one disagrees with it at the pin. The snapshot check
//! restates none: it compares the pinned files with the disk's history, so it also catches a rule
//! dropped from both. The generator mixes in [`late_read`] fragments (a request, a burst of writes
//! behind its cookie, the deliveries, a reconcile, the pin), which reach ADR 0037's round-2
//! counterexample: without the late-read rule in either, the regression test
//! [`should_refuse_complete_when_a_reconcile_read_after_the_cookie_published_a_mix`] and the
//! property fail on the snapshot check.
//!
//! Seeded (ver §3): every run starts from [`SEED`]; proptest writes a shrunk failure to
//! `proptest-regressions/`, which is committed.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use jfl_application::source_tree::{FileStat, MTime};
use jfl_application::views::{
    Cookie, Event, FileSeen, GitSeen, RequestId, Stamp, Stored, ViewRegistry,
};
use jfl_domain::{FileUri, RelPath};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

#[cfg(test)]
mod machine {
    use super::*;

    /// The fixed seed of every property here (ver §3 rule 2).
    pub(crate) const SEED: u64 = 0x1DC0_0902;

    pub(crate) const PATHS: [&str; 3] = ["A.java", "B.java", "C.java"];
    pub(crate) const ROOT: &str = "/w/m";

    pub(crate) type Registry = ViewRegistry<FileSeen, FileSeen, u64>;

    #[derive(Debug, Clone)]
    pub(crate) enum Op {
        Write(usize),
        Delete(usize),
        Checkout,
        Deliver,
        Overflow,
        WatchLost,
        WatchRestored,
        Reconcile,
        Start(u64, usize),
        Recookie(u64),
        CookieLost(u64),
        Pin(u64),
        Finish(u64),
        SyncStart,
        SyncDone,
    }

    /// A request on `p`, writes to `a` and `b` behind its cookie (`a` twice, so its delivered
    /// event and the disk differ), both deliveries, a reconcile and the pin: a reconcile that read
    /// after the cookie, publishing one file newer than its delivered event beside another older
    /// than the disk (ADR 0037's round-2 counterexample).
    pub(crate) fn late_read(id: u64, p: usize, a: usize, b: usize) -> Vec<Op> {
        vec![
            Op::Start(id, p),
            Op::Write(a),
            Op::Write(b),
            Op::Write(a),
            Op::Deliver,
            Op::Deliver,
            Op::Reconcile,
            Op::Pin(id),
        ]
    }

    /// Single operations, with [`late_read`] fragments mixed in.
    pub(crate) fn ops() -> impl Strategy<Value = Vec<Op>> {
        let paths = 0..PATHS.len();
        let fragment = prop_oneof![
            6 => op().prop_map(|op| vec![op]),
            1 => (1_u64..3, paths.clone(), paths.clone(), paths)
                .prop_map(|(id, p, a, b)| late_read(id, p, a, b)),
        ];
        proptest::collection::vec(fragment, 1..40)
            .prop_map(|fragments| fragments.into_iter().flatten().collect())
    }

    pub(crate) fn op() -> impl Strategy<Value = Op> {
        let path = 0..PATHS.len();
        let id = 1_u64..3;
        prop_oneof![
            3 => path.clone().prop_map(Op::Write),
            1 => path.clone().prop_map(Op::Delete),
            1 => Just(Op::Checkout),
            5 => Just(Op::Deliver),
            1 => Just(Op::Overflow),
            1 => Just(Op::WatchLost),
            1 => Just(Op::WatchRestored),
            4 => Just(Op::Reconcile),
            4 => (id.clone(), path).prop_map(|(i, p)| Op::Start(i, p)),
            2 => id.clone().prop_map(Op::Recookie),
            1 => id.clone().prop_map(Op::CookieLost),
            4 => id.clone().prop_map(Op::Pin),
            2 => id.prop_map(Op::Finish),
            1 => Just(Op::SyncStart),
            1 => Just(Op::SyncDone),
        ]
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Phase {
        Waiting,
        Pinned,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct RefRequest {
        phase: Phase,
        need: u64,
        cookie: Cookie,
        cookie_at: u64,
        cookies: u8,
        /// The disk history entry current when the request started.
        since: usize,
    }

    /// The disk: each path's version (`None`: absent) and `HEAD`'s.
    pub(crate) type Disk = (BTreeMap<usize, Option<u64>>, u64);

    /// The reference machine: what the registry should do, written from `formal/views.qnt` and ADR
    /// 0037 without the registry's code.
    #[derive(Debug, Clone)]
    pub(crate) struct Reference {
        generation: u64,
        watched: bool,
        certain: bool,
        dirty: BTreeSet<usize>,
        rescan: bool,
        index: BTreeMap<usize, Option<u64>>,
        git: u64,
        latest_read: Option<u64>,
        requests: BTreeMap<u64, RefRequest>,
        sync_waiting: bool,
        sync_scanned: bool,
    }

    pub(crate) struct World {
        disk: Disk,
        history: Vec<Disk>,
        events: VecDeque<Event>,
        clock: u64,
        next_version: u64,
        registry: Registry,
        reference: Reference,
    }

    pub(crate) struct All;

    impl Stored<u64> for All {
        fn holds(&self, _: &u64) -> bool {
            true
        }
    }

    pub(crate) fn uri(path: &str) -> FileUri {
        FileUri::from_path(path).unwrap()
    }

    pub(crate) fn rel(path: usize) -> RelPath {
        RelPath::new(PATHS[path]).unwrap()
    }

    pub(crate) fn index_of(path: &RelPath) -> usize {
        PATHS.iter().position(|p| *p == path.as_str()).unwrap()
    }

    /// What a stat sees of version `v`: never racy (the clock reads long after every write).
    pub(crate) fn seen(version: Option<u64>) -> FileSeen {
        FileSeen {
            stat: version.map(|v| FileStat {
                size: v,
                mtime: MTime {
                    secs: i64::try_from(v).unwrap(),
                    nanos: 0,
                },
                inode: 1,
            }),
            racy: false,
        }
    }

    impl World {
        pub(crate) fn new() -> Self {
            let files: BTreeMap<usize, Option<u64>> =
                (0..PATHS.len()).map(|p| (p, Some(1))).collect();
            let disk = (files.clone(), 1);
            let mut registry = Registry::new();
            let git = GitSeen {
                head: seen(Some(1)),
                index: seen(Some(1)),
            };
            registry
                .open(
                    uri(ROOT),
                    true,
                    git,
                    (0..PATHS.len()).map(|p| (rel(p), seen(Some(1)), 1)),
                )
                .unwrap();
            Self {
                history: vec![disk.clone()],
                disk,
                events: VecDeque::new(),
                clock: 1,
                next_version: 2,
                registry,
                reference: Reference {
                    generation: 1,
                    watched: true,
                    certain: true,
                    dirty: BTreeSet::new(),
                    rescan: false,
                    index: files,
                    git: 1,
                    latest_read: None,
                    requests: BTreeMap::new(),
                    sync_waiting: false,
                    sync_scanned: false,
                },
            }
        }

        fn tick(&mut self) -> u64 {
            self.clock = self.clock.saturating_add(1);
            self.clock
        }

        fn git_seen(&self) -> GitSeen<FileSeen> {
            GitSeen {
                head: seen(Some(self.disk.1)),
                index: seen(Some(1)),
            }
        }

        fn changed(&mut self, path: Option<usize>) {
            self.history.push(self.disk.clone());
            if let Some(path) = path
                && self.reference.watched
            {
                self.events.push_back(Event::File {
                    view: uri(ROOT),
                    path: rel(path),
                });
            }
        }

        /// Applies `op` to both; an error names the disagreement.
        pub(crate) fn step(&mut self, op: &Op) -> Result<(), String> {
            let r = &mut self.reference;
            match *op {
                Op::Write(p) => {
                    let v = self.next_version;
                    self.next_version = self.next_version.saturating_add(1);
                    self.disk.0.insert(p, Some(v));
                    self.changed(Some(p));
                }
                Op::Delete(p) => {
                    self.disk.0.insert(p, None);
                    self.changed(Some(p));
                }
                Op::Checkout => {
                    self.disk.1 = self.disk.1.saturating_add(1);
                    self.changed(None);
                }
                Op::Deliver => {
                    let Some(event) = self.events.pop_front() else {
                        return Ok(());
                    };
                    let expect = match &event {
                        Event::File { path, .. } => {
                            r.dirty.insert(index_of(path));
                            true
                        }
                        Event::Cookie { id, at } => match r.requests.get_mut(&id.0) {
                            Some(req) => {
                                if req.cookie == Cookie::Queued && req.cookie_at == at.0 {
                                    req.cookie = Cookie::Delivered;
                                }
                                true
                            }
                            None => false,
                        },
                        Event::Git { .. } => true,
                    };
                    agree("deliver", expect, self.registry.deliver(event).is_ok())?;
                }
                Op::Overflow => {
                    self.events.clear();
                    self.registry.overflow();
                    r.rescan = true;
                    r.certain = false;
                    for (id, req) in &mut r.requests {
                        if req.cookie == Cookie::Queued {
                            req.cookie = Cookie::Lost;
                            self.registry.cookie_lost(RequestId(*id)).unwrap();
                        }
                    }
                }
                Op::WatchLost => {
                    let expect = r.watched;
                    if expect {
                        r.watched = false;
                        r.certain = false;
                        r.rescan = true;
                    }
                    agree(
                        "watch_lost",
                        expect,
                        self.registry.watch_lost(&uri(ROOT)).is_ok(),
                    )?;
                }
                Op::WatchRestored => {
                    let expect = !r.watched;
                    if expect {
                        r.watched = true;
                        r.rescan = true;
                    }
                    agree(
                        "watch_restored",
                        expect,
                        self.registry.watch_restored(&uri(ROOT)).is_ok(),
                    )?;
                }
                Op::Reconcile => return self.reconcile(),
                Op::Start(id, p) => return self.start(id, p),
                Op::Recookie(id) => {
                    let now = self.tick();
                    let r = &mut self.reference;
                    let due = r.requests.get(&id).is_some_and(|q| {
                        q.phase == Phase::Waiting
                            && q.cookie == Cookie::Delivered
                            && q.cookies < 2
                            && r.watched
                            && r.latest_read > Some(q.cookie_at)
                    });
                    if due && let Some(q) = r.requests.get_mut(&id) {
                        q.cookie = Cookie::Queued;
                        q.cookie_at = now;
                        q.cookies = q.cookies.saturating_add(1);
                        self.events.push_back(Event::Cookie {
                            id: RequestId(id),
                            at: Stamp(now),
                        });
                    }
                    agree(
                        "request_recookie",
                        due,
                        self.registry
                            .request_recookie(RequestId(id), Stamp(now))
                            .is_ok(),
                    )?;
                }
                Op::CookieLost(id) => {
                    let expect = match r.requests.get_mut(&id) {
                        Some(q) => {
                            q.cookie = Cookie::Lost;
                            true
                        }
                        None => false,
                    };
                    agree(
                        "cookie_lost",
                        expect,
                        self.registry.cookie_lost(RequestId(id)).is_ok(),
                    )?;
                }
                Op::Pin(id) => return self.pin(id),
                Op::Finish(id) => {
                    let expect = r
                        .requests
                        .get(&id)
                        .is_some_and(|q| q.phase == Phase::Pinned);
                    let finished = self.registry.finish(RequestId(id)).is_ok()
                        && self.registry.forget(RequestId(id)).is_ok();
                    if expect {
                        r.requests.remove(&id);
                    }
                    agree("finish", expect, finished)?;
                }
                Op::SyncStart => {
                    let expect = !r.sync_waiting;
                    if expect {
                        r.sync_waiting = true;
                        r.sync_scanned = false;
                        r.rescan = true;
                    }
                    agree(
                        "sync_start",
                        expect,
                        self.registry.sync_start(&uri(ROOT)).is_ok(),
                    )?;
                }
                Op::SyncDone => {
                    let expect = r.sync_waiting && r.sync_scanned;
                    if expect {
                        r.sync_waiting = false;
                    }
                    agree(
                        "sync_done",
                        expect,
                        self.registry.sync_done(&uri(ROOT)).is_ok(),
                    )?;
                }
            }
            Ok(())
        }

        fn reconcile(&mut self) -> Result<(), String> {
            let r = &mut self.reference;
            let due = r.rescan || !r.dirty.is_empty();
            let disk = self.disk.clone();
            let git = self.git_seen();
            let listed: BTreeSet<RelPath> = disk
                .0
                .iter()
                .filter(|(_, v)| v.is_some())
                .map(|(p, _)| rel(*p))
                .collect();
            let started = self.registry.reconcile_start(
                &uri(ROOT),
                Some(&listed),
                |p| seen(disk.0.get(&index_of(p)).copied().flatten()),
                || git,
            );
            let todo = match (due, started) {
                (false, Err(_)) => return Ok(()),
                (true, Ok(todo)) => todo,
                (due, started) => {
                    return Err(format!("reconcile_start: due {due}, got {started:?}"));
                }
            };
            let r = &mut self.reference;
            let mut expected: BTreeSet<usize> = core::mem::take(&mut r.dirty);
            let rescan = r.rescan;
            if rescan {
                for (p, v) in &disk.0 {
                    if r.index.get(p) != Some(v) && (v.is_some() || r.index.contains_key(p)) {
                        expected.insert(*p);
                    }
                }
            }
            let got: BTreeSet<usize> = todo.iter().map(index_of).collect();
            if got != expected {
                return Err(format!(
                    "reconcile todo: expected {expected:?}, got {got:?}"
                ));
            }
            for path in todo {
                let p = index_of(&path);
                let v = disk.0.get(&p).copied().flatten();
                let stamp = self.tick();
                self.registry
                    .summarised(&uri(ROOT), &path, seen(v), v.unwrap_or(0), Stamp(stamp))
                    .map_err(|e| format!("summarised: {e}"))?;
                let r = &mut self.reference;
                r.index.insert(p, v);
                r.latest_read = Some(stamp);
            }
            let generation = self
                .registry
                .publish(&uri(ROOT), &All)
                .map_err(|e| format!("publish: {e}"))?;
            let r = &mut self.reference;
            r.generation = r.generation.saturating_add(1);
            if rescan {
                r.git = disk.1;
                r.certain |= r.watched;
                if r.sync_waiting {
                    r.sync_scanned = true;
                }
            }
            r.rescan = false;
            agree("generation", true, generation.0 == r.generation)
        }

        fn start(&mut self, id: u64, p: usize) -> Result<(), String> {
            let now = self.tick();
            let file = uri(&format!("{ROOT}/{}", PATHS[p]));
            let disk_v = self.disk.0.get(&p).copied().flatten();
            let git = self.git_seen();
            let got =
                self.registry
                    .request_start(RequestId(id), &file, &seen(disk_v), &git, Stamp(now));
            let r = &mut self.reference;
            let busy = r.requests.contains_key(&id);
            if busy {
                return agree("request_start", false, got.is_ok());
            }
            let file_fresh = r.index.get(&p) == Some(&disk_v);
            let git_fresh = r.git == self.disk.1;
            let need = if file_fresh && git_fresh {
                r.generation
            } else {
                r.generation.saturating_add(1)
            };
            if !file_fresh {
                r.dirty.insert(p);
            }
            if !git_fresh {
                r.rescan = true;
            }
            let cookie = if r.watched {
                self.events.push_back(Event::Cookie {
                    id: RequestId(id),
                    at: Stamp(now),
                });
                Cookie::Queued
            } else {
                Cookie::None
            };
            r.requests.insert(
                id,
                RefRequest {
                    phase: Phase::Waiting,
                    need,
                    cookie,
                    cookie_at: now,
                    cookies: u8::from(cookie == Cookie::Queued),
                    since: self.history.len().saturating_sub(1),
                },
            );
            agree("request_start", true, got.is_ok())
        }

        fn pin(&mut self, id: u64) -> Result<(), String> {
            let git = self.git_seen();
            let got = self
                .registry
                .request_pin(RequestId(id), &git)
                .map(|pin| (pin.completeness.is_complete(), pin.generation.clone()));
            let r = &mut self.reference;
            let Some(q) = r.requests.get(&id).cloned() else {
                return agree("request_pin", false, got.is_ok());
            };
            let enabled =
                q.phase == Phase::Waiting && r.generation >= q.need && q.cookie != Cookie::Queued;
            if !enabled {
                return agree("request_pin", false, got.is_ok());
            }
            let restat = r.git == self.disk.1;
            let complete = q.cookie == Cookie::Delivered
                && r.latest_read <= Some(q.cookie_at)
                && r.watched
                && r.certain
                && r.dirty.is_empty()
                && !r.rescan
                && restat;
            if !restat {
                r.rescan = true;
            }
            if let Some(req) = r.requests.get_mut(&id) {
                req.phase = Phase::Pinned;
            }
            let (is_complete, generation) = got.map_err(|e| format!("request_pin refused: {e}"))?;
            agree("completeness", complete, is_complete)?;
            if is_complete {
                let pinned: BTreeMap<usize, Option<u64>> = (0..PATHS.len())
                    .map(|p| {
                        let v = generation
                            .index
                            .get(&rel(p))
                            .and_then(|e| e.seen.stat.map(|s| s.size));
                        (p, v)
                    })
                    .collect();
                let snapshot = self.history[q.since..]
                    .iter()
                    .any(|(files, _)| *files == pinned);
                if !snapshot || generation.git.head != seen(Some(self.disk.1)) {
                    return Err(format!(
                        "complete but no snapshot: pinned {pinned:?}, history {:?}",
                        &self.history[q.since..]
                    ));
                }
            }
            Ok(())
        }
    }

    pub(crate) fn agree(what: &str, expected: bool, got: bool) -> Result<(), String> {
        if expected == got {
            Ok(())
        } else {
            Err(format!(
                "{what}: the reference says {expected}, the registry {got}"
            ))
        }
    }

    pub(crate) fn config() -> ProptestConfig {
        ProptestConfig {
            cases: 512,
            rng_seed: RngSeed::Fixed(SEED),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/views_machine.txt",
            ))),
            ..ProptestConfig::default()
        }
    }
}
use machine::{World, config, late_read, ops};

#[test]
fn should_refuse_complete_when_a_reconcile_read_after_the_cookie_published_a_mix() {
    // given
    let mut world = World::new();
    let steps = late_read(1, 2, 0, 1);

    // when
    let disagreement = steps.iter().enumerate().find_map(|(n, op)| {
        world
            .step(op)
            .err()
            .map(|why| format!("step {n} ({op:?}): {why}"))
    });

    // then
    assert_eq!(disagreement, None);
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_agree_with_the_reference_machine_when_operations_are_random(
        ops in ops()
    ) {
        // given
        let mut world = World::new();

        // when
        let disagreement = ops
            .iter()
            .enumerate()
            .find_map(|(n, op)| world.step(op).err().map(|why| format!("step {n} ({op:?}): {why}")));

        // then
        prop_assert!(disagreement.is_none(), "{}", disagreement.unwrap_or_default());
    }
}
