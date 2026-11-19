//! The view service: the async shell over the view registry's core ([`crate::views`]) and the
//! ports (plan §4, ADR 0017, ADR 0037). The registry does no I/O and fixes the order of
//! `formal/views.qnt`; this shell does the I/O each transition stands for, in that order:
//!
//! - **Open on first request, innermost first.** A request asks [`RepoProbe::resolve`] for the
//!   innermost worktree of its file and opens that view before the request starts, because
//!   [`ViewRegistry::bind`] picks the innermost *open* view: a nested worktree's file must never
//!   bind to its parent (pinned by
//!   `tests/view_service.rs::should_bind_a_nested_worktree_file_to_its_own_view_when_the_parent_is_open`).
//! - **Watch, then scan, then publish.** A view is opened by checking its filesystem
//!   ([`SourceTree::filesystem`]: only a local one is watched), watching, sweeping stale cookies
//!   ([`SourceTree::sweep_cookies`]), reading `HEAD`'s stat set and the clock, listing, reading
//!   and summarising files ([`ServiceConfig::parallelism`] at once), and publishing
//!   generation 1 (pinned by
//!   `tests/view_service.rs::should_hear_of_a_file_written_during_the_open_scan_when_the_watch_came_first`).
//!   A lost watch is re-established before the rescan that may certify the view again.
//! - **The first answer does not wait for the whole view** (plan §1.7, §2.7). A view of more
//!   than [`ServiceConfig::open_whole`] files reads only the request's own directory at its open
//!   and publishes every other listed file as [`Summary::Pending`]: the model's entry at the
//!   listed version with a racy summary, which no stat proves fresh, plus a coverage gap the model
//!   does not contain. The guard is this shell's coverage rule: `count` counts a pending file
//!   among the files and never among the indexed, and the request's `indexed < files` rule makes
//!   every answer from such a generation `ModulePartlyIndexed` (pinned by
//!   `should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest` and
//!   `should_read_only_the_requested_file_when_a_request_finds_it_pending`). A pending file
//!   leaves the view only by being read or removed: a request's stat finds it changed (dirty, then
//!   a build that reads just it), a hint, a rescan, or [`ViewService::settle`], whose file events
//!   are made by this shell (not raised by a watcher) and then reconciled and published. The
//!   open is the model's `init`; `doc/formal-models.md` ("The open in two steps") records the
//!   mapping, and why atomic publish holds only when a declared gap counts as stored.
//! - **A parent never holds a nested view's file** (plan §4.2). Each view keeps the nested roots
//!   its last scan skipped (worktrees, repositories, agent worktrees, links) and the views opened
//!   inside it since; a hint under one is dropped. A hint for a file the view does not hold asks
//!   for a rescan, never a read, since the path may be under a directory no scan has seen (a
//!   worktree just created, a `.gitignore`d one); a rescan leaves out every path its listing
//!   (unioned with the tracked files) does not admit, so whatever leaked in leaves
//!   ([`ViewRegistry::exclude`]); a view opened inside an open one makes the parent rescan (pinned
//!   by the `should_keep_*_out_of_*` tests and
//!   `should_drop_a_nested_view_s_files_from_the_parent_when_the_nested_view_opens`).
//! - **The listing, unioned.** Every scan unions the listing with the files the index tracks
//!   ([`RepoProbe::tracked_java`]), build-output directories included (IDX-06's card), and hands
//!   it to [`ViewRegistry::reconcile_start`], so a file created while no event could be raised is
//!   read. What a scan could not see (an unreadable directory, an unrepresentable path, a failed
//!   `ls-files`) and every file without a summary (unreadable, refused by the indexer) is a gap:
//!   the answer is `ModulePartlyIndexed`, never complete.
//! - **Hints.** A `.java` file the view holds is a file event; any other path a hint names (a new
//!   file, a directory, a `.gitignore`, a build file) asks for a rescan, since what changed below
//!   it is not named (`should_rescan_when_a_hint_names_no_java_file`); a cookie of this daemon is
//!   its request's barrier, another daemon's is ignored
//!   (`should_ignore_another_daemon_s_cookie_when_it_is_hinted`). A [`Rescan`] whose reasons
//!   [degrade the watch](crate::source_tree::RescanReasons::degrade_the_watch) is a lost watch
//!   (re-watched at once); any other is an overflow, for every view. Either loses the view's
//!   queued cookies, whose events may have been dropped or folded into it, at once rather than at
//!   their deadline (`should_answer_incomplete_when_a_rescan_comes_in_place_of_the_cookie`).
//! - **The cookie** ([`SourceTree::write_cookie`]) is written into the view's root, through its own
//!   watch, and waited for with a deadline ([`WallClock::deadline`]); a missed deadline is
//!   [`ViewRegistry::cookie_lost`]. A file already of the cookie's name is never removed: the next
//!   attempt's name is taken. The file exists for the microseconds between its create and remove;
//!   another watcher of the worktree sees it come and go.
//! - **The Git stat set.** `HEAD`, the ref it names and `packed-refs` stand for the registry's
//!   `HEAD`, the index for its index ([`GitObserved`]), so a commit on the checked-out branch,
//!   which leaves `HEAD` itself untouched, is seen at the pin.
//! - **Summaries live in the generation.** Each entry carries its summary ([`Summary`]); the
//!   summary store is the cache a view is reopened from. So the store's cap evicting an entry
//!   between a summary and its publish never leaves the build waiting for it (IDX-08's review),
//!   and the registry's atomic-publish guard (plan §7.1) holds by construction here (`Held`).
//!   The store is written after the publish, by [`ViewService::write_back`], never on the way to
//!   one: a cold open's writes cost as much as its parse (`doc/bench/README.md`). Until then a
//!   summary the indexer made is looked up in memory before the store, so a second view of the
//!   same content does not index it again
//!   (`should_index_the_same_content_once_when_a_second_view_opens_before_the_write_back`), and
//!   an empty store is not asked at all.
//!
//! A rescan re-stats every indexed path and lists the view; it does not narrow the work with
//! [`RepoProbe::changed_since`], which costs more than the stats it would save (39 ms on medium
//! against 9 ms for every stat, `doc/bench/README.md`), so IDX-05's ordering obligations for it
//! do not arise here.
//!
//! One request at a time: the service takes `&mut self`. The daemon (IDX-12) owns it and
//! serialises its requests.

use core::future::Future;
use core::pin::pin;
use core::task::Poll;
use core::task::Waker;
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jfl_domain::{
    ByteRange, Completeness, FileSummary, FileUri, Generation, IncompleteReason, KeyHasher,
    ModuleName, RelPath, SyntaxInputs, SyntaxKey,
};

use crate::clock::{WallClock, elapsed};
use crate::queries::{FileText, Sources};
use crate::repo_probe::{
    GitFile, GitSnapshot, GitStat, HeadState, RepoProbe, RepoProbeError, Worktree,
};
use crate::source_tree::{
    Exclusion, FileStat, FsKind, Hint, Hints, ListMode, MTime, Rescan, RescanReason, RescanReasons,
    SourceTree, SourceTreeError, cookie_name, cookie_stamp, is_cookie, is_java_file_name,
};
use crate::summary_store::SummaryStore;
use crate::syntax_indexer::{MAX_SOURCE_BYTES, SiteSyntax, SyntaxIndexer, strip_bom};
use crate::views::{
    Cookie, Event, FileSeen, GitSeen, Observed, Published, RequestId, Stamp, Stored, ViewError,
    ViewRegistry,
};

