//! The diagnostics coordinator (LSP-04; plan §3.2, §7.3; ADR 0007): which compile results are
//! published, when, and with which LSP version. The transitions of `formal/diagnostics.qnt`
//! (`doc/formal-models.md`, "`diagnostics`").
//!
//! This is the coordinator's core, and it does no I/O and never waits (plan §3.2, ADR 0015):
//! whoever drives it (the daemon, the `diagnostics` conformance driver, the tests) runs the
//! compiles, reads their replies, watches the disk and sends the publications, and tells the
//! coordinator what happened, one model action at a time (obligation 9). Each method is one
//! action of the model, with the guards the coordinator can see: a transition the model does not
//! enable is refused with a [`DiagnosticsError`], never applied. Two are the shell's to keep: a
//! job finished or still running is the worker's state (so [`Coordinator::timed_out`] trusts the
//! shell that the job runs), and seeing a disk text equal to the one seen before changes nothing.
//! A reply for a job the coordinator does not hold is [`Delivery::Unknown`], dropped.
//!
//! What it fixes (`doc/formal-models.md`, "LSP-04's obligations"):
//!
//! - **Results are keyed by (document, open session, input epoch), never by the LSP version**
//!   (obligation 1). A document's epoch is bumped whenever the winning text of any file its
//!   compile reads changes (its own, a dependency's, a file it looked up and did not find), and
//!   at every [`Coordinator::did_open`]; its session at every `didOpen`. Neither ever restarts, so
//!   a reply computed in an earlier session, or for a superseded input, never matches.
//! - **Every change makes a compile pending at once** (the document is `dirty`), so plan §3.2's
//!   rule (drop a result older than the newest published or pending one) becomes: a reply is
//!   published only when it was computed for its document's current epoch in its current session
//!   ([`Coordinator::deliver`], obligation 3), and every such reply is published, even when its
//!   diagnostics did not change.
//! - **The newer text wins, disk or overlay** (plan §2.5): an open document's overlay is what its
//!   compile reads while its [`Text::stamp`] is not older than the disk's the coordinator has
//!   seen; otherwise the disk's text is compiled and published without a version (obligation 4).
//!   A compile reads the winning text of every input, overlays included (obligation 7).
//! - **Only open documents get diagnostics.** [`Coordinator::did_close`] hands back the empty set
//!   to publish at once, and every later reply of that session is dropped (obligation 5).
//! - **A crash re-queues** every document whose current compile it lost, and a pending compile
//!   asks for a worker ([`Coordinator::needs_worker`]); **a timeout re-queues** its document and
//!   leaves the late job running (obligation 6).
//!
//! What a document's compile reads is [`Coordinator::track`]ed: by default the document itself
//! (the syntax pass), and for `javac` (SEM-05) what the job recorded it read, the negative
//! lookups included (obligation 2), so a change to any of it bumps the epoch.
//!
//! Pinned by: `jfl-conformance`'s `diagnostics` driver (`crates/jfl-conformance/src/diagnostics.rs`),
//! which replays every regression and generated trace of the model against this code, and
//! `tests/diagnostics_machine.rs` (a reference machine, ver §6.4).

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// An LSP document version (`integer` in LSP).
pub type Version = i32;

/// A text of a file, as the coordinator knows it: what an overlay or the disk held, with the time
/// it stands for.
///
/// The stamp orders an overlay against the disk (plan §2.5, §4.4): the daemon stamps a disk text
/// with its mtime, and an overlay with the file's mtime when the overlay arrived, so a disk
/// written after the overlay arrived wins, and a disk written before it (Claude Code's Edit tool
/// writes the file, then sends the same text) does not. Two texts are the same input when their
/// stamps and contents are equal.
#[derive(Clone)]
pub struct Text {
    stamp: u64,
    content: Arc<str>,
}

impl Text {
    /// A text with `stamp` holding `content`.
    pub fn new(stamp: u64, content: impl Into<Arc<str>>) -> Self {
        Self {
            stamp,
            content: content.into(),
        }
    }

    /// When the text was current: a larger stamp is newer.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The text.
    pub fn content(&self) -> &Arc<str> {
        &self.content
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.stamp == other.stamp
            && (Arc::ptr_eq(&self.content, &other.content) || self.content == other.content)
    }
}

