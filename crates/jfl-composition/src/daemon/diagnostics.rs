//! Pushed syntax diagnostics (LSP-04; plan §3.2, §4.4): the shell around
//! `jfl_application::diagnostics::Coordinator`, one per connection that asked
//! (`jfl/watchDiagnostics`), with the syntax pass as its worker on the blocking pool.
//!
//! - **One coordinator per watching connection.** Overlays belong to the connection that opened
//!   them (plan §4.4) and so do the publications, so the coordinator's documents are that
//!   connection's open documents, keyed by file. A connection that never watches costs nothing.
//! - **The worker compiles `Job::inputs` and nothing else** (obligation 7): [`Work::compile`]
//!   takes the job's winning text of its document, never the disk, so what is published is what the
//!   coordinator recorded. Pinned by
//!   `diagnostics::tests::should_compile_the_jobs_input_and_never_the_disk_when_the_disk_differs`.
//! - **Stamps are a counter, strictly increasing per observation**, never mtimes: an overlay's
//!   arrival and every disk text seen to differ from the last one take the next number, so a shell
//!   edit in the same clock tick as an overlay still wins once it is seen (the slice-1 review). The
//!   disk is seen just before each overlay is applied (so a disk the overlay merely repeats never
//!   overtakes it) and at every tick (a shell edit, the agent's write). Each look is a stat on the
//!   loop, as a request's (IDX-09); the file is read only when its stat changed since the last
//!   read, or that read was racy (git's rule: modified within [`RACY`] of it), and its content is
//!   compared with the text seen last, so an unchanged file is no change whatever its mtime.
//! - **What it keeps is bounded by the open documents** (plan §5.9): only files an open document
//!   reads are observed, and a closed document is forgotten once no job names it
//!   (`Coordinator::forget_closed`).
//! - **Never blocks the loop.** A compile runs on the blocking pool and its result comes back as a
//!   [`Compiled`] the run loop reads like a frame; a publication is queued to the connection's
//!   outbox like a reply (a full outbox severs the connection, never waits).
//! - **Stale diagnostics never reach the client.** The coordinator publishes a reply only for its
//!   document's current epoch in its current session (obligation 3); each publication carries the
//!   connection's `seq` when its compile took its inputs, so the proxy drops one that raced a newer
//!   change of its own (`jfl_protocol::messages::PublishDiagnosticsParams::seq`).
//! - **A re-sent document keeps the client's version** (obligation 11): `jfl/didOpen`'s version is
//!   passed on as sent. Pinned by
//!   `diagnostics::tests::should_publish_with_the_client_version_when_a_document_is_re_sent`.
//!
//! Pinned by `diagnostics::tests` and the transcripts `fixtures/lsp/diagnostics-*.jsonl`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use jfl_application::diagnostics::{Coordinator, Delivery, Incarnation, Job, JobId, Reply, Text};
use jfl_application::syntax_indexer::{
    MAX_SOURCE_BYTES, SyntaxDiagnostic, SyntaxProblem, strip_bom,
};
use jfl_domain::{ByteOffset, ByteRange, FileUri, LineIndex, Position, PositionEncoding, Range};
use jfl_protocol::DocumentUri;
use jfl_protocol::codec;
use jfl_protocol::envelope::WireRange;
use jfl_protocol::messages::{
    DiagnosticSource, PublishDiagnostics, PublishDiagnosticsParams, Severity, WireDiagnostic,
};
use tokio::sync::mpsc;

use super::host::ConnId;
use crate::languages::{self, Indexers};

/// The compiles one connection runs at once: enough that a quick second edit starts without
/// waiting for the first, few enough that one client cannot fill the blocking pool.
pub const JOBS_PER_CONNECTION: usize = 2;

/// Compiles finished and not yet read by the loop: at most every connection's slots.
pub const COMPILED_QUEUE: usize = 256;

/// A disk file modified this recently is read again at a tick even when its stat did not change
/// (git's racy-mtime rule: a write in the same timestamp tick as the last read changes nothing a
/// stat sees).
pub const RACY: Duration = Duration::from_secs(2);

