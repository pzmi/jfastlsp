//! The diagnostics coordinator against a reference machine (ADR 0007, ver §6.4; LSP-04's Accept
//! item 3), over plain proptest (not `proptest-state-machine`: IDX-09's and IDX-12's decision,
//! whose default features add crates against the std-only proptest of `architecture.toml`).
//!
//! A generated sequence of the model's stimuli (open, edit, Claude Code's Edit tool, disk writes
//! and deletes, close, the server noticing the disk, compiles started, finished and read, worker
//! spawns, crashes and timeouts, in any order) drives the coordinator, with the testkit's scripted
//! worker, and a small reference machine side by side: `formal/diagnostics.qnt`'s transitions
//! restated over integer texts. After each step they must agree on whether the step was taken,
//! on every document's session, version, epoch and pending compile, and on what the client shows;
//! and the reference's state must hold the model's four invariants (so a reference that drifts
//! from the model fails too).
//!
//! **`EventuallyUpToDate`'s reduction** (`doc/formal-models.md`, "Liveness"): once the random
//! prefix stops (edits and faults with it), only system moves run (noticing the disk, spawning
//! the worker, starting, finishing and reading compiles), at most [`RECOVERY`] of them; then
//! nothing may be enabled and every open document must show diagnostics of its current inputs.
//!
//! Seeded (ver §3): every run starts from [`SEED`]; proptest writes a shrunk failure to
//! `proptest-regressions/`, which is committed.

#[cfg(test)]
mod machine {
    use jfl_application::diagnostics::{Coordinator, Delivery, DiagnosticsError, JobId, Text};
    use jfl_testkit::fakes::compiler::{Echo, JobState, ScriptedWorker};
    use proptest::prelude::*;
    use proptest::test_runner::{FileFailurePersistence, RngSeed};

    /// The fixed seed of every property here (ver §3 rule 2).
    pub(crate) const SEED: u64 = 0x15B0_0004;
    /// System moves allowed after the prefix before every open document must be up to date.
    pub(crate) const RECOVERY: usize = 60;
    const SLOTS: usize = 3;
    /// `a` (0) reads `b` (1) and `c` (2); `b` reads itself. The client opens `a` and `b`.
    const NAMES: [&str; 3] = ["a", "b", "c"];

    #[derive(Debug, Clone, Copy)]
    pub(crate) enum Op {
        Open(usize, i32),
        Change(usize),
        ToolEdit(usize),
        DiskWrite(usize),
        DiskDelete,
        Close(usize),
        Notice(usize),
        Start(usize),
        Complete(usize),
        Deliver(usize),
        Restart,
        Crash,
        Timeout(usize),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Shown {
        kind: u8, // 0 none, 1 cleared, 2 diags
        version: Option<i32>,
        session: u32,
        epoch: u64,
        inputs: [i64; 3],
        inc: u32,
    }

    const NONE: Shown = Shown {
        kind: 0,
        version: None,
        session: 0,
        epoch: 0,
        inputs: [0; 3],
        inc: 0,
    };
    const CLEARED: Shown = Shown { kind: 1, ..NONE };

    #[derive(Debug, Clone, Copy, Default)]
    struct RDoc {
        open: bool,
        session: u32,
        version: i32,
        overlay: i64,
        seen: i64,
        epoch: u64,
        dirty: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct RJob {
        done: bool,
        doc: usize,
        session: u32,
        version: Option<i32>,
        epoch: u64,
        inputs: [i64; 3],
        inc: u32,
        late: bool,
    }

    /// The model, restated: integer texts, a larger one newer.
    #[derive(Debug, Clone)]
    struct Reference {
        disk: [i64; 3],
        seen_c: i64,
        docs: [RDoc; 2],
        up: bool,
        inc: u32,
        jobs: [Option<RJob>; SLOTS],
        client: [Shown; 2],
        high: [(i64, i64); 2],
        since_edit: [bool; 2],
        sent: Vec<(usize, u32, i32, i64)>,
        next_text: i64,
    }