impl Eq for Text {}

impl fmt::Debug for Text {
    // Never the content: a log or a test failure must not carry source text (fnd §5).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Text")
            .field("stamp", &self.stamp)
            .field("bytes", &self.content.len())
            .finish()
    }
}

/// A document's open session: its open count, 0 before the first `didOpen`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Session(pub u32);

/// A document's input epoch: bumped on every change of its compile's inputs and every `didOpen`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(pub u64);

/// A worker's incarnation: the first spawn is 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Incarnation(pub u32);

/// A compile job, unique for the coordinator's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// A document as the coordinator holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document<K> {
    /// The client has it open.
    pub open: bool,
    /// Its open session.
    pub session: Session,
    /// The client's latest version (kept after a close; 0 before the first open).
    pub version: Version,
    /// The client's text of that version, while open.
    pub overlay: Option<Text>,
    /// Its input epoch.
    pub epoch: Epoch,
    /// A compile of the current epoch is due and not started.
    pub dirty: bool,
    /// The files its compile reads, itself included.
    pub reads: BTreeSet<K>,
}

/// A compile the coordinator started: what it was started for and what it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job<K> {
    /// The job.
    pub id: JobId,
    /// The document compiled.
    pub document: K,
    /// Its session when the job started.
    pub session: Session,
    /// Its epoch when the job started.
    pub epoch: Epoch,
    /// The version its diagnostics are published with: the client's, when the overlay is what it
    /// compiles; `None` when the disk has overtaken the overlay (decision 4).
    pub version: Option<Version>,
    /// The winning text of every file it reads (`None`: absent, a negative lookup).
    pub inputs: BTreeMap<K, Option<Text>>,
    /// The worker incarnation that runs it.
    pub incarnation: Incarnation,
    /// It passed its deadline and was compiled again; its reply may still come.
    pub late: bool,
}

/// A worker's reply to a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply<D> {
    /// The job it answers.
    pub job: JobId,
    /// Its diagnostics: one epoch's whole set (obligation 8).
    pub diagnostics: D,
}

/// A `publishDiagnostics` to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication<K, D> {
    /// The document.
    pub document: K,
    /// The version whose text the diagnostics were computed from; `None` for disk content.
    pub version: Option<Version>,
    /// The diagnostics, replacing the client's.
    pub diagnostics: D,
}

/// What became of a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a publication must be sent"]
pub enum Delivery<K, D> {
    /// Computed for its document's current epoch in its current session: send it.
    Publish(Publication<K, D>),
    /// Stale (a newer epoch, another session, or closed): dropped. It holds what a coordinator
    /// that published every reply would send (plan §7.3's negative control).
    Stale(Publication<K, D>),
    /// A job the coordinator does not hold (read already, or lost): dropped.
    Unknown(Reply<D>),
}

/// The documents whose epoch a change bumped.
pub type Changed<K> = BTreeSet<K>;

/// What a `didClose` leaves to do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a close publishes the empty set at once"]
pub struct Closed<K> {
    /// The document whose diagnostics are cleared now: publish the empty set, without a version.
    pub clear: K,
    /// The documents whose epoch the close bumped (the document's own, when its overlay differed
    /// from the disk; its readers').
    pub changed: Changed<K>,
}

/// A transition the model does not enable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DiagnosticsError {
    /// `didOpen` of an open document.
    #[error("the document is open already")]
    AlreadyOpen,
    /// A change or a close of a document that is not open.
    #[error("the document is not open")]
    NotOpen,
    /// A compile of a document with none pending.
    #[error("no compile of the document is pending")]
    NotPending,
    /// A compile with no worker up.
    #[error("no worker is up")]
    WorkerDown,
    /// A spawn while a worker is up.
    #[error("a worker is up already")]
    WorkerUp,
    /// A compile with every job slot taken.
    #[error("every job slot is taken")]
    Busy,
    /// A job the coordinator does not hold, or one that may not time out.
    #[error("no such running job")]
    UnknownJob,
}