/// A compile's result, sent back to the run loop.
#[derive(Debug)]
pub struct Compiled {
    /// The connection whose coordinator started it.
    pub conn: ConnId,
    /// The job.
    pub job: JobId,
    /// The diagnostics, in the connection's encoding.
    pub diagnostics: Vec<WireDiagnostic>,
}

/// One compile to run: the job as the coordinator started it, for one connection.
#[derive(Debug, Clone)]
pub struct Work {
    /// The connection.
    pub conn: ConnId,
    /// The job: its document and the winning text of every file it reads.
    pub job: Job<FileUri>,
    /// The encoding of the ranges to compute.
    pub encoding: PositionEncoding,
}

impl Work {
    /// The syntax pass over the job's own input, its ranges counted in [`Work::encoding`] on that
    /// very text. A document whose input is absent (no overlay, no file) has no diagnostics.
    #[must_use]
    pub fn compile(&self, indexer: &Indexers<()>) -> Compiled {
        let text = self
            .job
            .inputs
            .get(&self.job.document)
            .and_then(Option::as_ref)
            .map(|t| Arc::clone(t.content()));
        let diagnostics = match text {
            Some(text) => syntax(indexer, &text, self.encoding),
            None => Vec::new(),
        };
        Compiled {
            conn: self.conn,
            job: self.job.id,
            diagnostics,
        }
    }
}

/// How a compile is run: on the blocking pool in the daemon, by hand in the tests that force a
/// reply order.
pub trait Runner: Send {
    /// Runs `work`; its [`Compiled`] comes back to the loop later.
    fn run(&mut self, work: Work);
}

/// The daemon's runner: each compile on Tokio's blocking pool, its result sent to the loop.
#[derive(Debug)]
pub struct PoolRunner {
    done: mpsc::Sender<Compiled>,
    indexer: Option<Arc<Indexers<()>>>,
}

impl PoolRunner {
    /// A runner reporting on `done`.
    #[must_use]
    pub const fn new(done: mpsc::Sender<Compiled>) -> Self {
        Self {
            done,
            indexer: None,
        }
    }

    fn indexer(&mut self) -> Option<Arc<Indexers<()>>> {
        if self.indexer.is_none() {
            self.indexer = Some(Arc::new(languages::indexers(()).ok()?));
        }
        self.indexer.clone()
    }
}

impl Runner for PoolRunner {
    fn run(&mut self, work: Work) {
        let indexer = self.indexer();
        let done = self.done.clone();
        let compile = move || {
            let compiled = match indexer {
                Some(indexer) => panic::catch_unwind(AssertUnwindSafe(|| work.compile(&indexer)))
                    .unwrap_or_else(|_| failed(&work)),
                None => failed(&work),
            };
            // The loop is gone when the daemon stops: nobody waits for this result.
            let _ = done.blocking_send(compiled);
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                let _detached = rt.spawn_blocking(compile);
            }
            // Outside a runtime (no daemon loop to report to): dropped with the loop's receiver.
            Err(_) => drop(compile),
        }
    }
}

/// The reply of a compile that could not run (the grammar did not load, or the pass panicked): one
/// warning that the text was not checked, so the epoch is still answered and nothing spins.
fn failed(work: &Work) -> Compiled {
    Compiled {
        conn: work.conn,
        job: work.job.id,
        diagnostics: remark(
            Position::default(),
            Severity::Warning,
            "not-checked",
            "the syntax check failed on this text; it was not checked".to_owned(),
        )
        .into_iter()
        .collect(),
    }
}