/// A file's summary as a generation holds it.
#[derive(Debug, Clone)]
pub enum Summary {
    /// The file did not exist when it was read.
    Absent,
    /// The file's summary, under the key of the bytes it summarises.
    Indexed {
        /// The key.
        key: SyntaxKey,
        /// The summary.
        summary: Arc<FileSummary>,
    },
    /// The file exists and has no summary: it could not be read whole, or the indexer refused
    /// it. A gap, never an empty summary (ADR 0014).
    Gap,
    /// The file was listed when its view opened and has not been read yet: a gap like [`Gap`]
    /// (the answer is `ModulePartlyIndexed`), whose entry is racy, so no stat proves it fresh and
    /// it leaves the view only by being read: a request's stat, a hint, a rescan or
    /// [`ViewService::settle`] (pinned by
    /// `tests/view_service.rs::should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest`).
    ///
    /// [`Gap`]: Summary::Gap
    Pending,
}

impl PartialEq for Summary {
    /// By key: a key names one content (plan §5.4).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent)
            | (Self::Gap, Self::Gap)
            | (Self::Pending, Self::Pending) => true,
            (Self::Indexed { key: a, .. }, Self::Indexed { key: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Eq for Summary {}

/// Every [`Summary`] carries what it stands for, so a generation holds its summaries whatever
/// the store's cap evicts.
struct Held;

impl Stored<Summary> for Held {
    fn holds(&self, _: &Summary) -> bool {
        true
    }
}

/// One side of the Git stat set (plan §4.3) as a snapshot saw it: `HEAD`, the ref it names and
/// `packed-refs` (the registry's `head`), or the index (its `index`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitObserved {
    /// What `HEAD` named (the `head` side only) and the files' stats; `None` when the snapshot
    /// failed.
    seen: Option<(Option<HeadState>, Vec<GitStat>)>,
    /// Whether every file was settled when it was stat'ed (the racy rule) and `HEAD` readable.
    settled: bool,
}

impl Observed for GitObserved {
    fn proves_unchanged(&self, now: &Self) -> bool {
        self.settled && self.seen.is_some() && self.seen == now.seen
    }
}

/// The registry's view of the stat set: `HEAD`, the ref `HEAD` names and `packed-refs` as its
/// `head`, the index as its `index`; nothing proven when the snapshot failed.
///
/// Pinned by: `view_service::tests::should_cover_the_branch_ref_when_head_is_observed`.
#[must_use]
pub fn observe_git(
    snapshot: Result<GitSnapshot, RepoProbeError>,
    observed_at: MTime,
    granularity: Duration,
) -> GitSeen<GitObserved> {
    let Ok(snapshot) = snapshot else {
        let unknown = GitObserved {
            seen: None,
            settled: false,
        };
        return GitSeen {
            head: unknown.clone(),
            index: unknown,
        };
    };
    let part = |index: bool| {
        let files: Vec<GitStat> = snapshot
            .files
            .iter()
            .filter(|f| (f.file == GitFile::Index) == index)
            .cloned()
            .collect();
        let settled = files.iter().all(|f| {
            f.stat
                .is_none_or(|s| s.settled_before(observed_at, granularity))
        });
        (files, settled)
    };
    let (head_files, head_settled) = part(false);
    let (index_files, index_settled) = part(true);
    let readable = snapshot.head != HeadState::Unreadable;
    GitSeen {
        head: GitObserved {
            seen: Some((Some(snapshot.head), head_files)),
            settled: head_settled && readable,
        },
        index: GitObserved {
            seen: Some((None, index_files)),
            settled: index_settled,
        },
    }
}

/// The service's settings; composition chooses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// The coarsest timestamp step of the filesystems served (the racy rule): the kernel's
    /// coarse clock tick on Linux, a second on HFS+.
    pub granularity: Duration,
    /// How long a request waits for each cookie before it is lost (plan §2.7).
    pub cookie_deadline: Duration,
    /// Names this daemon's cookies ([`cookie_name`]): ASCII letters and digits.
    pub owner: String,
    /// How old a cookie file must be for a view's open to sweep it as stale, far past any
    /// deadline: a live daemon's cookie lives for microseconds.
    pub cookie_sweep_age: Duration,
    /// How many files a view's open or a reconcile stats, reads and summarises at once, each
    /// step on the ports' blocking pool: more than the pool's threads, so a thread finishing one
    /// step finds the next file's waiting rather than idling for the hand-off.
    pub parallelism: usize,
    /// The most files a view opens whole. A larger view's open reads only the request's own
    /// directory (its package) and publishes every other file as [`Summary::Pending`], so the
    /// first answer does not wait for the whole view (plan §1.7's first useful answer, plan
    /// §2.7); [`ViewService::settle`] reads the rest.
    pub open_whole: usize,
    /// The most bytes of source files read and held at once by the files in flight (each from
    /// its read to the end of its parse); a file larger than this is read alone. The count
    /// ([`ServiceConfig::parallelism`]) alone would let 16 files of [`MAX_SOURCE_BYTES`] be held
    /// at once.
    pub read_budget: usize,
    /// The most summaries [`ViewService::write_back`] keeps waiting; past it, a new one is not
    /// kept for the store (the store is a cache: the next open re-indexes that content), so a
    /// daemon that never writes back holds no more than this beyond its generations.
    pub unwritten_cap: usize,
}

impl ServiceConfig {
    /// Linux's coarse tick (10 ms), a 200 ms cookie deadline (twice plan §1.7's 100 ms
    /// freshness target), cookies swept after a minute, sixteen files in flight (four pool
    /// threads' worth, measured by `just bench-views`), views of up to 256 files opened whole
    /// (about 50 ms of parsing on four cores), 32 MiB of source read at once (two of the
    /// largest files indexed; the medium corpus's mean file is 6 KB), and 65,536 summaries
    /// waiting to be written (about twice the large corpus's files).
    #[must_use]
    pub fn new(owner: &str) -> Self {
        Self {
            granularity: Duration::from_millis(10),
            cookie_deadline: Duration::from_millis(200),
            owner: owner.to_owned(),
            cookie_sweep_age: Duration::from_secs(60),
            parallelism: 16,
            open_whole: 256,
            read_budget: READ_BUDGET,
            unwritten_cap: 65_536,
        }
    }
}

/// Why a request could not be answered at all. Anything the service could not see but could
/// still answer around is an incomplete answer instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    /// The file is in no worktree (or is a worktree's root).
    #[error("the file is in no worktree")]
    NotInWorktree,
    /// Resolving the file's worktree failed.
    #[error("resolving the worktree failed: {0}")]
    Probe(#[from] RepoProbeError),
    /// The view's root could not be listed.
    #[error("the worktree could not be read: {0}")]
    Tree(#[from] SourceTreeError),
    /// The registry refused a transition: a defect of this service.
    #[error("the view registry refused: {0}")]
    View(#[from] ViewError),
    /// A request found no pinnable generation within its rounds of reconciles.
    #[error("no generation became pinnable")]
    NoProgress,
}

/// A pinned request: what it reads from, and how complete that is.
#[derive(Debug, Clone)]
pub struct Answer {
    /// The request, to [`ViewService::finish`].
    pub id: RequestId,
    /// The view it was bound to.
    pub view: FileUri,
    /// Its file under the view's root.
    pub path: RelPath,
    /// The pinned generation: the request reads only from it.
    pub generation: Arc<Published<FileSeen, GitObserved, Summary>>,
    /// Its completeness (ADR 0037), gaps included.
    pub completeness: Completeness,
    /// The view the completeness was computed from.
    pub complete_view: FileUri,
    /// Each cookie's round trip, written to delivered (at most two).
    pub cookie_round_trips: Vec<Duration>,
}

/// An open view's I/O side.
struct Opened<W> {
    worktree: Worktree,
    kind: FsKind,
    watch: Option<W>,
    /// What the last scan could not see.
    scan_gaps: u64,
    /// The roots of other views below this one, as the last scan skipped them (nested worktrees
    /// and repositories, agent worktrees, links) or as they were opened since: their files are
    /// never this view's.
    nested: Vec<RelPath>,
    /// The published generation's gaps: (generation, files counted, files indexed).
    counts: (Generation, u32, u32),
}

/// What a scan found: the paths the view admits, the stats the listing took, its gaps, and the
/// nested roots it skipped.
struct Scan {
    paths: BTreeSet<RelPath>,
    stats: BTreeMap<RelPath, FileStat>,
    gaps: u64,
    nested: Vec<RelPath>,
}

/// One summarised file: its path, what the stat saw before the read, the summary, and whether
/// the indexer made it (so the store does not hold it yet: [`ViewService::write_back`]).
struct Summarised {
    path: RelPath,
    seen: FileSeen,
    summary: Summary,
    fresh: bool,
}

/// Summaries indexed but not yet written to the store, by key.
type Unwritten = BTreeMap<SyntaxKey, Arc<FileSummary>>;

/// Runs `job` over every item, at most `width` at once, and returns the outputs in completion
/// order. Each job awaits the ports, whose blocking work runs on their pool; this future only
/// polls them, so `width` jobs in flight keep that many pool threads busy.
///
/// Pinned by:
/// `view_service::tests::should_keep_at_most_width_jobs_in_flight_and_return_each_output_once_when_jobs_stay_pending`.
pub(crate) async fn bounded<'a, I, T, F, Fut>(width: usize, items: I, job: F) -> Vec<T>
where
    I: IntoIterator,
    F: Fn(I::Item) -> Fut,
    Fut: Future<Output = T> + Send + 'a,
{
    type Job<'a, T> = core::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;
    let width = width.max(1);
    let mut queue = items.into_iter();
    let mut running: Vec<Job<'a, T>> = Vec::with_capacity(width);
    let mut done = Vec::new();
    loop {
        while running.len() < width {
            let Some(item) = queue.next() else {
                break;
            };
            running.push(Box::pin(job(item)));
        }
        if running.is_empty() {
            return done;
        }
        let finished = core::future::poll_fn(|cx| {
            let mut out = Vec::new();
            running.retain_mut(|job| match job.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    out.push(value);
                    false
                }
                Poll::Pending => true,
            });
            if out.is_empty() {
                Poll::Pending
            } else {
                Poll::Ready(out)
            }
        })
        .await;
        done.extend(finished);
    }
}

/// [`ServiceConfig::read_budget`]'s default: 32 MiB.
const READ_BUDGET: usize = 2 * MAX_SOURCE_BYTES;

/// Bytes of source read and not yet parsed, bounded: a permit is granted while the total stays
/// within the limit, or when nothing else is held (so a file larger than the limit is read
/// alone), and every release wakes the waiting tasks.
///
/// Pinned by: `view_service::tests::should_hold_at_most_the_budget_when_reads_ask_for_more`.
struct ReadBudget {
    limit: usize,
    state: Mutex<(usize, Vec<Waker>)>,
}

/// Bytes held of a [`ReadBudget`], returned when dropped.
struct Permit<'a> {
    budget: &'a ReadBudget,
    bytes: usize,
}