/// The coordinator of one worktree's documents, over file keys `K`.
#[derive(Debug, Clone)]
pub struct Coordinator<K> {
    capacity: usize,
    docs: BTreeMap<K, Document<K>>,
    seen: BTreeMap<K, Text>,
    readers: BTreeMap<K, BTreeSet<K>>,
    jobs: BTreeMap<JobId, Job<K>>,
    worker: Option<Incarnation>,
    next_job: u64,
}

impl<K: Ord + Clone> Coordinator<K> {
    /// A coordinator that runs at most `capacity` jobs at once (the worker's slots, at least one),
    /// with the disk texts it has seen so far.
    pub fn new(capacity: usize, disk: impl IntoIterator<Item = (K, Text)>) -> Self {
        Self {
            capacity: capacity.max(1),
            docs: BTreeMap::new(),
            seen: disk.into_iter().collect(),
            readers: BTreeMap::new(),
            jobs: BTreeMap::new(),
            worker: None,
            next_job: 0,
        }
    }

    /// The document `doc`, once tracked or opened.
    pub fn document(&self, doc: &K) -> Option<&Document<K>> {
        self.docs.get(doc)
    }

    /// The disk text of `file` the coordinator has seen; `None` while it has seen none, or the
    /// file absent.
    pub fn seen(&self, file: &K) -> Option<&Text> {
        self.seen.get(file)
    }

    /// The job `id`, from its start until its reply is read or a crash loses it.
    pub fn job(&self, id: JobId) -> Option<&Job<K>> {
        self.jobs.get(&id)
    }

    /// The jobs started and not read or lost.
    pub fn jobs(&self) -> impl Iterator<Item = &Job<K>> {
        self.jobs.values()
    }

    /// The worker's incarnation, while one is up.
    pub fn worker(&self) -> Option<Incarnation> {
        self.worker
    }

    /// The open documents with a compile pending.
    pub fn pending(&self) -> impl Iterator<Item = &K> {
        self.docs
            .iter()
            .filter(|(_, d)| d.open && d.dirty)
            .map(|(k, _)| k)
    }

    /// No worker is up and a compile is pending: spawn one (plan §3.4: lazily, and after a crash
    /// once the backoff has passed).
    pub fn needs_worker(&self) -> bool {
        self.worker.is_none() && self.pending().next().is_some()
    }

    /// Sets what `doc`'s compile reads (itself always): the syntax pass reads the document alone;
    /// a javac job reads what it recorded (SEM-05). A document tracked for the first time starts
    /// at epoch 0; for one already tracked, a change of its inputs bumps its epoch.
    pub fn track(&mut self, doc: K, reads: impl IntoIterator<Item = K>) -> Changed<K> {
        let mut reads: BTreeSet<K> = reads.into_iter().collect();
        reads.insert(doc.clone());
        let mut changed = Changed::new();
        if let Some(old) = self.docs.get(&doc).map(|d| d.reads.clone()) {
            let before = self.inputs_of(&old);
            let after = self.inputs_of(&reads);
            self.set_reads(&doc, &old, &reads);
            if before != after {
                changed.insert(doc);
                self.bump(&changed);
            }
            return changed;
        }
        self.set_reads(&doc, &BTreeSet::new(), &reads);
        self.docs.insert(
            doc,
            Document {
                open: false,
                session: Session::default(),
                version: 0,
                overlay: None,
                epoch: Epoch::default(),
                dirty: false,
                reads,
            },
        );
        changed
    }

    /// `didOpen`: the client's text at `version` (1 for a fresh open; the client's own version
    /// when a proxy re-sends the document to a restarted daemon, never renumbered: obligation
    /// 11). A new session and a new epoch.
    pub fn did_open(
        &mut self,
        doc: K,
        version: Version,
        text: Text,
    ) -> Result<Changed<K>, DiagnosticsError> {
        if !self.docs.contains_key(&doc) {
            let _new = self.track(doc.clone(), []);
        }
        if self.docs.get(&doc).is_some_and(|d| d.open) {
            return Err(DiagnosticsError::AlreadyOpen);
        }
        let forced = doc.clone();
        Ok(self.change(&doc, Some(forced), |this| {
            if let Some(d) = this.docs.get_mut(&doc) {
                d.open = true;
                d.session = Session(d.session.0.saturating_add(1));
                d.version = version;
                d.overlay = Some(text);
            }
        }))
    }