/// The syntax diagnostics of `text`, ranges in `encoding`.
#[must_use]
pub fn syntax(
    indexer: &Indexers<()>,
    text: &str,
    encoding: PositionEncoding,
) -> Vec<WireDiagnostic> {
    let bytes = strip_bom(text.as_bytes());
    let indexed = match indexer.index_blocking(bytes) {
        Ok(indexed) => indexed,
        Err(refused) => {
            let message = format!("the file was not checked: {refused}");
            return remark(
                Position::default(),
                Severity::Warning,
                "not-checked",
                message,
            )
            .into_iter()
            .collect();
        }
    };
    // The pass counts from after a byte-order mark; the client counts in the text it holds, the
    // mark included when it kept one, so the ranges are moved past it and counted in that text.
    let bom = u32::try_from(text.len().saturating_sub(bytes.len())).unwrap_or(0);
    let Ok(lines) = LineIndex::new(text) else {
        return Vec::new();
    };
    let mut out: Vec<WireDiagnostic> = indexed
        .diagnostics
        .iter()
        .filter_map(|d| wire(d, bom, &lines, encoding))
        .collect();
    if indexed.diagnostics_dropped > 0 {
        let end = out
            .last()
            .map_or_else(Position::default, |d| d.range.0.end());
        let message = format!(
            "{} more syntax errors are not shown",
            indexed.diagnostics_dropped
        );
        out.extend(remark(end, Severity::Information, "more", message));
    }
    out
}

/// A diagnostic of the pass itself (not of the text's syntax), at the empty range `at`.
fn remark(at: Position, severity: Severity, code: &str, message: String) -> Option<WireDiagnostic> {
    Some(WireDiagnostic {
        range: WireRange(Range::new(at, at).ok()?),
        severity,
        source: DiagnosticSource::Syntax,
        code: Some(code.to_owned()),
        message,
    })
}

fn wire(
    d: &SyntaxDiagnostic,
    bom: u32,
    lines: &LineIndex,
    encoding: PositionEncoding,
) -> Option<WireDiagnostic> {
    let shift = |at: ByteOffset| ByteOffset::new(at.get().saturating_add(bom));
    let bytes = ByteRange::new(shift(d.range.start()), shift(d.range.end())).ok()?;
    let range = lines.range(bytes, encoding).ok()?;
    let (code, message) = match &d.problem {
        SyntaxProblem::Unexpected => ("unexpected", "unexpected text".to_owned()),
        SyntaxProblem::Missing(what) if what.chars().all(|c| c.is_alphabetic() || c == ' ') => {
            ("missing", format!("{what} expected"))
        }
        SyntaxProblem::Missing(what) => ("missing", format!("'{what}' expected")),
        SyntaxProblem::StringTemplate => (
            "string-template",
            "a string template is not Java (a preview of Java 21 and 22, withdrawn in 23)"
                .to_owned(),
        ),
    };
    Some(WireDiagnostic {
        range: WireRange(range),
        severity: Severity::Error,
        source: DiagnosticSource::Syntax,
        code: Some(code.to_owned()),
        message,
    })
}

/// The disk as last read for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stat {
    modified: Option<SystemTime>,
    len: u64,
    /// When it was read (the racy rule).
    read_at: SystemTime,
}

/// One watching connection.
struct Watch {
    encoding: PositionEncoding,
    coordinator: Coordinator<FileUri>,
    /// The URI each open document was sent with, echoed in its publications.
    uris: BTreeMap<FileUri, DocumentUri>,
    /// The connection's document notifications applied so far (`seq`).
    applied: u64,
    /// Each running job's `seq` at its start.
    seqs: BTreeMap<JobId, u64>,
    stats: BTreeMap<FileUri, Stat>,
    incarnation: u32,
}

/// What became of a document notification, for the coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    /// Opened (or re-sent) at `version` with `text`.
    Open {
        /// The client's version.
        version: i32,
        /// The text.
        text: &'a str,
    },
    /// The whole new text at `version`.
    Edit {
        /// The client's version.
        version: i32,
        /// The text.
        text: &'a str,
    },
    /// Closed.
    Close,
}

/// Every watching connection's coordinator, and the runner of their compiles.
pub struct Diagnostics {
    watches: BTreeMap<ConnId, Watch>,
    stamp: u64,
    runner: Box<dyn Runner>,
}