    impl Reference {
        fn new() -> Self {
            let mut docs = [RDoc::default(); 2];
            docs[0].seen = 1;
            docs[1].seen = 2;
            Self {
                disk: [1, 2, 0],
                seen_c: 0,
                docs,
                up: false,
                inc: 0,
                jobs: [None; SLOTS],
                client: [NONE; 2],
                high: [(-1, -1); 2],
                since_edit: [false; 2],
                sent: Vec::new(),
                next_text: 3,
            }
        }

        fn eff(&self, d: usize) -> i64 {
            let x = self.docs[d];
            if x.open && x.overlay >= x.seen {
                x.overlay
            } else {
                x.seen
            }
        }

        fn inputs(&self, d: usize) -> [i64; 3] {
            if d == 0 {
                [self.eff(0), self.eff(1), self.seen_c]
            } else {
                [0, self.eff(1), 0]
            }
        }

        fn tag(&self, d: usize) -> Option<i32> {
            let x = self.docs[d];
            (x.overlay >= x.seen).then_some(x.version)
        }

        fn current(&self, k: &RJob) -> bool {
            let x = self.docs[k.doc];
            x.open && k.session == x.session && k.epoch == x.epoch
        }

        /// Applies `edit`, then bumps every document whose inputs changed, and `forced`.
        fn change(&mut self, forced: Option<usize>, edit: impl FnOnce(&mut Self)) {
            let before = [self.inputs(0), self.inputs(1)];
            edit(self);
            for (d, was) in before.iter().enumerate() {
                if forced == Some(d) || self.inputs(d) != *was {
                    self.docs[d].epoch = self.docs[d].epoch.saturating_add(1);
                    self.docs[d].dirty = self.docs[d].open;
                    self.since_edit[d] = false;
                }
            }
        }

        fn publish(&mut self, d: usize, shown: Shown) {
            let p = self.client[d];
            if p.kind == 2 {
                let (e, v) = self.high[d];
                self.high[d] = (
                    e.max(p.epoch as i64),
                    v.max(p.version.map_or(-1, i64::from)),
                );
            }
            self.client[d] = shown;
            self.since_edit[d] = true;
        }

        fn free(&self) -> Option<usize> {
            self.jobs.iter().position(Option::is_none)
        }

        fn needs_worker(&self) -> bool {
            !self.up && self.docs.iter().any(|x| x.open && x.dirty)
        }

        /// Whether the model enables `op`.
        fn enabled(&self, op: Op) -> bool {
            match op {
                Op::Open(d, _) => !self.docs[d].open,
                Op::Change(d) | Op::ToolEdit(d) | Op::Close(d) => self.docs[d].open,
                Op::DiskWrite(_) => true,
                Op::DiskDelete => self.disk[2] != 0,
                Op::Notice(f) => {
                    if f == 2 {
                        self.disk[2] != self.seen_c
                    } else {
                        self.disk[f] != self.docs[f].seen
                    }
                }
                Op::Start(d) => {
                    self.docs[d].open && self.docs[d].dirty && self.up && self.free().is_some()
                }
                Op::Complete(j) => {
                    self.jobs[j].is_some_and(|k| !k.done && self.up && k.inc == self.inc)
                }
                Op::Deliver(j) => self.jobs[j].is_some_and(|k| k.done),
                Op::Restart => self.needs_worker(),
                Op::Crash => self.up,
                Op::Timeout(j) => {
                    self.jobs[j].is_some_and(|k| !k.done && !k.late && self.up && k.inc == self.inc)
                }
            }
        }