    /// `didChange`: the client's new text at `version`.
    pub fn did_change(
        &mut self,
        doc: K,
        version: Version,
        text: Text,
    ) -> Result<Changed<K>, DiagnosticsError> {
        if !self.docs.get(&doc).is_some_and(|d| d.open) {
            return Err(DiagnosticsError::NotOpen);
        }
        Ok(self.change(&doc, None, |this| {
            if let Some(d) = this.docs.get_mut(&doc) {
                d.version = version;
                d.overlay = Some(text);
            }
        }))
    }

    /// `didClose`: the overlay is dropped; the empty set is to be published at once, and every
    /// later reply of the session is dropped.
    pub fn did_close(&mut self, doc: K) -> Result<Closed<K>, DiagnosticsError> {
        if !self.docs.get(&doc).is_some_and(|d| d.open) {
            return Err(DiagnosticsError::NotOpen);
        }
        let changed = self.change(&doc, None, |this| {
            if let Some(d) = this.docs.get_mut(&doc) {
                d.open = false;
                d.overlay = None;
                d.dirty = false;
            }
        });
        Ok(Closed {
            clear: doc,
            changed,
        })
    }

    /// The watcher or the freshness stat saw `file`'s disk text (`None`: absent). Every document
    /// whose compile reads it takes a new epoch when its winning text changed.
    pub fn see_disk(&mut self, file: K, text: Option<Text>) -> Changed<K> {
        let key = file.clone();
        self.change(&key, None, |this| {
            match text {
                Some(text) => this.seen.insert(file, text),
                None => this.seen.remove(&file),
            };
        })
    }

    /// The worker `incarnation` is up.
    pub fn worker_started(&mut self, incarnation: Incarnation) -> Result<(), DiagnosticsError> {
        if self.worker.is_some() {
            return Err(DiagnosticsError::WorkerUp);
        }
        self.worker = Some(incarnation);
        Ok(())
    }

    /// Starts `doc`'s pending compile on the worker, of its inputs' winning texts as they are now.
    /// A pending compile is never held back because an older one still runs: results are ordered
    /// by epoch, not by arrival.
    pub fn start(&mut self, doc: &K) -> Result<Job<K>, DiagnosticsError> {
        let incarnation = self.worker.ok_or(DiagnosticsError::WorkerDown)?;
        let d = self.docs.get(doc).ok_or(DiagnosticsError::NotPending)?;
        if !(d.open && d.dirty) {
            return Err(DiagnosticsError::NotPending);
        }
        if self.jobs.len() >= self.capacity {
            return Err(DiagnosticsError::Busy);
        }
        let job = Job {
            id: JobId(self.next_job),
            document: doc.clone(),
            session: d.session,
            epoch: d.epoch,
            version: self.tag(doc, d),
            inputs: self.inputs_of(&d.reads),
            incarnation,
            late: false,
        };
        self.next_job = self.next_job.saturating_add(1);
        if let Some(d) = self.docs.get_mut(doc) {
            d.dirty = false;
        }
        self.jobs.insert(job.id, job.clone());
        Ok(job)
    }

    /// Job `id` passed its deadline: its document is compiled again when the job was current, and
    /// the job keeps running; its reply is published or dropped by the same rule as any other.
    pub fn timed_out(&mut self, id: JobId) -> Result<(), DiagnosticsError> {
        let worker = self.worker;
        let job = self
            .jobs
            .get(&id)
            .filter(|j| !j.late && Some(j.incarnation) == worker)
            .ok_or(DiagnosticsError::UnknownJob)?;
        let requeue = self.current(job).then(|| job.document.clone());
        if let Some(job) = self.jobs.get_mut(&id) {
            job.late = true;
        }
        if let Some(d) = requeue.and_then(|doc| self.docs.get_mut(&doc)) {
            d.dirty = true;
        }
        Ok(())
    }

    /// The worker died (or was killed after a hung compile) and the jobs `lost` will never be
    /// answered; a reply it wrote before it died is still read. Every document whose current
    /// compile was lost is compiled again (obligation 6).
    pub fn worker_lost(
        &mut self,
        lost: impl IntoIterator<Item = JobId>,
    ) -> Result<(), DiagnosticsError> {
        if self.worker.take().is_none() {
            return Err(DiagnosticsError::WorkerDown);
        }
        for id in lost {
            let Some(job) = self.jobs.remove(&id) else {
                continue;
            };
            if self.current(&job)
                && let Some(d) = self.docs.get_mut(&job.document)
            {
                d.dirty = true;
            }
        }
        Ok(())
    }