impl std::fmt::Debug for Diagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Diagnostics")
            .field("watching", &self.watches.len())
            .field("stamp", &self.stamp)
            .finish_non_exhaustive()
    }
}

/// A publication to queue on a connection: the `jfl/publishDiagnostics` frame's body.
pub type Outgoing = (ConnId, Vec<u8>);

impl Diagnostics {
    /// No connection watching yet; compiles run through `runner`.
    #[must_use]
    pub fn new(runner: Box<dyn Runner>) -> Self {
        Self {
            watches: BTreeMap::new(),
            stamp: 0,
            runner,
        }
    }

    /// Whether `conn` watches.
    #[must_use]
    pub fn watching(&self, conn: ConnId) -> bool {
        self.watches.contains_key(&conn)
    }

    /// How many documents `conn`'s coordinator holds (open, or closed with a job still out).
    #[must_use]
    pub fn documents(&self, conn: ConnId) -> usize {
        self.watches
            .get(&conn)
            .map_or(0, |w| w.coordinator.documents())
    }

    fn next_stamp(&mut self) -> u64 {
        self.stamp = self.stamp.saturating_add(1);
        self.stamp
    }

    /// `conn` asked for its documents' diagnostics in `encoding` (`jfl/watchDiagnostics`), with
    /// `open` its documents open now and `applied` its document notifications so far. A second
    /// watch changes the encoding of what follows.
    pub fn watch<'a>(
        &mut self,
        conn: ConnId,
        encoding: PositionEncoding,
        applied: u64,
        open: impl IntoIterator<Item = (&'a DocumentUri, i32, &'a str)>,
    ) -> Vec<Outgoing> {
        if let Some(watch) = self.watches.get_mut(&conn) {
            watch.encoding = encoding;
            return Vec::new();
        }
        self.watches.insert(
            conn,
            Watch {
                encoding,
                coordinator: Coordinator::new(JOBS_PER_CONNECTION, []),
                uris: BTreeMap::new(),
                applied,
                seqs: BTreeMap::new(),
                stats: BTreeMap::new(),
                incarnation: 0,
            },
        );
        let mut out = Vec::new();
        for (uri, version, text) in open {
            out.extend(self.document(conn, applied, uri, Change::Open { version, text }));
        }
        out
    }

    /// `conn` is gone (closed, severed, or the daemon stops): its coordinator with it; compiles
    /// still running are dropped when they come back.
    pub fn gone(&mut self, conn: ConnId) {
        self.watches.remove(&conn);
    }

    /// Every connection is gone.
    pub fn clear(&mut self) {
        self.watches.clear();
    }

    /// A document notification of `conn` was applied (its `applied`-th): the disk seen, the
    /// overlay given to the coordinator, compiles started. Returns what to publish now (a close's
    /// empty set).
    pub fn document(
        &mut self,
        conn: ConnId,
        applied: u64,
        uri: &DocumentUri,
        change: Change<'_>,
    ) -> Vec<Outgoing> {
        let Some(watch) = self.watches.get_mut(&conn) else {
            return Vec::new();
        };
        watch.applied = applied;
        let file = uri.file().clone();
        let mut out = Vec::new();
        match change {
            Change::Open { version, text } | Change::Edit { version, text } => {
                if watch.coordinator.document(&file).is_none() {
                    let _new = watch.coordinator.track(file.clone(), []);
                }
                // The disk first, so an overlay that repeats it is the newer text.
                if let Some(disk) = changed_disk(&file, &mut watch.stats) {
                    self.see(conn, &file, disk);
                }
                let text = Text::new(self.next_stamp(), text);
                let Some(watch) = self.watches.get_mut(&conn) else {
                    return out;
                };
                // A `didOpen` of a document open already (sent again under another spelling of
                // its URI) is its new text.
                let open = watch.coordinator.document(&file).is_some_and(|d| d.open);
                let _changed = if open {
                    watch.coordinator.did_change(file.clone(), version, text)
                } else {
                    watch.coordinator.did_open(file.clone(), version, text)
                };
                watch.uris.insert(file, uri.clone());
            }
            Change::Close => {
                if let Ok(closed) = watch.coordinator.did_close(file.clone()) {
                    let sent = watch
                        .uris
                        .get(&closed.clear)
                        .cloned()
                        .unwrap_or_else(|| uri.clone());
                    out.extend(
                        encode(&sent, None, applied, watch.encoding, Vec::new()).map(|b| (conn, b)),
                    );
                }
                self.forget(conn);
            }
        }
        self.pump(conn);
        out
    }

    /// The disk of `file` as now read, for `conn`'s coordinator: a changed text takes the next
    /// stamp; the same text as seen last changes nothing.
    fn see(&mut self, conn: ConnId, file: &FileUri, disk: Disk) {
        let Some(watch) = self.watches.get(&conn) else {
            return;
        };
        let same = match (&disk, watch.coordinator.seen(file)) {
            (Disk::Absent, None) => true,
            (Disk::Text(now), Some(seen)) => seen.content().as_ref() == now.as_str(),
            _ => false,
        };
        if same {
            return;
        }
        let text = match disk {
            Disk::Text(now) => Some(Text::new(self.next_stamp(), now)),
            Disk::Absent => None,
        };
        if let Some(watch) = self.watches.get_mut(&conn) {
            let _changed = watch.coordinator.see_disk(file.clone(), text);
        }
    }

    /// The tick: the disk of every file an open document reads, read again where its stat changed
    /// or is racy, for every watching connection; the compiles it makes pending start now and
    /// publish when they come back.
    pub fn observe_disk(&mut self) {
        let conns: Vec<ConnId> = self.watches.keys().copied().collect();
        for conn in conns {
            let files: Vec<FileUri> = match self.watches.get(&conn) {
                Some(w) => w
                    .uris
                    .keys()
                    .filter(|f| w.coordinator.is_read(f))
                    .cloned()
                    .collect(),
                None => continue,
            };
            for file in files {
                let disk = self
                    .watches
                    .get_mut(&conn)
                    .and_then(|w| changed_disk(&file, &mut w.stats));
                if let Some(disk) = disk {
                    self.see(conn, &file, disk);
                }
            }
            self.pump(conn);
        }
    }

    /// A compile came back: published when it is current, and further compiles started.
    pub fn compiled(&mut self, compiled: Compiled) -> Vec<Outgoing> {
        let conn = compiled.conn;
        let Some(watch) = self.watches.get_mut(&conn) else {
            return Vec::new();
        };
        let seq = watch.seqs.remove(&compiled.job).unwrap_or(watch.applied);
        let delivery = watch.coordinator.deliver(Reply {
            job: compiled.job,
            diagnostics: compiled.diagnostics,
        });
        let mut out = Vec::new();
        if let Delivery::Publish(p) = delivery
            && let Some(uri) = watch.uris.get(&p.document)
        {
            out.extend(
                encode(uri, p.version, seq, watch.encoding, p.diagnostics).map(|b| (conn, b)),
            );
        }
        self.forget(conn);
        self.pump(conn);
        out
    }

    /// Forgets `conn`'s closed documents no job names, with the URIs and stats kept for them.
    fn forget(&mut self, conn: ConnId) {
        if let Some(watch) = self.watches.get_mut(&conn) {
            let _dropped = watch.coordinator.forget_closed();
            let coordinator = &watch.coordinator;
            watch.uris.retain(|f, _| coordinator.document(f).is_some());
            watch.stats.retain(|f, _| coordinator.is_read(f));
        }
    }

    /// Starts every pending compile `conn`'s slots allow, spawning the worker first if needed.
    fn pump(&mut self, conn: ConnId) {
        let Some(watch) = self.watches.get_mut(&conn) else {
            return;
        };
        if watch.coordinator.needs_worker() {
            watch.incarnation = watch.incarnation.saturating_add(1);
            let _up = watch
                .coordinator
                .worker_started(Incarnation(watch.incarnation));
        }
        let pending: Vec<FileUri> = watch.coordinator.pending().cloned().collect();
        for doc in pending {
            let Ok(job) = watch.coordinator.start(&doc) else {
                break;
            };
            watch.seqs.insert(job.id, watch.applied);
            self.runner.run(Work {
                conn,
                job,
                encoding: watch.encoding,
            });
        }
    }
}