        /// Takes `op`, which the model enables.
        fn apply(&mut self, op: Op) {
            match op {
                Op::Open(d, v) => {
                    let text = self.disk[d];
                    self.change(Some(d), |m| {
                        let x = &mut m.docs[d];
                        x.open = true;
                        x.session = x.session.saturating_add(1);
                        x.version = v;
                        x.overlay = text;
                    });
                    self.high[d] = (-1, -1);
                    self.sent.push((d, self.docs[d].session, v, text));
                }
                Op::Change(d) | Op::ToolEdit(d) => {
                    let n = self.next_text;
                    self.next_text = self.next_text.saturating_add(1);
                    if matches!(op, Op::ToolEdit(_)) {
                        self.disk[d] = n;
                    }
                    self.change(None, |m| {
                        m.docs[d].version = m.docs[d].version.saturating_add(1);
                        m.docs[d].overlay = n;
                    });
                    self.sent
                        .push((d, self.docs[d].session, self.docs[d].version, n));
                }
                Op::DiskWrite(f) => {
                    self.disk[f] = self.next_text;
                    self.next_text = self.next_text.saturating_add(1);
                }
                Op::DiskDelete => self.disk[2] = 0,
                Op::Close(d) => {
                    self.change(None, |m| {
                        m.docs[d].open = false;
                        m.docs[d].overlay = 0;
                        m.docs[d].dirty = false;
                    });
                    self.client[d] = CLEARED;
                }
                Op::Notice(f) => {
                    let text = self.disk[f];
                    self.change(None, |m| {
                        if f == 2 {
                            m.seen_c = text
                        } else {
                            m.docs[f].seen = text
                        }
                    });
                }
                Op::Start(d) => {
                    let job = RJob {
                        done: false,
                        doc: d,
                        session: self.docs[d].session,
                        version: self.tag(d),
                        epoch: self.docs[d].epoch,
                        inputs: self.inputs(d),
                        inc: self.inc,
                        late: false,
                    };
                    if let Some(j) = self.free() {
                        self.jobs[j] = Some(job);
                    }
                    self.docs[d].dirty = false;
                }
                Op::Complete(j) => {
                    if let Some(k) = self.jobs[j].as_mut() {
                        k.done = true;
                    }
                }
                Op::Deliver(j) => {
                    if let Some(k) = self.jobs[j].take()
                        && self.current(&k)
                    {
                        let shown = Shown {
                            kind: 2,
                            version: k.version,
                            session: k.session,
                            epoch: k.epoch,
                            inputs: k.inputs,
                            inc: k.inc,
                        };
                        self.publish(k.doc, shown);
                    }
                }
                Op::Restart => {
                    self.up = true;
                    self.inc = self.inc.saturating_add(1);
                }
                Op::Crash => {
                    self.up = false;
                    for j in 0..SLOTS {
                        if let Some(k) = self.jobs[j]
                            && !k.done
                            && k.inc == self.inc
                        {
                            if self.current(&k) {
                                self.docs[k.doc].dirty = true;
                            }
                            self.jobs[j] = None;
                        }
                    }
                }
                Op::Timeout(j) => {
                    if let Some(k) = self.jobs[j] {
                        self.jobs[j] = Some(RJob { late: true, ..k });
                        if self.current(&k) {
                            self.docs[k.doc].dirty = true;
                        }
                    }
                }
            }
        }

        fn up_to_date(&self, d: usize) -> bool {
            let p = self.client[d];
            let x = self.docs[d];
            p.kind == 2
                && p.session == x.session
                && p.epoch == x.epoch
                && p.inputs == self.inputs(d)
        }

        /// The model's `allInvariants`.
        fn invariants(&self) -> Result<(), String> {
            for d in 0..2 {
                let (p, x, (he, hv)) = (self.client[d], self.docs[d], self.high[d]);
                let v = p.version.map_or(-1, i64::from);
                if p.kind == 2 && (p.epoch as i64) < he {
                    return Err(format!("NeverOlderOverNewer (epoch) of {d}"));
                }
                if p.kind == 2 && p.version.is_some() && v < hv {
                    return Err(format!("NeverOlderOverNewer (version) of {d}"));
                }
                if x.open
                    && p.kind == 2
                    && let Some(v) = p.version
                    && !self.sent.contains(&(d, x.session, v, p.inputs[d]))
                {
                    return Err(format!("PublishedMatchesVersion of {d}"));
                }
                if !x.open && p.kind != if x.session == 0 { 0 } else { 1 } {
                    return Err(format!("ClosedPublishesNothing of {d}"));
                }
                if x.open && self.since_edit[d] && !self.up_to_date(d) {
                    return Err(format!("FreshAfterEdit of {d}"));
                }
            }
            Ok(())
        }
    }