impl ReadBudget {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            state: Mutex::new((0, Vec::new())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, (usize, Vec<Waker>)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_acquire(&self, bytes: usize, waker: Option<&Waker>) -> Option<Permit<'_>> {
        let mut state = self.lock();
        let held = state.0;
        if held == 0 || held.saturating_add(bytes) <= self.limit {
            state.0 = held.saturating_add(bytes);
            return Some(Permit {
                budget: self,
                bytes,
            });
        }
        if let Some(waker) = waker {
            state.1.push(waker.clone());
        }
        None
    }

    async fn acquire(&self, bytes: usize) -> Permit<'_> {
        core::future::poll_fn(|cx| match self.try_acquire(bytes, Some(cx.waker())) {
            Some(permit) => Poll::Ready(permit),
            None => Poll::Pending,
        })
        .await
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let waiting = {
            let mut state = self.budget.lock();
            state.0 = state.0.saturating_sub(self.bytes);
            core::mem::take(&mut state.1)
        };
        for waker in waiting {
            waker.wake();
        }
    }
}

/// How many store writes [`ViewService::write_back`] keeps in flight. One at a time, each write
/// waits for a hand-off to a pool thread and back (66 µs on the bench VM), more than a write in
/// key order costs; four threads also write faster than one (`just bench-views`' stages).
const WRITERS: usize = 4;

/// How many names a cookie tries before it is lost ([`cookie_name`]'s attempts).
const COOKIE_ATTEMPTS: u32 = 3;

/// The registry this service drives.
pub type Registry = ViewRegistry<FileSeen, GitObserved, Summary>;

/// The most reconciles one request waits through before giving up ([`ServiceError::NoProgress`]):
/// a request needs at most two publishes (plan §4.3), plus one for its first cookie's events.
const MAX_ROUNDS: u32 = 8;

/// A file's text for the queries, read through the service's ports: only while the file's bytes
/// are the ones a generation's key names, so a position or a Javadoc never comes from a newer
/// file than the summary it is read with (pinned by
/// `tests/queries.rs::should_leave_out_a_file_and_say_so_when_it_changed_after_the_pin`).
#[derive(Debug)]
pub struct ServiceSources<'s, T, X, H> {
    tree: &'s T,
    indexer: &'s X,
    hasher: &'s H,
}

impl<T, X, H> Sources for ServiceSources<'_, T, X, H>
where
    T: SourceTree,
    X: SyntaxIndexer,
    H: KeyHasher + Clone + Send + Sync,
{
    fn text(
        &self,
        root: &FileUri,
        path: &RelPath,
        key: &SyntaxKey,
    ) -> impl Future<Output = Option<Arc<FileText>>> + Send {
        let key = *key;
        async move {
            let read = self
                .tree
                .read(root, path, MAX_SOURCE_BYTES.saturating_add(4))
                .await
                .ok()?;
            if read.truncated {
                return None;
            }
            let bytes = strip_bom(&read.bytes);
            let mut digest = self.hasher.clone();
            digest.update(bytes);
            let grammar = self.indexer.grammar();
            let found = SyntaxKey::derive(
                &SyntaxInputs {
                    file: digest.finish(),
                    grammar: grammar.name,
                    grammar_version: grammar.version,
                    summariser_version: self.indexer.summariser_version(),
                },
                self.hasher.clone(),
            );
            if found != key {
                return None;
            }
            let text = core::str::from_utf8(bytes).ok()?.to_owned();
            FileText::new(text).map(Arc::new)
        }
    }

    fn site_syntax(
        &self,
        text: &Arc<FileText>,
        name: ByteRange,
    ) -> impl Future<Output = Option<SiteSyntax>> + Send {
        self.indexer.site_syntax(Arc::clone(text), name)
    }
}

/// The view service over the ports: `T` the source tree, `P` the repository probe, `X` the syntax
/// indexer, `S` the summary store, `C` the wall clock, `H` the key hasher.
pub struct ViewService<T: SourceTree, P, X, S, C, H> {
    tree: T,
    probe: P,
    indexer: X,
    store: S,
    clock: C,
    hasher: H,
    config: ServiceConfig,
    registry: Registry,
    views: BTreeMap<FileUri, Opened<T::Watch>>,
    /// Each cookie waited for: (root, stamp) to its request.
    cookies: BTreeMap<(FileUri, u64), RequestId>,
    /// Summaries the indexer made that the store does not hold yet, by key: written after
    /// the publish, by [`ViewService::write_back`], since a generation holds its own summaries
    /// and the store is only the cache a view reopens from.
    unwritten: Unwritten,
    next_request: u64,
    stamp: u64,
}