    /// A reply was read: published when its job was computed for its document's current epoch in
    /// its current session (the document is open), dropped otherwise (obligation 3).
    pub fn deliver<D>(&mut self, reply: Reply<D>) -> Delivery<K, D> {
        let Some(job) = self.jobs.remove(&reply.job) else {
            return Delivery::Unknown(reply);
        };
        let current = self.current(&job);
        let publication = Publication {
            document: job.document,
            version: job.version,
            diagnostics: reply.diagnostics,
        };
        if current {
            Delivery::Publish(publication)
        } else {
            Delivery::Stale(publication)
        }
    }

    /// The job was computed for its document's current epoch, in its current open session.
    fn current(&self, job: &Job<K>) -> bool {
        self.docs
            .get(&job.document)
            .is_some_and(|d| d.open && d.session == job.session && d.epoch == job.epoch)
    }

    /// The winning text of `file`: an open document's overlay while it is not older than the disk
    /// seen, else the disk seen.
    fn effective(&self, file: &K) -> Option<&Text> {
        let seen = self.seen.get(file);
        let overlay = self
            .docs
            .get(file)
            .filter(|d| d.open)
            .and_then(|d| d.overlay.as_ref());
        match overlay {
            Some(o) if seen.is_none_or(|s| o.stamp >= s.stamp) => Some(o),
            _ => seen,
        }
    }

    /// The version a compile of `doc` is published with: the client's while its overlay wins,
    /// none when the disk seen has overtaken it (decision 4).
    fn tag(&self, doc: &K, d: &Document<K>) -> Option<Version> {
        let overlay = d.overlay.as_ref()?;
        match self.seen.get(doc) {
            Some(disk) if disk.stamp > overlay.stamp => None,
            _ => Some(d.version),
        }
    }

    fn inputs_of(&self, reads: &BTreeSet<K>) -> BTreeMap<K, Option<Text>> {
        reads
            .iter()
            .map(|f| (f.clone(), self.effective(f).cloned()))
            .collect()
    }

    fn set_reads(&mut self, doc: &K, old: &BTreeSet<K>, new: &BTreeSet<K>) {
        for f in old.difference(new) {
            if let Some(r) = self.readers.get_mut(f) {
                r.remove(doc);
                if r.is_empty() {
                    self.readers.remove(f);
                }
            }
        }
        for f in new.difference(old) {
            self.readers
                .entry(f.clone())
                .or_default()
                .insert(doc.clone());
        }
        if let Some(d) = self.docs.get_mut(doc) {
            d.reads = new.clone();
        }
    }

    /// Applies `edit`, which changes at most `file`'s winning text, then bumps every document
    /// whose inputs it changed: `file`'s readers when its winning text moved, and `forced`.
    fn change(&mut self, file: &K, forced: Option<K>, edit: impl FnOnce(&mut Self)) -> Changed<K> {
        let was = self.effective(file).cloned();
        edit(self);
        let mut changed: Changed<K> = forced.into_iter().collect();
        if self.effective(file) != was.as_ref()
            && let Some(readers) = self.readers.get(file)
        {
            changed.extend(readers.iter().cloned());
        }
        self.bump(&changed);
        changed
    }