/// A publication's frame body.
fn encode(
    uri: &DocumentUri,
    version: Option<i32>,
    seq: u64,
    encoding: PositionEncoding,
    diagnostics: Vec<WireDiagnostic>,
) -> Option<Vec<u8>> {
    codec::encode(&codec::notification::<PublishDiagnostics>(
        PublishDiagnosticsParams {
            uri: uri.clone(),
            version,
            seq,
            position_encoding: encoding,
            diagnostics,
        },
    ))
    .ok()
}

/// A file's disk as read.
enum Disk {
    Text(String),
    Absent,
}

/// Reads `file` now, recording its stat; `None` when it is not read (a directory, a denied file,
/// or one over `MAX_SOURCE_BYTES`, which the syntax pass would refuse anyway): the disk is not
/// seen then, and the overlay stays the text.
fn read_disk(file: &FileUri, stats: &mut BTreeMap<FileUri, Stat>) -> Option<Disk> {
    let path = Path::new(file.path());
    let meta = match fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            stats.remove(file);
            return Some(Disk::Absent);
        }
        Err(_) => return None,
    };
    let limit = u64::try_from(MAX_SOURCE_BYTES.saturating_add(4)).unwrap_or(u64::MAX);
    if meta.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(limit).read_to_end(&mut bytes))
        .ok()?;
    stats.insert(
        file.clone(),
        Stat {
            modified: meta.modified().ok(),
            len: meta.len(),
            read_at: SystemTime::now(),
        },
    );
    Some(Disk::Text(String::from_utf8_lossy(&bytes).into_owned()))
}