enum Either<A, B> {
    Left(A),
    Right(B),
}

/// `a` or `b`, whichever resolves first, `a` when both are ready.
async fn first<A: Future, B: Future>(a: A, b: B) -> Either<A::Output, B::Output> {
    let mut a = pin!(a);
    let mut b = pin!(b);
    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = a.as_mut().poll(cx) {
            return Poll::Ready(Either::Left(out));
        }
        if let Poll::Ready(out) = b.as_mut().poll(cx) {
            return Poll::Ready(Either::Right(out));
        }
        Poll::Pending
    })
    .await
}

/// Whether `path` is `dir` or below it.
fn under(path: &RelPath, dir: &RelPath) -> bool {
    path.as_str()
        .strip_prefix(dir.as_str())
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The files of `published` (present ones and gaps, plus `scan_gaps` the scan could not see) and
/// how many of them have a summary: fewer is `ModulePartlyIndexed`.
fn count(published: &Published<FileSeen, GitObserved, Summary>, scan_gaps: u64) -> (u32, u32) {
    let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let gap = |s: &Option<Summary>| matches!(s, Some(Summary::Gap | Summary::Pending) | None);
    let entries = published
        .index
        .values()
        .filter(|e| e.seen.stat.is_some() || gap(&e.summary))
        .count();
    let indexed = published
        .index
        .values()
        .filter(|e| matches!(e.summary, Some(Summary::Indexed { .. })))
        .count();
    let scan_gaps = usize::try_from(scan_gaps).unwrap_or(usize::MAX);
    (to_u32(entries.saturating_add(scan_gaps)), to_u32(indexed))
}

/// A stat value no earlier stat can prove unchanged: what an unknown file is compared with.
const UNKNOWN: FileSeen = FileSeen {
    stat: None,
    racy: true,
};

/// The stat a deferred file the listing did not stat (a tracked file the walk skipped) records:
/// present, racy, so the file is read before anything is claimed of it.
const UNLISTED: FileStat = FileStat {
    size: 0,
    mtime: MTime { secs: 0, nanos: 0 },
    inode: 0,
};

/// The ports a file's read needs, borrowed: what the concurrent reads of
/// [`ViewService::summarise_all`] share (the open watches, which are `Send` but not `Sync`,
/// stay behind).
struct Reader<'a, T, X, S, C, H> {
    tree: &'a T,
    indexer: &'a X,
    store: &'a S,
    clock: &'a C,
    hasher: &'a H,
    granularity: Duration,
    /// Summaries made since the last write-back: read before the store, so a view opened before
    /// they are written does not index the same content again.
    unwritten: &'a Unwritten,
    /// The bytes of source the files in flight may hold.
    budget: &'a ReadBudget,
    /// The store held nothing when the reads began, so a lookup can only miss: a cold open skips
    /// a pool round trip per file. Only this service writes the store, and only in
    /// [`ViewService::write_back`], which no read overlaps.
    store_empty: bool,
}

/// The stat of `root/path`: `None` when nothing is there, `Err` when it could not be read.
async fn stat_of<T: SourceTree>(
    tree: &T,
    root: &FileUri,
    path: &RelPath,
) -> Result<Option<FileStat>, ()> {
    match tree.stat(root, path).await {
        Ok(stat) => Ok(Some(stat)),
        Err(SourceTreeError::NotFound | SourceTreeError::NotAFile) => Ok(None),
        Err(_) => Err(()),
    }
}

impl<T, X, S, C, H> Reader<'_, T, X, S, C, H>
where
    T: SourceTree,
    X: SyntaxIndexer,
    S: SummaryStore,
    C: WallClock,
    H: KeyHasher + Clone + Send + Sync,
{
    /// Reads and summarises `path`, through the store: what the stat saw before the read, the
    /// summary, and whether the indexer made it. `known` is a stat already taken before this
    /// read (a listing's or a rescan's, the clock read before it): the read follows it, so it is
    /// what the entry records; without one the file is stat'ed first.
    async fn summarise(
        &self,
        root: &FileUri,
        path: &RelPath,
        known: Option<FileSeen>,
    ) -> (FileSeen, Summary, bool) {
        let seen = match known {
            Some(seen) => seen,
            None => {
                let at = self.clock.now();
                match stat_of(self.tree, root, path).await {
                    Ok(stat) => FileSeen::new(stat, at, self.granularity),
                    Err(()) => return (UNKNOWN, Summary::Gap, false),
                }
            }
        };
        let Some(stat) = seen.stat else {
            return (seen, Summary::Absent, false);
        };
        // Held from the read to the end of the parse, which owns the bytes.
        let want = usize::try_from(stat.size)
            .unwrap_or(usize::MAX)
            .min(MAX_SOURCE_BYTES.saturating_add(4));
        let _permit = self.budget.acquire(want).await;
        let read = match self
            .tree
            .read(root, path, MAX_SOURCE_BYTES.saturating_add(4))
            .await
        {
            Ok(read) => read,
            Err(SourceTreeError::NotFound | SourceTreeError::NotAFile) => {
                return (UNKNOWN, Summary::Absent, false);
            }
            Err(_) => return (seen, Summary::Gap, false),
        };
        if read.truncated {
            return (seen, Summary::Gap, false);
        }
        let mut digest = self.hasher.clone();
        digest.update(strip_bom(&read.bytes));
        let grammar = self.indexer.grammar();
        let key = SyntaxKey::derive(
            &SyntaxInputs {
                file: digest.finish(),
                grammar: grammar.name,
                grammar_version: grammar.version,
                summariser_version: self.indexer.summariser_version(),
            },
            self.hasher.clone(),
        );
        if let Some(summary) = self.unwritten.get(&key) {
            return (
                seen,
                Summary::Indexed {
                    key,
                    summary: Arc::clone(summary),
                },
                false,
            );
        }
        if !self.store_empty
            && let Ok(Some(summary)) = self.store.get(key).await
        {
            return (
                seen,
                Summary::Indexed {
                    key,
                    summary: Arc::new(summary),
                },
                false,
            );
        }
        match self.indexer.index(read.bytes).await {
            Ok(indexed) => (
                seen,
                Summary::Indexed {
                    key,
                    summary: Arc::new(indexed.summary),
                },
                true,
            ),
            Err(_) => (seen, Summary::Gap, false),
        }
    }
}