    /// The coordinator, its scripted worker and client, and the reference, side by side.
    pub(crate) struct World {
        reference: Reference,
        coordinator: Coordinator<String>,
        worker: ScriptedWorker,
        slots: [Option<JobId>; SLOTS],
        client: [Shown; 2],
    }

    fn text(n: i64) -> Text {
        Text::new(u64::try_from(n).unwrap_or_default(), n.to_string())
    }

    fn name(i: usize) -> String {
        NAMES[i].to_owned()
    }

    impl World {
        pub(crate) fn new() -> Self {
            let mut coordinator = Coordinator::new(SLOTS, [(name(0), text(1)), (name(1), text(2))]);
            let _ = coordinator.track(name(0), [name(1), name(2)]);
            let _ = coordinator.track(name(1), []);
            Self {
                reference: Reference::new(),
                coordinator,
                worker: ScriptedWorker::new(),
                slots: [None; SLOTS],
                client: [NONE; 2],
            }
        }

        /// Takes `op` on both sides when the model enables it; when it does not, the coordinator
        /// must refuse it too (where the coordinator guards it). Then compares.
        pub(crate) fn step(&mut self, op: Op) -> Result<(), String> {
            if !self.reference.enabled(op) {
                return self.refuses(op);
            }
            self.apply(op)?;
            self.reference.apply(op);
            self.reference.invariants()?;
            self.compare()
        }

        fn refuses(&mut self, op: Op) -> Result<(), String> {
            let refused = match op {
                Op::Open(d, v) => {
                    self.coordinator
                        .did_open(name(d), v, text(self.reference.disk[d]))
                        == Err(DiagnosticsError::AlreadyOpen)
                }
                Op::Change(d) | Op::ToolEdit(d) => {
                    self.coordinator.did_change(name(d), 1, text(99))
                        == Err(DiagnosticsError::NotOpen)
                }
                Op::Close(d) => self.coordinator.did_close(name(d)).is_err(),
                Op::Start(d) => self.coordinator.start(&name(d)).is_err(),
                Op::Restart => !self.coordinator.needs_worker(),
                _ => true,
            };
            if refused {
                self.compare()
            } else {
                Err("the coordinator took a step the model does not enable".to_owned())
            }
        }

        fn job(&self, j: usize) -> Result<JobId, String> {
            self.slots[j].ok_or_else(|| format!("slot {j} holds no job"))
        }