    /// Every changed document takes a new epoch, and an open one a pending compile.
    fn bump(&mut self, changed: &Changed<K>) {
        for doc in changed {
            if let Some(d) = self.docs.get_mut(doc) {
                d.epoch = Epoch(d.epoch.0.saturating_add(1));
                d.dirty = d.open;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(c: &mut Coordinator<&'static str>, doc: &'static str, stamp: u64) {
        let opened = c.did_open(doc, 1, Text::new(stamp, "text"));
        assert!(opened.is_ok(), "{opened:?}");
    }

    #[test]
    fn should_print_no_source_text_when_a_text_is_debugged() {
        // given
        let text = Text::new(7, "class Secret {}");

        // when
        let shown = format!("{text:?}");

        // then
        assert_eq!(shown, "Text { stamp: 7, bytes: 15 }");
    }

    #[test]
    fn should_refuse_each_transition_when_the_model_does_not_enable_it() {
        // given
        let mut c: Coordinator<&'static str> = Coordinator::new(1, []);
        let mut got = Vec::new();

        // when
        got.push(c.did_change("a", 2, Text::new(1, "x")).err());
        got.push(c.did_close("a").err());
        open(&mut c, "a", 1);
        got.push(c.did_open("a", 1, Text::new(1, "x")).err());
        got.push(c.start(&"a").err());
        got.push(c.worker_lost([]).err());
        let wanted = c.needs_worker();
        got.push(c.worker_started(Incarnation(1)).err());
        got.push(c.worker_started(Incarnation(2)).err());
        let job = c.start(&"a").map(|j| j.id).unwrap_or(JobId(u64::MAX));
        got.push(c.start(&"a").err());
        let _changed = c.did_change("a", 2, Text::new(2, "y"));
        got.push(c.start(&"a").err());
        got.push(c.timed_out(job).err());
        got.push(c.timed_out(job).err());
        got.push(c.timed_out(JobId(99)).err());

        // then
        use DiagnosticsError::*;
        assert!(wanted, "a pending compile with no worker asks for one");
        assert_eq!(
            got,
            [
                Some(NotOpen),
                Some(NotOpen),
                Some(AlreadyOpen),
                Some(WorkerDown),
                Some(WorkerDown),
                None,
                Some(WorkerUp),
                Some(NotPending),
                Some(Busy),
                None,
                Some(UnknownJob),
                Some(UnknownJob),
            ]
        );
    }

    #[test]
    fn should_bump_only_the_readers_when_a_file_they_read_changes() {
        // given: `a` reads `b`; `b` reads itself; `c` reads nothing of theirs
        let mut c: Coordinator<&'static str> = Coordinator::new(1, [("b", Text::new(1, "b1"))]);
        let _a = c.track("a", ["b"]);
        open(&mut c, "a", 2);
        open(&mut c, "c", 3);
        let before = |c: &Coordinator<&'static str>, d| c.document(&d).map(|x| x.epoch);
        let (a0, c0) = (before(&c, "a"), before(&c, "c"));

        // when
        let changed = c.see_disk("b", Some(Text::new(4, "b2")));
        let unchanged = c.see_disk("b", Some(Text::new(4, "b2")));

        // then
        assert_eq!(changed, ["a"].into_iter().collect());
        assert!(
            unchanged.is_empty(),
            "the same text seen again is no change"
        );
        assert_eq!(before(&c, "a"), a0.map(|e| Epoch(e.0.saturating_add(1))));
        assert_eq!(before(&c, "c"), c0);
        assert!(c.document(&"a").is_some_and(|d| d.dirty));
    }

    #[test]
    fn should_bump_the_document_when_tracking_changes_what_it_reads() {
        // given
        let mut c: Coordinator<&'static str> = Coordinator::new(1, [("b", Text::new(1, "b1"))]);
        open(&mut c, "a", 2);

        // when
        let wider = c.track("a", ["b"]);
        let same = c.track("a", ["b"]);
        let absent = c.track("a", ["b", "gone"]);

        // then: reading `b` changes the inputs; a file that is absent and was absent before is a
        // new input too (a negative lookup), so the inputs differ
        assert_eq!(wider, ["a"].into_iter().collect());
        assert!(same.is_empty());
        assert_eq!(absent, ["a"].into_iter().collect());
    }

    #[test]
    fn should_publish_without_a_version_when_the_disk_has_overtaken_the_overlay() {
        // given
        let mut c: Coordinator<&'static str> = Coordinator::new(1, []);
        open(&mut c, "a", 1);
        let _ = c.worker_started(Incarnation(1));
        let _changed = c.see_disk("a", Some(Text::new(5, "disk")));

        // when
        let job = c.start(&"a");
        let delivered = job.map(|job| {
            c.deliver(Reply {
                job: job.id,
                diagnostics: (),
            })
        });

        // then
        assert_eq!(
            delivered,
            Ok(Delivery::Publish(Publication {
                document: "a",
                version: None,
                diagnostics: (),
            }))
        );
    }
}