/// The disk of `file` when its stat changed since it was last read, or the last read was racy;
/// `None` when nothing could have changed.
fn changed_disk(file: &FileUri, stats: &mut BTreeMap<FileUri, Stat>) -> Option<Disk> {
    let path = Path::new(file.path());
    let meta = fs::metadata(path).ok();
    if let (Some(meta), Some(last)) = (&meta, stats.get(file)) {
        let same = meta.modified().ok() == last.modified && meta.len() == last.len;
        let racy = last
            .modified
            .is_none_or(|m| last.read_at.duration_since(m).unwrap_or_default() < RACY);
        if same && !racy {
            return None;
        }
    }
    read_disk(file, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc as std_mpsc;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A fresh directory under the system's temporary directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("jfl-diagnostics-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A runner that keeps every compile for the test to finish, in any order.
    struct Held(std_mpsc::SyncSender<Work>);

    impl Runner for Held {
        fn run(&mut self, work: Work) {
            let _ = self.0.send(work);
        }
    }

    fn indexer() -> Indexers<()> {
        languages::indexers(()).unwrap()
    }

    fn held() -> (Diagnostics, std_mpsc::Receiver<Work>) {
        let (tx, rx) = std_mpsc::sync_channel(16);
        (Diagnostics::new(Box::new(Held(tx))), rx)
    }

    fn uri(dir: &Path, name: &str) -> DocumentUri {
        DocumentUri::parse(&format!("file://{}/{name}", dir.display())).unwrap()
    }

    fn published(out: &[Outgoing]) -> Vec<PublishDiagnosticsParams> {
        out.iter()
            .map(|(_, body)| {
                let value: serde_json::Value = serde_json::from_slice(body).unwrap();
                serde_json::from_value(value["params"].clone()).unwrap()
            })
            .collect()
    }

    const CONN: ConnId = ConnId(7);
    const BROKEN: &str = "class A {\n  int x = 1\n}\n";
    const FIXED: &str = "class A {\n  int x = 1;\n}\n";
    /// [`FIXED`] broken by one byte: a missing semicolon, the same length.
    const SAME_LENGTH: &str = "class A {\n  int x = 1 \n}\n";

    #[test]
    fn should_publish_only_the_newest_when_two_quick_edits_reply_in_reverse_order() {
        // given: a watching connection; one open, two quick edits, three compiles held
        let dir = Scratch::new();
        let doc = uri(dir.path(), "A.java");
        let (mut d, works) = held();
        let indexer = indexer();
        let _ = d.watch(CONN, PositionEncoding::Utf16, 0, []);
        let _ = d.document(
            CONN,
            1,
            &doc,
            Change::Open {
                version: 1,
                text: BROKEN,
            },
        );
        let _ = d.document(
            CONN,
            2,
            &doc,
            Change::Edit {
                version: 2,
                text: FIXED,
            },
        );
        let first = works.try_recv().unwrap();
        let second = works.try_recv().unwrap();
        let _ = d.document(
            CONN,
            3,
            &doc,
            Change::Edit {
                version: 3,
                text: BROKEN,
            },
        );

        // when: the second edit's compile answers first, then the first's
        let mut out = d.compiled(second.compile(&indexer));
        out.extend(d.compiled(first.compile(&indexer)));
        let third = works.try_recv().unwrap();
        out.extend(d.compiled(third.compile(&indexer)));

        // then: only version 3's, with its one error
        let got = published(&out);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].version, Some(3));
        assert_eq!(got[0].seq, 3);
        assert_eq!(got[0].diagnostics.len(), 1);
        assert!(works.try_recv().is_err(), "nothing else is compiled");
    }

    #[test]
    fn should_compile_the_jobs_input_and_never_the_disk_when_the_disk_differs() {
        // given: the disk holds a broken file; the client's overlay is fixed
        let dir = Scratch::new();
        std::fs::write(dir.path().join("A.java"), BROKEN).unwrap();
        let doc = uri(dir.path(), "A.java");
        let (mut d, works) = held();
        let _ = d.watch(CONN, PositionEncoding::Utf16, 0, []);
        let _ = d.document(
            CONN,
            1,
            &doc,
            Change::Open {
                version: 1,
                text: FIXED,
            },
        );
        let work = works.try_recv().unwrap();

        // when: the disk is broken again, differently, before the compile runs
        std::fs::write(dir.path().join("A.java"), "class {").unwrap();
        let compiled = work.compile(&indexer());

        // then: the overlay was compiled (no error), as the job recorded it
        let input = work.job.inputs.get(doc.file()).cloned().flatten();
        assert_eq!(
            input.map(|t| t.content().to_string()),
            Some(FIXED.to_owned())
        );
        assert!(
            compiled.diagnostics.is_empty(),
            "{:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn should_publish_the_disk_without_a_version_when_a_shell_edit_follows_the_overlay() {
        // given: an open document equal to the disk
        let dir = Scratch::new();
        let path = dir.path().join("A.java");
        std::fs::write(&path, FIXED).unwrap();
        let doc = uri(dir.path(), "A.java");
        let (mut d, works) = held();
        let indexer = indexer();
        let _ = d.watch(CONN, PositionEncoding::Utf16, 0, []);
        let _ = d.document(
            CONN,
            1,
            &doc,
            Change::Open {
                version: 1,
                text: FIXED,
            },
        );
        let first = d.compiled(works.try_recv().unwrap().compile(&indexer));

        // when: a shell edit of the same length in the same timestamp tick (its mtime set back),
        // which a stat cannot see: seen at the next tick because the last read was racy
        d.observe_disk();
        let nothing = works.try_recv().is_err();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, SAME_LENGTH).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(before)
            .unwrap();
        let unseen_by_stat = std::fs::metadata(&path).unwrap().modified().unwrap() == before
            && std::fs::metadata(&path).unwrap().len() == FIXED.len() as u64;
        d.observe_disk();
        let disk = d.compiled(works.try_recv().unwrap().compile(&indexer));

        // then: the overlay's publication had its version; the disk's has none and its error
        assert!(nothing, "the same disk text is no change");
        assert!(unseen_by_stat, "the stat is the one the last read recorded");
        assert_eq!(published(&first)[0].version, Some(1));
        let disk = published(&disk);
        assert_eq!(disk[0].version, None);
        assert_eq!(disk[0].diagnostics.len(), 1);
    }

    #[test]
    fn should_publish_with_the_client_version_when_a_document_is_re_sent() {
        // given: a new connection (a restarted daemon) the proxy re-sends a document to
        let dir = Scratch::new();
        let doc = uri(dir.path(), "A.java");
        let (mut d, works) = held();
        let _ = d.watch(CONN, PositionEncoding::Utf16, 0, []);

        // when
        let _ = d.document(
            CONN,
            1,
            &doc,
            Change::Open {
                version: 7,
                text: BROKEN,
            },
        );
        let out = d.compiled(works.try_recv().unwrap().compile(&indexer()));

        // then: obligation 11, never renumbered
        assert_eq!(published(&out)[0].version, Some(7));
    }

    #[test]
    fn should_publish_the_empty_set_and_drop_the_late_reply_when_closed_during_a_compile() {
        // given
        let dir = Scratch::new();
        let doc = uri(dir.path(), "A.java");
        let (mut d, works) = held();
        let _ = d.watch(CONN, PositionEncoding::Utf8, 0, []);
        let _ = d.document(
            CONN,
            1,
            &doc,
            Change::Open {
                version: 1,
                text: BROKEN,
            },
        );
        let running = works.try_recv().unwrap();

        // when
        let closed = d.document(CONN, 2, &doc, Change::Close);
        let late = d.compiled(running.compile(&indexer()));

        // then: an empty set, at the close's seq, and nothing after it; nothing is kept
        let closed = published(&closed);
        assert_eq!(closed.len(), 1);
        assert!(closed[0].diagnostics.is_empty());
        assert_eq!((closed[0].version, closed[0].seq), (None, 2));
        assert!(late.is_empty());
        assert_eq!(d.documents(CONN), 0);
    }

    #[test]
    fn should_count_the_byte_order_mark_when_the_compiled_text_keeps_one() {
        // given: the same broken line 0, with and without a leading U+FEFF
        let plain = "class A { int x = 1 }\n";
        let marked = format!("\u{FEFF}{plain}");

        // when
        let at = |text: &str, encoding| {
            syntax(&indexer(), text, encoding)
                .first()
                .map(|d| d.range.0.start())
        };

        // then: one UTF-16 unit, three UTF-8 bytes further
        let (p16, p8) = (
            at(plain, PositionEncoding::Utf16),
            at(plain, PositionEncoding::Utf8),
        );
        assert_eq!(p16, Some(Position::new(0, 19)));
        assert_eq!(
            at(&marked, PositionEncoding::Utf16),
            Some(Position::new(0, 20))
        );
        assert_eq!(p8, Some(Position::new(0, 19)));
        assert_eq!(
            at(&marked, PositionEncoding::Utf8),
            Some(Position::new(0, 22))
        );
    }

    #[test]
    fn should_count_columns_in_utf16_on_the_compiled_text_when_a_line_holds_astral_characters() {
        // given: an emoji (two UTF-16 units, four bytes) before the missing semicolon
        let text = "class A {\n  String s = \"\u{1F600}\"\n}\n";

        // when
        let utf16 = syntax(&indexer(), text, PositionEncoding::Utf16);
        let utf8 = syntax(&indexer(), text, PositionEncoding::Utf8);

        // then
        let at = |d: &[WireDiagnostic]| d.first().map(|d| d.range.0.start());
        assert_eq!(at(&utf16), Some(Position::new(1, 17)));
        assert_eq!(at(&utf8), Some(Position::new(1, 19)));
    }
}