        fn apply(&mut self, op: Op) -> Result<(), String> {
            let r = &self.reference;
            match op {
                Op::Open(d, v) => {
                    let _ = self
                        .coordinator
                        .did_open(name(d), v, text(r.disk[d]))
                        .map_err(|e| e.to_string())?;
                }
                Op::Change(d) | Op::ToolEdit(d) => {
                    let v = r.docs[d].version.saturating_add(1);
                    let _ = self
                        .coordinator
                        .did_change(name(d), v, text(r.next_text))
                        .map_err(|e| e.to_string())?;
                }
                Op::DiskWrite(_) | Op::DiskDelete => {}
                Op::Close(d) => {
                    let closed = self
                        .coordinator
                        .did_close(name(d))
                        .map_err(|e| e.to_string())?;
                    if closed.clear != name(d) {
                        return Err("the close clears another document".to_owned());
                    }
                    self.client[d] = CLEARED;
                }
                Op::Notice(f) => {
                    let n = r.disk[f];
                    let _ = self
                        .coordinator
                        .see_disk(name(f), (n != 0).then(|| text(n)));
                }
                Op::Start(d) => {
                    let job = self
                        .coordinator
                        .start(&name(d))
                        .map_err(|e| e.to_string())?;
                    self.worker.submit(&job)?;
                    let free = self.slots.iter().position(Option::is_none);
                    let slot = free.ok_or("no free slot")?;
                    self.slots[slot] = Some(job.id);
                }
                Op::Complete(j) => self.worker.complete(self.job(j)?)?,
                Op::Deliver(j) => {
                    let reply = self.worker.take(self.job(j)?).ok_or("no reply written")?;
                    self.slots[j] = None;
                    match self.coordinator.deliver(reply) {
                        Delivery::Publish(p) => {
                            let d = NAMES.iter().position(|n| *n == p.document).ok_or("doc")?;
                            let echo = p.diagnostics.first().and_then(|m| Echo::parse(m));
                            let echo = echo.ok_or("an unreadable reply")?;
                            let read = |f: &str| {
                                echo.inputs
                                    .get(f)
                                    .cloned()
                                    .flatten()
                                    .and_then(|t| t.parse::<i64>().ok())
                                    .unwrap_or(0)
                            };
                            self.client[d] = Shown {
                                kind: 2,
                                version: p.version,
                                session: echo.session,
                                epoch: echo.epoch,
                                inputs: [read("a"), read("b"), read("c")],
                                inc: echo.incarnation,
                            };
                        }
                        Delivery::Stale(_) => {}
                        Delivery::Unknown(_) => return Err("an unknown job".to_owned()),
                    }
                }
                Op::Restart => {
                    let inc = self.worker.spawn()?;
                    self.coordinator
                        .worker_started(inc)
                        .map_err(|e| e.to_string())?;
                }
                Op::Crash => {
                    let lost = self.worker.crash()?;
                    for slot in &mut self.slots {
                        if slot.is_some_and(|id| lost.contains(&id)) {
                            *slot = None;
                        }
                    }
                    self.coordinator
                        .worker_lost(lost)
                        .map_err(|e| e.to_string())?;
                }
                Op::Timeout(j) => {
                    let id = self.job(j)?;
                    if self.worker.state(id) != JobState::Running {
                        return Err("timing out a job that is not running".to_owned());
                    }
                    self.coordinator.timed_out(id).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }

        fn compare(&self) -> Result<(), String> {
            let r = &self.reference;
            for (d, doc) in NAMES.iter().take(2).enumerate() {
                let x = r.docs[d];
                let got = self.coordinator.document(&name(d)).ok_or("untracked")?;
                let mine = (got.open, got.session.0, got.version, got.epoch.0, got.dirty);
                let theirs = (x.open, x.session, x.version, x.epoch, x.dirty);
                if mine != theirs {
                    return Err(format!(
                        "{doc}: (open, session, version, epoch, dirty) is {mine:?}, the reference's {theirs:?}"
                    ));
                }
                if self.client[d] != r.client[d] {
                    return Err(format!(
                        "{doc} shows {:?}, the reference {:?}",
                        self.client[d], r.client[d]
                    ));
                }
            }
            if self.coordinator.needs_worker() != r.needs_worker() {
                return Err("they disagree on whether a worker is needed".to_owned());
            }
            Ok(())
        }

        /// The first enabled system move, in a fixed order: notice the disk, spawn, read a reply,
        /// finish a compile, start one.
        fn system_move(&self) -> Option<Op> {
            let r = &self.reference;
            let moves = (0..3)
                .map(Op::Notice)
                .chain([Op::Restart])
                .chain((0..SLOTS).map(Op::Deliver))
                .chain((0..SLOTS).map(Op::Complete))
                .chain((0..2).map(Op::Start));
            moves.into_iter().find(|op| r.enabled(*op))
        }

        /// `EventuallyUpToDate`'s reduction: after at most [`RECOVERY`] system moves nothing is
        /// enabled and every open document is up to date.
        pub(crate) fn recover(&mut self) -> Result<(), String> {
            for _ in 0..RECOVERY {
                let Some(op) = self.system_move() else {
                    break;
                };
                self.step(op)
                    .map_err(|why| format!("recovery {op:?}: {why}"))?;
            }
            if let Some(op) = self.system_move() {
                return Err(format!(
                    "{op:?} still enabled after {RECOVERY} system moves"
                ));
            }
            for (d, doc) in NAMES.iter().take(2).enumerate() {
                let x = self.reference.docs[d];
                let p = self.client[d];
                let up_to_date = p.kind == 2
                    && p.session == x.session
                    && p.epoch == x.epoch
                    && p.inputs == self.reference.inputs(d);
                if x.open && !up_to_date {
                    return Err(format!("{doc} is open and not up to date: {p:?}"));
                }
            }
            Ok(())
        }
    }

    fn op() -> impl Strategy<Value = Op> {
        let doc = 0..2_usize;
        let file = 0..3_usize;
        let slot = 0..SLOTS;
        prop_oneof![
            (doc.clone(), 1..4_i32).prop_map(|(d, v)| Op::Open(d, v)),
            doc.clone().prop_map(Op::Change),
            doc.clone().prop_map(Op::ToolEdit),
            file.clone().prop_map(Op::DiskWrite),
            Just(Op::DiskDelete),
            doc.clone().prop_map(Op::Close),
            file.prop_map(Op::Notice),
            doc.prop_map(Op::Start),
            slot.clone().prop_map(Op::Complete),
            slot.clone().prop_map(Op::Deliver),
            Just(Op::Restart),
            Just(Op::Crash),
            slot.prop_map(Op::Timeout),
        ]
    }

    pub(crate) fn ops() -> impl Strategy<Value = Vec<Op>> {
        proptest::collection::vec(op(), 1..60)
    }

    pub(crate) fn config() -> ProptestConfig {
        ProptestConfig {
            cases: 1024,
            rng_seed: RngSeed::Fixed(SEED),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/diagnostics_machine.txt",
            ))),
            ..ProptestConfig::default()
        }
    }
}

#[cfg(test)]
use machine::{Op, World, config, ops};
#[cfg(test)]
use proptest::prelude::*;

#[cfg(test)]
proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_agree_with_the_reference_and_bring_every_open_document_up_to_date_when_steps_are_random(
        ops in ops()
    ) {
        // given
        let mut world = World::new();

        // when
        let disagreement = ops
            .iter()
            .enumerate()
            .find_map(|(n, op)| world.step(*op).err().map(|why| format!("step {n} ({op:?}): {why}")))
            .or_else(|| world.recover().err());

        // then
        prop_assert!(disagreement.is_none(), "{}", disagreement.unwrap_or_default());
    }
}

#[test]
fn should_publish_only_the_newest_when_two_quick_edits_reply_in_reverse_order() {
    // given: three versions compiled; the newest reply is read first (the model's scenario)
    let mut world = World::new();
    let script = [
        Op::Open(0, 1),
        Op::Restart,
        Op::Start(0),
        Op::Change(0),
        Op::Start(0),
        Op::Change(0),
        Op::Start(0),
        Op::Complete(2),
        Op::Complete(0),
        Op::Deliver(2),
        Op::Complete(1),
        Op::Deliver(1),
        Op::Deliver(0),
    ];

    // when
    let disagreement = script.iter().enumerate().find_map(|(n, op)| {
        world
            .step(*op)
            .err()
            .map(|why| format!("step {n} ({op:?}): {why}"))
    });

    // then: the coordinator agrees with the reference at every step, and the reference's
    // invariants (FreshAfterEdit, NeverOlderOverNewer) held throughout
    assert_eq!(disagreement, None);
    assert_eq!(world.recover(), Ok(()));
}