impl<T, P, X, S, C, H> ViewService<T, P, X, S, C, H>
where
    T: SourceTree,
    P: RepoProbe,
    X: SyntaxIndexer,
    S: SummaryStore,
    C: WallClock + Clone,
    H: KeyHasher + Clone + Send + Sync,
{
    /// A service with no view open.
    pub fn new(ports: (T, P, X, S), clock: C, hasher: H, config: ServiceConfig) -> Self {
        let (tree, probe, indexer, store) = ports;
        Self {
            tree,
            probe,
            indexer,
            store,
            clock,
            hasher,
            config,
            registry: Registry::new(),
            views: BTreeMap::new(),
            cookies: BTreeMap::new(),
            unwritten: BTreeMap::new(),
            next_request: 1,
            stamp: 0,
        }
    }

    /// The registry, to read views and requests.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The source tree, the repository probe and the summary store.
    #[must_use]
    pub fn ports(&self) -> (&T, &P, &S) {
        (&self.tree, &self.probe, &self.store)
    }

    /// What the queries read a file's text through ([`crate::queries`]): the source tree, the
    /// text checked against a generation's key with this service's indexer and hasher.
    #[must_use]
    pub fn sources(&self) -> ServiceSources<'_, T, X, H> {
        ServiceSources {
            tree: &self.tree,
            indexer: &self.indexer,
            hasher: &self.hasher,
        }
    }

    /// The filesystem kind the view of `root` was opened on.
    #[must_use]
    pub fn filesystem(&self, root: &FileUri) -> Option<FsKind> {
        self.views.get(root).map(|o| o.kind)
    }

    fn tick(&mut self) -> Stamp {
        self.stamp = self.stamp.saturating_add(1);
        Stamp(self.stamp)
    }

    fn seen(&self, stat: Option<FileStat>, at: MTime) -> FileSeen {
        FileSeen::new(stat, at, self.config.granularity)
    }

    /// The Git stat set of `worktree`, the clock read first.
    async fn git(&self, worktree: &Worktree) -> GitSeen<GitObserved> {
        let at = self.clock.now();
        let snapshot = self.probe.snapshot(worktree).await;
        observe_git(snapshot, at, self.config.granularity)
    }

    async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<Option<FileStat>, ()> {
        stat_of(&self.tree, root, path).await
    }

    fn reader<'a>(&'a self, budget: &'a ReadBudget) -> Reader<'a, T, X, S, C, H> {
        Reader {
            budget,
            tree: &self.tree,
            indexer: &self.indexer,
            store: &self.store,
            clock: &self.clock,
            hasher: &self.hasher,
            granularity: self.config.granularity,
            unwritten: &self.unwritten,
            store_empty: self.store.stats().entries == 0,
        }
    }

    /// The listing of `worktree` unioned with its tracked files, leaving out what is a view of
    /// its own (a nested worktree or repository, agent worktrees) or a link: the paths the view
    /// admits. `None` when the root could not be listed.
    async fn scan(&self, worktree: &Worktree) -> Option<Scan> {
        let listing = self
            .tree
            .list(&worktree.root, ListMode::PathsAndStat)
            .await
            .ok()?;
        let mut gaps = listing.unlisted_count;
        let nested: Vec<RelPath> = listing
            .skipped
            .iter()
            .filter(|s| s.why != Exclusion::BuildOutput)
            .map(|s| s.path.clone())
            .collect();
        let mut stats = BTreeMap::new();
        let mut paths = BTreeSet::new();
        for file in &listing.files {
            if let Some(stat) = file.stat {
                stats.insert(file.path.clone(), stat);
            }
            paths.insert(file.path.clone());
        }
        match self.probe.tracked_java(worktree).await {
            Ok(tracked) => {
                gaps = gaps.saturating_add(tracked.unrepresentable);
                for path in tracked.files {
                    if !nested.iter().any(|o| under(&path, o)) {
                        paths.insert(path);
                    }
                }
            }
            Err(_) => gaps = gaps.saturating_add(1),
        }
        Some(Scan {
            paths,
            stats,
            gaps,
            nested,
        })
    }

    /// Summarises `files` (each with a stat taken before, when there is one),
    /// [`ServiceConfig::parallelism`] at a time: each stat, read, store lookup and parse runs on
    /// the ports' blocking pool, so a cold open uses every core the pool has. The summaries the
    /// indexer made wait in [`ViewService::write_back`]'s queue.
    async fn summarise_all(
        &mut self,
        root: &FileUri,
        files: Vec<(RelPath, Option<FileSeen>)>,
    ) -> Vec<Summarised> {
        let budget = ReadBudget::new(self.config.read_budget);
        let done = {
            let reader = &self.reader(&budget);
            bounded(self.config.parallelism, files, |(path, known)| async move {
                let (seen, summary, fresh) = reader.summarise(root, &path, known).await;
                Summarised {
                    path,
                    seen,
                    summary,
                    fresh,
                }
            })
            .await
        };
        for file in &done {
            if let (true, Summary::Indexed { key, summary }) = (file.fresh, &file.summary)
                && self.unwritten.len() < self.config.unwritten_cap
            {
                self.unwritten.insert(*key, Arc::clone(summary));
            }
        }
        done
    }

    /// The stat of every one of `paths`, [`ServiceConfig::parallelism`] at a time, the clock
    /// read first (`at`): what a rescan compares with the published entries.
    async fn stat_all(
        &self,
        root: &FileUri,
        paths: Vec<RelPath>,
        at: MTime,
    ) -> BTreeMap<RelPath, FileSeen> {
        let tree = &self.tree;
        let granularity = self.config.granularity;
        bounded(self.config.parallelism, paths, |path| async move {
            let seen = match stat_of(tree, root, &path).await {
                Ok(stat) => FileSeen::new(stat, at, granularity),
                Err(()) => UNKNOWN,
            };
            (path, seen)
        })
        .await
        .into_iter()
        .collect()
    }

    /// Recounts the published generation of `root` ([`count`]).
    fn recount(&mut self, root: &FileUri) {
        let Some(view) = self.registry.view(root) else {
            return;
        };
        let published = &view.published;
        if let Some(opened) = self.views.get_mut(root) {
            let (files, indexed) = count(published, opened.scan_gaps);
            opened.counts = (published.generation, files, indexed);
        }
    }

    /// Opens the view of `worktree` unless it is open: filesystem, watch, then scan, then publish.
    /// An open view whose watch was lost and could not be re-established is watched again here.
    /// A view opened inside another open view is a nested root of that view from now on, which
    /// rescans: whatever of it the parent indexed before it was a worktree leaves the parent.
    ///
    /// A view of more than [`ServiceConfig::open_whole`] files reads only the directory of
    /// `first` (the request's file) and publishes every other listed file as
    /// [`Summary::Pending`], racy, so the request answers without waiting for the whole view
    /// (partly indexed) and [`ViewService::settle`] reads the rest. Each file read here was
    /// stat'ed by the listing, after the clock: that stat comes before the read, so it is the
    /// entry's.
    async fn open(&mut self, worktree: Worktree, first: &RelPath) -> Result<(), ServiceError> {
        let root = worktree.root.clone();
        if let Some(opened) = self.views.get(&root) {
            if opened.watch.is_none()
                && opened.kind.watchable()
                && let Ok(watch) = self.tree.watch(&root).await
            {
                self.registry.watch_restored(&root)?;
                if let Some(opened) = self.views.get_mut(&root) {
                    opened.watch = Some(watch);
                }
            }
            return Ok(());
        }
        let kind = self.tree.filesystem(&root).await.unwrap_or(FsKind::Other);
        let watch = if kind.watchable() {
            self.tree.watch(&root).await.ok()
        } else {
            None
        };
        let now = self.clock.now();
        let age = i64::try_from(self.config.cookie_sweep_age.as_secs()).unwrap_or(i64::MAX);
        let before = MTime {
            secs: now.secs.saturating_sub(age),
            nanos: now.nanos,
        };
        // A stale cookie is a stray file in the user's tree; failing to remove it costs nothing
        // here, so the sweep's error is not the open's.
        let _ = self.tree.sweep_cookies(&root, before).await;
        let git = self.git(&worktree).await;
        let at = self.clock.now();
        let scan = self
            .scan(&worktree)
            .await
            .ok_or(ServiceError::Tree(SourceTreeError::RootNotFound))?;
        let whole = scan.paths.len() <= self.config.open_whole;
        let package = first.as_str().rsplit_once('/').map_or("", |(dir, _)| dir);
        let in_package = |path: &RelPath| {
            path.as_str()
                .rsplit_once('/')
                .map_or("", |(dir, _)| dir)
                .eq(package)
        };
        let mut now = Vec::new();
        let mut later = Vec::new();
        for path in scan.paths {
            let stat = scan.stats.get(&path).copied();
            if whole || in_package(&path) {
                now.push((path, stat.map(|s| self.seen(Some(s), at))));
            } else {
                // Listed, not read: racy, so no stat proves it fresh and a stat-less listing
                // entry (a tracked file the walk skipped) still counts as present.
                let seen = FileSeen {
                    stat: Some(stat.unwrap_or(UNLISTED)),
                    racy: true,
                };
                later.push((path, seen, Summary::Pending));
            }
        }
        let files: Vec<(RelPath, FileSeen, Summary)> = self
            .summarise_all(&root, now)
            .await
            .into_iter()
            .filter(|f| !matches!(f.summary, Summary::Absent))
            .map(|f| (f.path, f.seen, f.summary))
            .chain(later)
            .collect();
        let watched = watch.is_some();
        self.registry.open(root.clone(), watched, git, files)?;
        let parents: Vec<(FileUri, RelPath)> = self
            .views
            .keys()
            .filter_map(|parent| root.relative_to(parent).map(|rel| (parent.clone(), rel)))
            .collect();
        for (parent, rel) in parents {
            if let Some(opened) = self.views.get_mut(&parent)
                && !opened.nested.iter().any(|n| under(&rel, n))
            {
                opened.nested.push(rel);
                self.registry.deliver(Event::Git { view: parent })?;
            }
        }
        self.views.insert(
            root.clone(),
            Opened {
                worktree,
                kind,
                watch,
                scan_gaps: scan.gaps,
                nested: scan.nested,
                counts: (Generation(0), 0, 0),
            },
        );
        self.recount(&root);
        Ok(())
    }

    /// Takes every waiting hint of every open view and applies it.
    async fn drain(&mut self) -> Result<(), ServiceError> {
        let mut taken = Vec::new();
        for (root, opened) in &mut self.views {
            if let Some(watch) = opened.watch.as_mut() {
                while let Some(hint) = watch.try_next() {
                    taken.push((root.clone(), hint));
                }
            }
        }
        for (root, hint) in taken {
            self.apply(&root, hint).await?;
        }
        Ok(())
    }

    /// Whether `path` lies under a nested root of the view of `root`: never the view's file.
    fn nested(&self, root: &FileUri, path: &RelPath) -> bool {
        self.views
            .get(root)
            .is_some_and(|o| o.nested.iter().any(|n| under(path, n)))
    }

    /// One hint of `root`'s watch. A `.java` file the view holds is a file event; one under a
    /// nested root is not the view's and is dropped; any other path (a new file, which may be
    /// under a directory no scan has seen, `.gitignore`d or a worktree just created; a directory;
    /// a `.gitignore` or build file) asks for a rescan, whose listing decides what the view holds.
    async fn apply(&mut self, root: &FileUri, hint: Hint) -> Result<(), ServiceError> {
        match hint {
            Hint::Changed(path) => {
                if let Some(stamp) = cookie_stamp(&self.config.owner, &path) {
                    if let Some(id) = self.cookies.get(&(root.clone(), stamp)).copied() {
                        self.registry.deliver(Event::Cookie {
                            id,
                            at: Stamp(stamp),
                        })?;
                    }
                } else if is_cookie(&path) || self.nested(root, &path) {
                    // Another daemon's cookie, or another view's file: no change of this view.
                } else if is_java_file_name(path.file_name())
                    && self
                        .registry
                        .view(root)
                        .is_some_and(|v| v.published.index.contains_key(&path))
                {
                    self.registry.deliver(Event::File {
                        view: root.clone(),
                        path,
                    })?;
                } else {
                    self.registry.deliver(Event::Git { view: root.clone() })?;
                }
            }
            Hint::Rescan(rescan) => self.rescan_hint(root, rescan).await?,
        }
        Ok(())
    }

    async fn rescan_hint(&mut self, root: &FileUri, rescan: Rescan) -> Result<(), ServiceError> {
        if rescan.reasons.degrade_the_watch() {
            let watched = self.registry.view(root).is_some_and(|v| v.watched);
            if watched {
                self.registry.watch_lost(root)?;
            }
            if let Some(opened) = self.views.get_mut(root) {
                opened.watch = None;
            }
            if self.views.get(root).is_some_and(|o| o.kind.watchable())
                && let Ok(watch) = self.tree.watch(root).await
            {
                self.registry.watch_restored(root)?;
                if let Some(opened) = self.views.get_mut(root) {
                    opened.watch = Some(watch);
                }
            }
        } else {
            self.registry.overflow();
        }
        let queued: Vec<RequestId> = self
            .cookies
            .iter()
            .filter(|((r, _), _)| r == root)
            .map(|(_, id)| *id)
            .filter(|id| {
                self.registry
                    .request(*id)
                    .is_some_and(|r| r.cookie == Cookie::Queued)
            })
            .collect();
        for id in queued {
            self.registry.cookie_lost(id)?;
        }
        Ok(())
    }

    /// Writes request `id`'s cookie stamped `stamp` into `root` and waits for it until the
    /// deadline: its round trip when it came back.
    async fn cookie(
        &mut self,
        id: RequestId,
        root: &FileUri,
        stamp: Stamp,
    ) -> Result<Option<Duration>, ServiceError> {
        let key = (root.clone(), stamp.0);
        self.cookies.insert(key.clone(), id);
        let written = self.clock.now();
        let result = self.wait_cookie(id, root, stamp).await;
        self.cookies.remove(&key);
        result?;
        let delivered = self
            .registry
            .request(id)
            .is_some_and(|r| r.cookie == Cookie::Delivered);
        Ok(delivered.then(|| elapsed(written, self.clock.now())))
    }

    /// Writes the cookie, under the next attempt's name when a file already holds one (never
    /// removing it: it may not be ours), at most [`COOKIE_ATTEMPTS`] names.
    async fn write_cookie(&self, root: &FileUri, stamp: Stamp) -> bool {
        for attempt in 0..COOKIE_ATTEMPTS {
            let Some(name) = cookie_name(&self.config.owner, stamp.0, attempt) else {
                return false;
            };
            match self.tree.write_cookie(root, &name).await {
                Ok(()) => return true,
                Err(SourceTreeError::Io(std::io::ErrorKind::AlreadyExists)) => {}
                Err(_) => return false,
            }
        }
        false
    }

    async fn wait_cookie(
        &mut self,
        id: RequestId,
        root: &FileUri,
        stamp: Stamp,
    ) -> Result<(), ServiceError> {
        if !self.write_cookie(root, stamp).await {
            self.registry.cookie_lost(id)?;
            return Ok(());
        }
        let clock = self.clock.clone();
        let mut deadline = pin!(clock.deadline(self.config.cookie_deadline));
        loop {
            if self
                .registry
                .request(id)
                .is_none_or(|r| r.cookie != Cookie::Queued)
            {
                return Ok(());
            }
            let Some(watch) = self.views.get_mut(root).and_then(|o| o.watch.as_mut()) else {
                self.registry.cookie_lost(id)?;
                return Ok(());
            };
            let hint = match watch.try_next() {
                Some(hint) => hint,
                None => match first(watch.next(), deadline.as_mut()).await {
                    Either::Left(Some(hint)) => hint,
                    Either::Left(None) => Hint::Rescan(Rescan {
                        reasons: RescanReasons::of(RescanReason::WatchEnded),
                        dropped: 0,
                    }),
                    Either::Right(()) => {
                        self.registry.cookie_lost(id)?;
                        return Ok(());
                    }
                },
            };
            self.apply(root, hint).await?;
        }
    }

    /// Reconciles `root` when a delivered event or a rescan waits: re-stat, re-read what changed,
    /// publish. On a rescan, the view is listed first, after its watch is up, and every path the
    /// listing (unioned with the tracked files) does not admit leaves the view; without one, a
    /// path the view never held (a request's stat of a new file) is left out and a rescan asked
    /// for, whose listing decides.
    async fn reconcile(&mut self, root: &FileUri) -> Result<bool, ServiceError> {
        let Some(view) = self.registry.view(root) else {
            return Err(ServiceError::View(ViewError::UnknownView));
        };
        let rescan = view.rescan;
        if !rescan && view.dirty.is_empty() {
            return Ok(false);
        }
        let dirty: Vec<RelPath> = view.dirty.iter().cloned().collect();
        let indexed: Vec<RelPath> = if rescan {
            view.published.index.keys().cloned().collect()
        } else {
            Vec::new()
        };
        let Some(worktree) = self.views.get(root).map(|o| o.worktree.clone()) else {
            return Err(ServiceError::View(ViewError::UnknownView));
        };
        let git = self.git(&worktree).await;
        let at = self.clock.now();
        let mut stats: BTreeMap<RelPath, FileSeen> = BTreeMap::new();
        let mut listed = None;
        // Without a rescan only the dirty paths are read, and each read stats its file first; the
        // build's entry for a path waiting to be read is replaced by that stat before any
        // publish, so it needs none here.
        if rescan {
            let mut wanted = dirty;
            if let Some(scan) = self.scan(&worktree).await {
                for (path, stat) in scan.stats {
                    stats.insert(path, self.seen(Some(stat), at));
                }
                wanted.extend(scan.paths.iter().cloned());
                listed = Some(scan.paths);
                if let Some(opened) = self.views.get_mut(root) {
                    opened.scan_gaps = scan.gaps;
                    opened.nested = scan.nested;
                }
            }
            wanted.extend(indexed);
            wanted.retain(|path| !stats.contains_key(path));
            wanted.sort_unstable();
            wanted.dedup();
            let more = self.stat_all(root, wanted, at).await;
            stats.extend(more);
        }
        let mut todo = self.registry.reconcile_start(
            root,
            listed.as_ref(),
            |p| stats.get(p).copied().unwrap_or(UNKNOWN),
            || git,
        )?;
        let (excluded, escalate) = self.inadmissible(root, listed.as_ref(), &todo);
        for path in &excluded {
            self.registry.exclude(root, path)?;
            todo.remove(path);
        }
        let files = todo
            .into_iter()
            .map(|path| {
                let known = stats.get(&path).copied();
                (path, known)
            })
            .collect();
        for file in self.summarise_all(root, files).await {
            let stamp = self.tick();
            self.registry
                .summarised(root, &file.path, file.seen, file.summary, stamp)?;
        }
        self.registry.publish(root, &Held)?;
        self.recount(root);
        if escalate {
            self.registry.deliver(Event::Git { view: root.clone() })?;
        }
        Ok(true)
    }

    /// The paths of `root`'s build the view does not admit, and whether a rescan must decide
    /// some of them. With a listing: every path outside it (a nested root's, a `.gitignore`d
    /// one's, one that leaked in before). Without: each path to read that lies under a nested
    /// root, or that the published generation never held (a rescan decides those).
    fn inadmissible(
        &self,
        root: &FileUri,
        listed: Option<&BTreeSet<RelPath>>,
        todo: &BTreeSet<RelPath>,
    ) -> (Vec<RelPath>, bool) {
        let Some(view) = self.registry.view(root) else {
            return (Vec::new(), false);
        };
        if let Some(listed) = listed {
            let outside = view
                .build
                .iter()
                .flat_map(|b| b.index.keys())
                .filter(|p| !listed.contains(*p))
                .cloned()
                .collect();
            return (outside, false);
        }
        let mut escalate = false;
        let mut out = Vec::new();
        for path in todo {
            if self.nested(root, path) {
                out.push(path.clone());
            } else if !view.published.index.contains_key(path) {
                escalate = true;
                out.push(path.clone());
            }
        }
        (out, escalate)
    }

    /// Answers a request on `file`: bind, stat, cookie, reconcile, second cookie, pin. The answer
    /// holds the pinned generation until [`ViewService::finish`].
    pub async fn request(&mut self, file: &FileUri) -> Result<Answer, ServiceError> {
        let worktree = self
            .probe
            .resolve(file)
            .await?
            .ok_or(ServiceError::NotInWorktree)?;
        let root = worktree.root.clone();
        let path = file.relative_to(&root).ok_or(ServiceError::NotInWorktree)?;
        self.open(worktree.clone(), &path).await?;
        if self.registry.bind(file).map(|(r, _)| r) != Some(&root) {
            // A deeper view is open for a worktree `resolve` no longer names (removed since).
            return Err(ServiceError::NotInWorktree);
        }
        self.drain().await?;
        let id = RequestId(self.next_request);
        self.next_request = self.next_request.saturating_add(1);
        let at = self.clock.now();
        let seen = match self.stat(&root, &path).await {
            Ok(stat) => self.seen(stat, at),
            Err(()) => UNKNOWN,
        };
        let git = self.git(&worktree).await;
        let stamp = self.tick();
        let started = self.registry.request_start(id, file, &seen, &git, stamp)?;
        let mut round_trips = Vec::new();
        if started.write_cookie {
            round_trips.extend(self.cookie(id, &root, stamp).await?);
        }
        let mut voluntary = false;
        for _ in 0..MAX_ROUNDS {
            self.drain().await?;
            let (pinnable, due, cookies) = self.progress(id, &root)?;
            if !pinnable || (due && !voluntary) {
                voluntary |= pinnable;
                self.reconcile(&root).await?;
                continue;
            }
            if cookies < 2 {
                let stamp = self.tick();
                match self.registry.request_recookie(id, stamp) {
                    Ok(_) => {
                        round_trips.extend(self.cookie(id, &root, stamp).await?);
                        continue;
                    }
                    Err(ViewError::NoRecookie) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let git = self.git(&worktree).await;
            let opened = self.views.get(&root).map(|o| (o.counts, o.scan_gaps));
            let pin = self.registry.request_pin(id, &git)?;
            let mut completeness = pin.completeness.clone();
            let (files, indexed) = match opened {
                Some(((generation, files, indexed), _))
                    if generation == pin.generation.generation =>
                {
                    (files, indexed)
                }
                Some((_, gaps)) => count(&pin.generation, gaps),
                None => count(&pin.generation, 1),
            };
            if indexed < files {
                completeness = completeness.and(IncompleteReason::ModulePartlyIndexed {
                    module: ModuleName::worktree(),
                    indexed,
                    files,
                });
            }
            return Ok(Answer {
                id,
                view: root,
                path,
                generation: Arc::clone(&pin.generation),
                completeness,
                complete_view: pin.complete_view.clone(),
                cookie_round_trips: round_trips,
            });
        }
        Err(ServiceError::NoProgress)
    }

    /// Whether request `id` may pin, whether a reconcile of `root` is due, and how many cookies
    /// it wrote.
    fn progress(&self, id: RequestId, root: &FileUri) -> Result<(bool, bool, u8), ServiceError> {
        let request = self.registry.request(id).ok_or(ViewError::UnknownRequest)?;
        let view = self.registry.view(root).ok_or(ViewError::UnknownView)?;
        let pinnable = view.published.generation >= request.need;
        let due = view.rescan || !view.dirty.is_empty();
        Ok((pinnable, due, request.cookies))
    }

    /// The request answered: its pin is dropped, so its generation is freed once no other
    /// request holds it.
    pub fn finish(&mut self, id: RequestId) -> Result<(), ServiceError> {
        self.registry.finish(id)?;
        self.registry.forget(id)?;
        Ok(())
    }

    /// `sync` on the view of `file`'s worktree: returns once a rescan started after the call has
    /// published (plan §4.3).
    pub async fn sync(&mut self, file: &FileUri) -> Result<Generation, ServiceError> {
        let worktree = self
            .probe
            .resolve(file)
            .await?
            .ok_or(ServiceError::NotInWorktree)?;
        let root = worktree.root.clone();
        let path = file.relative_to(&root).ok_or(ServiceError::NotInWorktree)?;
        self.open(worktree, &path).await?;
        self.drain().await?;
        self.registry.sync_start(&root)?;
        for _ in 0..MAX_ROUNDS {
            self.reconcile(&root).await?;
            if self.registry.view(&root).is_some_and(|v| v.sync.scanned) {
                self.registry.sync_done(&root)?;
                let generation = self
                    .registry
                    .view(&root)
                    .map_or(Generation(0), |v| v.published.generation);
                return Ok(generation);
            }
            self.drain().await?;
        }
        Err(ServiceError::NoProgress)
    }

    /// Reads every file an open left [`Summary::Pending`] in each open view, as one build of
    /// file events (the model's `deliver`, `reconcileStart`, `summarise` and `publish`), and
    /// publishes it. Whether any view had one. The daemon calls it as soon as the request that
    /// opened a view has its answer; until then that view answers partly indexed.
    ///
    /// Pinned by: `tests/view_service.rs::should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest`.
    pub async fn settle(&mut self) -> Result<bool, ServiceError> {
        self.drain().await?;
        let mut any = false;
        let roots: Vec<FileUri> = self.views.keys().cloned().collect();
        for root in roots {
            let pending: Vec<RelPath> = self.registry.view(&root).map_or_else(Vec::new, |v| {
                v.published
                    .index
                    .iter()
                    .filter(|(_, e)| matches!(e.summary, Some(Summary::Pending)))
                    .map(|(path, _)| path.clone())
                    .collect()
            });
            if pending.is_empty() {
                continue;
            }
            any = true;
            for path in pending {
                self.registry.deliver(Event::File {
                    view: root.clone(),
                    path,
                })?;
            }
            self.reconcile(&root).await?;
        }
        Ok(any)
    }

    /// Writes at most `budget` of the summaries the indexer made to the store, [`WRITERS`] at a
    /// time; how many wait still (at most [`ServiceConfig::unwritten_cap`] wait: past it a new
    /// summary is not kept for the store, and the next open re-indexes its content). A generation holds its own summaries, so nothing waits
    /// for these writes but the next open of the same content: they run after the publish (a
    /// cold open's writes cost as much as its parse on four threads, and do not scale across
    /// threads: `doc/bench/README.md`). A failed write leaves the store without the summary,
    /// which the next open re-indexes.
    ///
    /// Pinned by: `tests/view_service.rs::should_store_the_summaries_only_when_written_back`.
    pub async fn write_back(&mut self, budget: usize) -> usize {
        let mut batch = Vec::new();
        while batch.len() < budget {
            let Some(entry) = self.unwritten.pop_first() else {
                break;
            };
            batch.push(entry);
        }
        let store = &self.store;
        bounded(WRITERS, batch, |(key, summary)| async move {
            // A failed write leaves the store without it: the next open re-indexes the file.
            let _ = store.put(key, summary).await;
        })
        .await;
        self.unwritten.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_probe::StatRecord;

    fn stat(secs: i64) -> StatRecord {
        let at = MTime { secs, nanos: 0 };
        StatRecord {
            dev: 1,
            inode: 2,
            size: 3,
            mtime: at,
            ctime: at,
        }
    }

    fn snapshot(branch: i64) -> Result<GitSnapshot, RepoProbeError> {
        let uri = FileUri::from_path("/r/.git/HEAD").map_err(|_| RepoProbeError::NotFound)?;
        let entry = |file, secs| GitStat {
            file,
            path: uri.clone(),
            stat: Some(stat(secs)),
        };
        Ok(GitSnapshot {
            head: HeadState::Symbolic("refs/heads/main".to_owned()),
            files: vec![
                entry(GitFile::Head, 1),
                entry(GitFile::Ref, branch),
                entry(GitFile::PackedRefs, 1),
                entry(GitFile::Index, 1),
            ],
        })
    }

    /// Polls `future` with a no-op waker until it is ready, at most `polls` times.
    fn run<F: Future>(future: F, polls: usize) -> Option<F::Output> {
        let mut future = pin!(future);
        let mut cx = core::task::Context::from_waker(Waker::noop());
        (0..polls).find_map(|_| match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => Some(out),
            Poll::Pending => None,
        })
    }

    #[test]
    fn should_keep_at_most_width_jobs_in_flight_and_return_each_output_once_when_jobs_stay_pending()
    {
        // given
        let active = core::sync::atomic::AtomicUsize::new(0);
        let peak = core::sync::atomic::AtomicUsize::new(0);
        let job = |i: usize| {
            let (active, peak) = (&active, &peak);
            let mut started = false;
            core::future::poll_fn(move |cx| {
                if started {
                    active.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                    return Poll::Ready(i);
                }
                started = true;
                let now = active.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                peak.fetch_max(now.saturating_add(1), core::sync::atomic::Ordering::Relaxed);
                cx.waker().wake_by_ref();
                Poll::Pending
            })
        };

        // when
        let mut out = run(bounded(3, 0..10, job), 100).unwrap();

        // then
        out.sort_unstable();
        assert_eq!(out, (0..10).collect::<Vec<_>>());
        assert_eq!(peak.load(core::sync::atomic::Ordering::Relaxed), 3);
    }

    #[test]
    fn should_hold_at_most_the_budget_when_reads_ask_for_more() {
        // given
        let budget = ReadBudget::new(10);
        let first = budget.try_acquire(6, None).unwrap();

        // when
        let refused = budget.try_acquire(6, None).is_none();
        let fits = budget.try_acquire(4, None).unwrap();
        drop(first);
        let after = budget.try_acquire(6, None);
        drop((fits, after));
        let alone = budget.try_acquire(25, None);

        // then
        assert!(refused);
        assert!(
            alone.is_some(),
            "a file larger than the budget is read alone"
        );
        drop(alone);
        assert_eq!(budget.lock().0, 0);
    }

    #[test]
    fn should_wake_a_waiting_read_when_a_permit_is_released() {
        // given
        let budget = ReadBudget::new(10);
        let held = budget.try_acquire(8, None).unwrap();
        let woken = Arc::new(Flag::default());
        let waker = Waker::from(Arc::clone(&woken));
        let mut cx = core::task::Context::from_waker(&waker);
        let mut waiting = pin!(budget.acquire(5));

        // when
        let before = waiting.as_mut().poll(&mut cx).is_pending();
        drop(held);

        // then
        assert!(before);
        assert!(woken.0.load(core::sync::atomic::Ordering::Relaxed));
        assert!(waiting.as_mut().poll(&mut cx).is_ready());
    }

    #[derive(Default)]
    struct Flag(core::sync::atomic::AtomicBool);

    impl std::task::Wake for Flag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, core::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    fn should_cover_the_branch_ref_when_head_is_observed() {
        // given
        let at = MTime {
            secs: 100,
            nanos: 0,
        };
        let granularity = Duration::from_millis(10);
        let before = observe_git(snapshot(2), at, granularity);

        // when
        let committed = observe_git(snapshot(3), at, granularity);
        let same = observe_git(snapshot(2), at, granularity);
        let failed = observe_git(
            Err(RepoProbeError::Io(std::io::ErrorKind::Other)),
            at,
            granularity,
        );

        // then
        assert!(!before.head.proves_unchanged(&committed.head));
        assert!(before.index.proves_unchanged(&committed.index));
        assert!(before.head.proves_unchanged(&same.head));
        assert!(!failed.head.proves_unchanged(&failed.head));
        let racy = observe_git(snapshot(2), MTime { secs: 2, nanos: 0 }, granularity);
        assert!(!racy.head.proves_unchanged(&same.head));
    }
}
