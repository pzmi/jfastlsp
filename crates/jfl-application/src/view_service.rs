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
//!   ([`SourceTree::sweep_cookies`]), reading the clock and `HEAD`'s stat set, listing, reading
//!   and summarising every file ([`ServiceConfig::parallelism`] at once), and publishing
//!   generation 1 (pinned by
//!   `tests/view_service.rs::should_hear_of_a_file_written_during_the_open_scan_when_the_watch_came_first`).
//!   A lost watch is re-established before the rescan that may certify the view again.
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
//!   queued cookies, whose events may have been dropped.
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
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jfl_domain::{
    Completeness, FileSummary, FileUri, Generation, IncompleteReason, KeyHasher, ModuleName,
    RelPath, SyntaxInputs, SyntaxKey,
};

use crate::clock::{WallClock, elapsed};
use crate::repo_probe::{
    GitFile, GitSnapshot, GitStat, HeadState, RepoProbe, RepoProbeError, Worktree,
};
use crate::source_tree::{
    Exclusion, FileStat, FsKind, Hint, Hints, ListMode, MTime, Rescan, RescanReason, RescanReasons,
    SourceTree, SourceTreeError, cookie_name, cookie_stamp, is_cookie, is_java_file_name,
};
use crate::summary_store::SummaryStore;
use crate::syntax_indexer::{MAX_SOURCE_BYTES, SyntaxIndexer, strip_bom};
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
}

impl PartialEq for Summary {
    /// By key: a key names one content (plan §5.4).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent) | (Self::Gap, Self::Gap) => true,
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
    /// How many files a view's open or a reconcile reads and summarises at once, each on the
    /// ports' blocking pool.
    pub parallelism: usize,
}

impl ServiceConfig {
    /// Linux's coarse tick (10 ms), a 200 ms cookie deadline (twice plan §1.7's 100 ms
    /// freshness target), cookies swept after a minute, four files summarised at once.
    #[must_use]
    pub fn new(owner: &str) -> Self {
        Self {
            granularity: Duration::from_millis(10),
            cookie_deadline: Duration::from_millis(200),
            owner: owner.to_owned(),
            cookie_sweep_age: Duration::from_secs(60),
            parallelism: 4,
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

/// One summarised file: its path, what the stat saw before the read, and the summary.
type Summarised = (RelPath, FileSeen, Summary);

/// How many names a cookie tries before it is lost ([`cookie_name`]'s attempts).
const COOKIE_ATTEMPTS: u32 = 3;

/// The registry this service drives.
pub type Registry = ViewRegistry<FileSeen, GitObserved, Summary>;

/// The most reconciles one request waits through before giving up ([`ServiceError::NoProgress`]):
/// a request needs at most two publishes (plan §4.3), plus one for its first cookie's events.
const MAX_ROUNDS: u32 = 8;

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
    let gap = |s: &Option<Summary>| matches!(s, Some(Summary::Gap) | None);
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
    /// Reads and summarises `path`, through the store: what the stat saw before the read, and
    /// the summary.
    async fn summarise(&self, root: &FileUri, path: &RelPath) -> (FileSeen, Summary) {
        let at = self.clock.now();
        let stat = match stat_of(self.tree, root, path).await {
            Ok(stat) => stat,
            Err(()) => return (UNKNOWN, Summary::Gap),
        };
        let seen = FileSeen::new(stat, at, self.granularity);
        if stat.is_none() {
            return (seen, Summary::Absent);
        }
        let read = match self
            .tree
            .read(root, path, MAX_SOURCE_BYTES.saturating_add(4))
            .await
        {
            Ok(read) => read,
            Err(SourceTreeError::NotFound | SourceTreeError::NotAFile) => {
                return (UNKNOWN, Summary::Absent);
            }
            Err(_) => return (seen, Summary::Gap),
        };
        if read.truncated {
            return (seen, Summary::Gap);
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
        if let Ok(Some(summary)) = self.store.get(key).await {
            return (
                seen,
                Summary::Indexed {
                    key,
                    summary: Arc::new(summary),
                },
            );
        }
        match self.indexer.index(read.bytes).await {
            Ok(indexed) => {
                let summary = Arc::new(indexed.summary);
                // A failed write leaves the store without it: the next open re-indexes the file.
                let _ = self.store.put(key, Arc::clone(&summary)).await;
                (seen, Summary::Indexed { key, summary })
            }
            Err(_) => (seen, Summary::Gap),
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

    fn reader(&self) -> Reader<'_, T, X, S, C, H> {
        Reader {
            tree: &self.tree,
            indexer: &self.indexer,
            store: &self.store,
            clock: &self.clock,
            hasher: &self.hasher,
            granularity: self.config.granularity,
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

    /// Summarises `paths`, at most [`ServiceConfig::parallelism`] at a time: each read, hash and
    /// parse runs on the ports' blocking pool, so a cold open uses every core it is given.
    async fn summarise_all(&self, root: &FileUri, paths: Vec<RelPath>) -> Vec<Summarised> {
        type Job<'a> = core::pin::Pin<Box<dyn Future<Output = Summarised> + Send + 'a>>;
        let width = self.config.parallelism.max(1);
        let reader = &self.reader();
        let mut queue = paths.into_iter();
        let mut running: Vec<Job<'_>> = Vec::with_capacity(width);
        let mut done = Vec::new();
        loop {
            while running.len() < width {
                let Some(path) = queue.next() else {
                    break;
                };
                running.push(Box::pin(async move {
                    let (seen, summary) = reader.summarise(root, &path).await;
                    (path, seen, summary)
                }));
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
    async fn open(&mut self, worktree: Worktree) -> Result<(), ServiceError> {
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
        let scan = self
            .scan(&worktree)
            .await
            .ok_or(ServiceError::Tree(SourceTreeError::RootNotFound))?;
        let files: Vec<Summarised> = self
            .summarise_all(&root, scan.paths.into_iter().collect())
            .await
            .into_iter()
            .filter(|(_, _, summary)| !matches!(summary, Summary::Absent))
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
        let mut wanted = dirty;
        if rescan {
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
        }
        for path in wanted {
            if stats.contains_key(&path) {
                continue;
            }
            let seen = match self.stat(root, &path).await {
                Ok(stat) => self.seen(stat, at),
                Err(()) => UNKNOWN,
            };
            stats.insert(path, seen);
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
        for (path, seen, summary) in self.summarise_all(root, todo.into_iter().collect()).await {
            let stamp = self.tick();
            self.registry
                .summarised(root, &path, seen, summary, stamp)?;
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
        self.open(worktree.clone()).await?;
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
        self.open(worktree).await?;
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
