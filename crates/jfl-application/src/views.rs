//! The view registry (plan §4, §7.1; ADR 0017, ADR 0037): a view per worktree root, its
//! generations built beside the published one and published atomically, requests that stat, wait
//! for their cookie, pin a generation and read only from it, and `sync` as a barrier.
//!
//! This is the registry's core, and it does no I/O: whoever drives it (the daemon's view service,
//! the `views` conformance driver, the tests) stats, reads, summarises, stores and writes cookies,
//! and hands the registry what it saw. Each method is one transition of `formal/views.qnt`
//! (`doc/formal-models.md`, "Stimuli"), with the model's guards: a transition the model does not
//! enable is refused with a [`ViewError`], never applied. So the `views` driver
//! (`jfl-conformance`) replays the model's traces against this code step for step, and the async
//! shell over the ports cannot reorder what the model fixes. A core with ports inside it was
//! weighed and not taken: the model's steps are finer than one port call (a reconcile is a start,
//! one summary at a time and a publish), and a trace must be able to stop between any two.
//!
//! What it holds the caller to (ADR 0037, `doc/formal-models.md`, "IDX-09's obligations"):
//!
//! - **Watch, then scan, then publish.** [`ViewRegistry::open`] and every rescan certify a view
//!   only when its watch was up when the scan started ([`ViewRegistry::watch_restored`] asks for
//!   one); every loss ([`ViewRegistry::overflow`] for every view of the instance,
//!   [`ViewRegistry::watch_lost`]) leaves the view uncertain until such a scan publishes.
//! - **The racy-mtime rule** is the caller's [`Observed::proves_unchanged`]: [`FileSeen`] applies
//!   it to a file; an entry read within its file's write tick proves nothing.
//! - **The cookie.** [`ViewRegistry::request_start`] says whether to write one (the view is
//!   watched); the caller writes it into the view through the same in-order event stream and
//!   delivers it back with [`Event::Cookie`] (with its stamp), or calls [`ViewRegistry::cookie_lost`] when its
//!   deadline passes first. A summary read after the cookie ([`Stamp`]s on one clock with cookie
//!   writes) asks for a second one ([`ViewRegistry::request_recookie`]), two at most.
//! - **The pin re-stats `HEAD` and the index** ([`ViewRegistry::request_pin`]); for a linked
//!   worktree they live under the common directory's `worktrees/<name>/`.
//!
//! Pinned by: `jfl-conformance`'s `views` driver (`crates/jfl-conformance/src/views.rs`), which
//! replays every regression and generated trace of the model, and `tests/views.rs`.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use jfl_domain::{
    Completeness, FileUri, Generation, IncompleteReason, IndexState, IndexStateError, ModuleIndex,
    ModuleName, RelPath, completeness,
};

use crate::repo_probe::older_by;
use crate::source_tree::{FileStat, MTime};

/// What a stat saw of a file or a Git file, as a view remembers it.
pub trait Observed: Clone + Eq + fmt::Debug {
    /// Whether `self`, seen earlier, proves that `now` shows no change: the stats are equal and
    /// `self` was not taken within its file's write tick (the racy-mtime rule, plan §4.3).
    fn proves_unchanged(&self, now: &Self) -> bool;
}

/// What a stat saw of a source file.
pub trait FileObserved: Observed {
    /// Whether the file exists.
    fn present(&self) -> bool;
}

/// A source file as a stat saw it: its [`FileStat`] (`None`: absent) and whether the reading was
/// racy, taken within the timestamp granularity of the file's last write, so that an equal stat
/// later proves nothing.
///
/// Pinned by: `views::tests::should_never_prove_a_racy_entry_unchanged_when_the_stat_is_equal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileSeen {
    /// The stat; `None` when the file did not exist.
    pub stat: Option<FileStat>,
    /// Whether the reading falls within the granularity of the file's last write.
    pub racy: bool,
}

impl FileSeen {
    /// `stat` taken at or after `observed_at`, the wall-clock time read before the stat (or the
    /// read it stands for), on a filesystem whose timestamps step by at most `granularity`.
    #[must_use]
    pub fn new(stat: Option<FileStat>, observed_at: MTime, granularity: Duration) -> Self {
        let racy = stat.is_some_and(|s| !older_by(s.mtime, observed_at, granularity));
        Self { stat, racy }
    }
}

impl Observed for FileSeen {
    fn proves_unchanged(&self, now: &Self) -> bool {
        !self.racy && self.stat == now.stat
    }
}

impl FileObserved for FileSeen {
    fn present(&self) -> bool {
        self.stat.is_some()
    }
}

/// A view's `HEAD` and Git index, as a stat saw them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitSeen<G> {
    /// `HEAD` and the branch it names.
    pub head: G,
    /// The index.
    pub index: G,
}

impl<G: Observed> GitSeen<G> {
    fn proves_unchanged(&self, now: &Self) -> bool {
        self.head.proves_unchanged(&now.head) && self.index.proves_unchanged(&now.index)
    }
}

/// Whether the store holds a summary: what a publish checks of every entry first (atomic publish).
pub trait Stored<K> {
    /// Whether the summary stored under `key` can be read back.
    fn holds(&self, key: &K) -> bool;
}

/// A place on the clock that orders summary reads with cookie writes: a summary whose stamp is
/// after a cookie's was read after that cookie was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp(pub u64);

/// A request, as its caller numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(pub u64);

/// One file of a generation (or of a build).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<D, K> {
    /// What the stat saw when the file was read (or, while the build has yet to read it, when the
    /// build started).
    pub seen: D,
    /// The summary's key; `None` only while the build has yet to read the file.
    pub summary: Option<K>,
    /// When the summary was read; `None` for the scan a view was opened with.
    pub read_at: Option<Stamp>,
}

/// A published generation: immutable, shared by every request that pinned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published<D, G, K> {
    /// Its number, counted per view.
    pub generation: Generation,
    /// `HEAD` and the index it was built at.
    pub git: GitSeen<G>,
    /// Its files.
    pub index: BTreeMap<RelPath, Entry<D, K>>,
    /// The latest read stamp of any entry: a request whose cookie is older finds a late read.
    latest_read: Option<Stamp>,
    /// How many of its files exist, counted once at publish so a pin does not walk the index.
    present: usize,
}

impl<D: FileObserved, G, K> Published<D, G, K> {
    fn new(generation: Generation, git: GitSeen<G>, index: BTreeMap<RelPath, Entry<D, K>>) -> Self {
        let latest_read = index.values().filter_map(|e| e.read_at).max();
        let present = index.values().filter(|e| e.seen.present()).count();
        Self {
            generation,
            git,
            index,
            latest_read,
            present,
        }
    }
}

/// The build beside the published generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build<D, G, K> {
    /// `HEAD` and the index it records.
    pub git: GitSeen<G>,
    /// Its files: the published ones, with every path it re-reads replaced.
    pub index: BTreeMap<RelPath, Entry<D, K>>,
    /// The paths whose summaries it has yet to read and store.
    pub pending: BTreeSet<RelPath>,
    /// Whether its publish makes the view certain: a full scan started with the watch up and no
    /// loss since.
    pub certifies: bool,
    /// The `sync` (its epoch) whose rescan this is.
    pub serves_sync: Option<u64>,
}

/// Where a view's `sync` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncPhase {
    /// None started.
    #[default]
    Idle,
    /// Started, not returned.
    Waiting,
    /// Returned.
    Done,
}

/// A view's `sync` (plan §4.3): it returns only once a rescan started after it has published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncState {
    /// Where it is.
    pub phase: SyncPhase,
    /// How many were started.
    pub epoch: u64,
    /// Whether a rescan that started after the latest one has published.
    pub scanned: bool,
}

/// One worktree's view.
#[derive(Debug, Clone)]
pub struct ViewState<D, G, K> {
    /// Its worktree root: the key the registry holds it under.
    pub root: FileUri,
    /// The delivered file events no build has taken.
    pub dirty: BTreeSet<RelPath>,
    /// Whether a full reconcile is due.
    pub rescan: bool,
    /// Whether the view's watch is established.
    pub watched: bool,
    /// Whether the view has seen no loss since a full scan that started with its watch up.
    pub certain: bool,
    /// The build in progress.
    pub build: Option<Build<D, G, K>>,
    /// The published generation.
    pub published: Arc<Published<D, G, K>>,
    /// The generation the last publish replaced (0 before the first).
    pub replaced: Generation,
    /// Its `sync`.
    pub sync: SyncState,
}

/// Where a request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPhase {
    /// Started: waiting for its generation and its cookie.
    Waiting,
    /// Pinned to a generation.
    Pinned,
    /// Answered.
    Done,
}

/// A request's latest cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cookie {
    /// None was written: the view is not watched.
    None,
    /// Written, its event not back yet.
    Queued,
    /// Its event came back through the view's event stream.
    Delivered,
    /// Lost: dropped in an overflow, or its deadline passed first.
    Lost,
}

/// What a request pinned.
#[derive(Debug, Clone)]
pub struct Pin<D, G, K> {
    /// The generation, which a publish never changes.
    pub generation: Arc<Published<D, G, K>>,
    /// Its completeness, computed from the request's own view at that generation.
    pub completeness: Completeness,
    /// The root of the view whose state the completeness was computed from (the view's own, so
    /// never another view's at the same generation number: plan §4.5).
    pub complete_view: FileUri,
}

/// One request.
#[derive(Debug, Clone)]
pub struct Request<D, G, K> {
    /// Where it is.
    pub phase: RequestPhase,
    /// The view it was bound to: the innermost open worktree of its file.
    pub view: FileUri,
    /// Its file, under that view's root.
    pub path: RelPath,
    /// The first generation it may pin: one built after its stat.
    pub need: Generation,
    /// Its latest cookie.
    pub cookie: Cookie,
    /// When its latest cookie was written.
    pub cookie_at: Stamp,
    /// How many cookies it wrote (two at most).
    pub cookies: u8,
    /// What it pinned.
    pub pin: Option<Pin<D, G, K>>,
}

/// One delivered watcher event, in the order the view's event stream delivered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A source file of `view` changed (a hint: the reconcile decides what).
    File {
        /// The view whose watch saw it.
        view: FileUri,
        /// The file.
        path: RelPath,
    },
    /// `view`'s `HEAD` or index changed (only a main worktree's watch can see them).
    Git {
        /// The view.
        view: FileUri,
    },
    /// A request's cookie came back: every event raised before it was delivered. `at` is the
    /// cookie's place on the clock (the stamp it was written at, which names the cookie file): an
    /// event of an older cookie of the same request (a first cookie's late `IN_CLOSE_WRITE`, a
    /// cookie whose deadline passed) is not the barrier of the latest one and is ignored.
    Cookie {
        /// The request.
        id: RequestId,
        /// When that cookie was written.
        at: Stamp,
    },
}

/// What [`ViewRegistry::request_start`] tells its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The view the request was bound to.
    pub view: FileUri,
    /// The file under its root.
    pub path: RelPath,
    /// Whether to write a cookie into the view now.
    pub write_cookie: bool,
}

/// A transition the registry refuses: the model does not enable it in this state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    /// No open view has this root.
    #[error("no open view has this root")]
    UnknownView,
    /// A view with this root is open already.
    #[error("the view is open already")]
    AlreadyOpen,
    /// The path is under no open view.
    #[error("the path is under no open view")]
    Unbound,
    /// The request is not known.
    #[error("unknown request")]
    UnknownRequest,
    /// The request is in a phase the transition does not start from.
    #[error("the request is not in a phase this transition starts from")]
    WrongPhase,
    /// The request's cookie is still queued.
    #[error("the request's cookie is still queued")]
    CookieQueued,
    /// No published generation the request may pin yet.
    #[error("no generation the request may pin yet")]
    NotPinnable,
    /// No second cookie is due.
    #[error("no second cookie is due")]
    NoRecookie,
    /// The pinned generation does not hold the path.
    #[error("the pinned generation does not hold the path")]
    NotInGeneration,
    /// A build is in progress already.
    #[error("a build is in progress")]
    BuildActive,
    /// Nothing to reconcile: no delivered event and no rescan due.
    #[error("nothing to reconcile")]
    NothingToReconcile,
    /// No build is in progress.
    #[error("no build is in progress")]
    NoBuild,
    /// The build does not wait for this path's summary.
    #[error("the build does not wait for this path")]
    NotPending,
    /// The build still waits for summaries.
    #[error("the build still waits for summaries")]
    PendingSummaries,
    /// A summary the build names is not in the store.
    #[error("a summary the build names is not stored")]
    NotStored,
    /// The watch is up already, or down already.
    #[error("the watch is already in that state")]
    WatchState,
    /// A `sync` is waiting already, or none has scanned yet.
    #[error("the sync is not in a state this transition starts from")]
    SyncState,
    /// A count would overflow.
    #[error("a count would overflow")]
    Overflow,
}

/// The view registry. `D` is what a stat saw of a source file ([`FileSeen`] in the daemon), `G`
/// what it saw of `HEAD` or the index, `K` a summary's key in the store.
#[derive(Debug, Clone)]
pub struct ViewRegistry<D, G, K> {
    views: BTreeMap<FileUri, ViewState<D, G, K>>,
    requests: BTreeMap<RequestId, Request<D, G, K>>,
}

impl<D, G, K> Default for ViewRegistry<D, G, K> {
    fn default() -> Self {
        Self {
            views: BTreeMap::new(),
            requests: BTreeMap::new(),
        }
    }
}

fn next(generation: Generation, by: u64) -> Result<Generation, ViewError> {
    generation
        .0
        .checked_add(by)
        .map(Generation)
        .ok_or(ViewError::Overflow)
}

impl<D: FileObserved, G: Observed, K: Clone + Eq + fmt::Debug> ViewRegistry<D, G, K> {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the view of the worktree at `root` and publishes its generation 1 from `files`, the
    /// scan's every file with what its stat saw and its stored summary. Watch, then scan: the
    /// caller established the watch (`watched`) before it listed; a view it could not watch (a
    /// network, FUSE or overlay filesystem, ADR 0037) is opened unwatched and stays uncertain.
    pub fn open(
        &mut self,
        root: FileUri,
        watched: bool,
        git: GitSeen<G>,
        files: impl IntoIterator<Item = (RelPath, D, K)>,
    ) -> Result<(), ViewError> {
        if self.views.contains_key(&root) {
            return Err(ViewError::AlreadyOpen);
        }
        let index = files
            .into_iter()
            .map(|(path, seen, key)| {
                let entry = Entry {
                    seen,
                    summary: Some(key),
                    read_at: None,
                };
                (path, entry)
            })
            .collect();
        let view = ViewState {
            root: root.clone(),
            dirty: BTreeSet::new(),
            rescan: false,
            watched,
            certain: watched,
            build: None,
            published: Arc::new(Published::new(Generation(1), git, index)),
            replaced: Generation(0),
            sync: SyncState::default(),
        };
        self.views.insert(root, view);
        Ok(())
    }

    /// The view of `root`.
    #[must_use]
    pub fn view(&self, root: &FileUri) -> Option<&ViewState<D, G, K>> {
        self.views.get(root)
    }

    /// The request `id`, once started.
    #[must_use]
    pub fn request(&self, id: RequestId) -> Option<&Request<D, G, K>> {
        self.requests.get(&id)
    }

    /// The innermost open view enclosing `file` and the file under its root (plan §4.1): a file
    /// of a nested worktree binds to that worktree's view, never to its parent's, as long as the
    /// caller opened the view `RepoProbe::resolve` named first.
    ///
    /// Pinned by: `views::tests::should_bind_to_the_innermost_view_when_views_nest`.
    #[must_use]
    pub fn bind(&self, file: &FileUri) -> Option<(&FileUri, RelPath)> {
        self.views
            .keys()
            .filter_map(|root| file.relative_to(root).map(|path| (root, path)))
            .max_by_key(|(root, _)| root.path().len())
    }

    fn view_mut(&mut self, root: &FileUri) -> Result<&mut ViewState<D, G, K>, ViewError> {
        self.views.get_mut(root).ok_or(ViewError::UnknownView)
    }

    fn request_mut(&mut self, id: RequestId) -> Result<&mut Request<D, G, K>, ViewError> {
        self.requests.get_mut(&id).ok_or(ViewError::UnknownRequest)
    }

    /// The next event of the in-order stream: a file event marks its path dirty, a Git event
    /// asks for a rescan, a cookie is its request's barrier, but only the request's latest
    /// cookie while it is queued: any other cookie event is ignored.
    ///
    /// Pinned by: `tests/views.rs::should_wait_for_the_second_cookie_when_a_stale_first_cookie_event_arrives`.
    pub fn deliver(&mut self, event: Event) -> Result<(), ViewError> {
        match event {
            Event::File { view, path } => {
                self.view_mut(&view)?.dirty.insert(path);
            }
            Event::Git { view } => self.view_mut(&view)?.rescan = true,
            Event::Cookie { id, at } => {
                let request = self.request_mut(id)?;
                if request.cookie == Cookie::Queued && request.cookie_at == at {
                    request.cookie = Cookie::Delivered;
                }
            }
        }
        Ok(())
    }

    /// The event queue overflowed (inotify's `IN_Q_OVERFLOW`, FSEvents' dropped or must-scan
    /// flags): one instance serves every view, so every view rescans and is uncertain.
    pub fn overflow(&mut self) {
        for view in self.views.values_mut() {
            view.rescan = true;
            view.certain = false;
            if let Some(build) = view.build.as_mut() {
                build.certifies = false;
            }
        }
    }

    /// The request's cookie will not come back: it was dropped, or its deadline passed. The
    /// request may pin, never as complete.
    pub fn cookie_lost(&mut self, id: RequestId) -> Result<(), ViewError> {
        self.request_mut(id)?.cookie = Cookie::Lost;
        Ok(())
    }

    /// The view's watch is lost (an `add_watch` error, `IN_IGNORED`, `IN_DELETE_SELF`,
    /// `IN_UNMOUNT`, FSEvents' `RootChanged` or `EventIdsWrapped`): later changes raise no event.
    pub fn watch_lost(&mut self, root: &FileUri) -> Result<(), ViewError> {
        let view = self.view_mut(root)?;
        if !view.watched {
            return Err(ViewError::WatchState);
        }
        view.watched = false;
        view.certain = false;
        view.rescan = true;
        if let Some(build) = view.build.as_mut() {
            build.certifies = false;
        }
        Ok(())
    }

    /// The view's watch is established again: watch, then scan.
    pub fn watch_restored(&mut self, root: &FileUri) -> Result<(), ViewError> {
        let view = self.view_mut(root)?;
        if view.watched {
            return Err(ViewError::WatchState);
        }
        view.watched = true;
        view.rescan = true;
        Ok(())
    }

    /// Starts the view's next build from the delivered events and, on a rescan, every path a
    /// stat (`stat_file`) cannot prove unchanged, recording `HEAD` and the index (`stat_git`);
    /// returns the paths the caller reads, summarises and stores next. On a rescan, `listed` is
    /// the view's listing taken after the watch was up: a listed path the index lacks (created
    /// while no event could be raised: a lost watch, an overflow) is read too, and an indexed path
    /// the listing lacks is re-statted like any other (it reads as absent). A rescan certifies
    /// the view only when its watch is up and it was given a listing: re-statting the indexed
    /// paths alone cannot see a new file. A rescan serves a waiting `sync`. `listed` is ignored
    /// when no rescan is due.
    ///
    /// Pinned by: `tests/views.rs::should_read_a_file_created_while_the_watch_was_lost_when_the_rescan_lists_it`
    /// and `tests/views.rs::should_stay_uncertain_when_a_rescan_has_no_listing`.
    pub fn reconcile_start(
        &mut self,
        root: &FileUri,
        listed: Option<&BTreeSet<RelPath>>,
        mut stat_file: impl FnMut(&RelPath) -> D,
        stat_git: impl FnOnce() -> GitSeen<G>,
    ) -> Result<BTreeSet<RelPath>, ViewError> {
        let view = self.view_mut(root)?;
        if view.build.is_some() {
            return Err(ViewError::BuildActive);
        }
        if view.dirty.is_empty() && !view.rescan {
            return Err(ViewError::NothingToReconcile);
        }
        let published = Arc::clone(&view.published);
        let mut index = published.index.clone();
        let mut todo = core::mem::take(&mut view.dirty);
        let rescan = view.rescan;
        let mut seen_now = BTreeMap::new();
        if rescan {
            for (path, entry) in &published.index {
                let now = stat_file(path);
                if !entry.seen.proves_unchanged(&now) {
                    todo.insert(path.clone());
                }
                seen_now.insert(path.clone(), now);
            }
            for path in listed.into_iter().flatten() {
                if !published.index.contains_key(path) {
                    todo.insert(path.clone());
                }
            }
        }
        for path in &todo {
            let seen = match seen_now.remove(path) {
                Some(seen) => seen,
                None => stat_file(path),
            };
            let read_at = index.get(path).and_then(|e| e.read_at);
            index.insert(
                path.clone(),
                Entry {
                    seen,
                    summary: None,
                    read_at,
                },
            );
        }
        let git = if rescan {
            stat_git()
        } else {
            published.git.clone()
        };
        let serves_sync =
            (rescan && view.sync.phase == SyncPhase::Waiting).then_some(view.sync.epoch);
        view.rescan = false;
        view.build = Some(Build {
            git,
            index,
            pending: todo.clone(),
            certifies: rescan && view.watched && listed.is_some(),
            serves_sync,
        });
        Ok(todo)
    }

    /// The build's summary of `path` is stored under `key`: it was read `read_at`, when the stat
    /// saw `seen` (racy when read within the file's write tick).
    pub fn summarised(
        &mut self,
        root: &FileUri,
        path: &RelPath,
        seen: D,
        key: K,
        read_at: Stamp,
    ) -> Result<(), ViewError> {
        let build = self
            .view_mut(root)?
            .build
            .as_mut()
            .ok_or(ViewError::NoBuild)?;
        if !build.pending.remove(path) {
            return Err(ViewError::NotPending);
        }
        build.index.insert(
            path.clone(),
            Entry {
                seen,
                summary: Some(key),
                read_at: Some(read_at),
            },
        );
        Ok(())
    }

    /// Leaves `path` out of the view's build: the next generation does not hold it. Not a
    /// transition of the model, whose views index a fixed set of paths: the daemon's shell calls
    /// it for a path the view does not admit (under a nested worktree or repository, or outside
    /// the listing a rescan unioned with the tracked files), so a parent never indexes a nested
    /// view's file and a path that leaked in leaves at the next rescan. The `views` driver never
    /// calls it.
    ///
    /// Pinned by: `tests/view_service.rs::should_keep_a_nested_worktree_file_out_of_the_parent_when_it_is_edited`.
    pub fn exclude(&mut self, root: &FileUri, path: &RelPath) -> Result<(), ViewError> {
        let build = self
            .view_mut(root)?
            .build
            .as_mut()
            .ok_or(ViewError::NoBuild)?;
        build.index.remove(path);
        build.pending.remove(path);
        Ok(())
    }

    /// Publishes the build as the view's next generation, atomically: only when it waits for no
    /// summary and `store` holds every summary it names (atomic publish). Requests that pinned the
    /// previous generation keep it.
    pub fn publish(
        &mut self,
        root: &FileUri,
        store: &impl Stored<K>,
    ) -> Result<Generation, ViewError> {
        let view = self.view_mut(root)?;
        let build = view.build.as_ref().ok_or(ViewError::NoBuild)?;
        if !build.pending.is_empty() {
            return Err(ViewError::PendingSummaries);
        }
        let stored = build
            .index
            .values()
            .all(|e| e.summary.as_ref().is_some_and(|key| store.holds(key)));
        if !stored {
            return Err(ViewError::NotStored);
        }
        let generation = next(view.published.generation, 1)?;
        let Some(build) = view.build.take() else {
            return Err(ViewError::NoBuild);
        };
        view.replaced = view.published.generation;
        view.published = Arc::new(Published::new(generation, build.git, build.index));
        view.certain = view.certain || build.certifies;
        if view.sync.phase == SyncPhase::Waiting && build.serves_sync == Some(view.sync.epoch) {
            view.sync.scanned = true;
        }
        Ok(generation)
    }

    /// Starts request `id` on `file`: binds it to the innermost open view, compares what the
    /// request's stat saw (`seen`, `git`) with the published generation, and asks for a cookie
    /// when the view is watched. A change the stat sees (the racy rule applied) marks the path
    /// dirty, or asks for a rescan for `HEAD` or the index, and the request then waits for a
    /// generation whose build started after it. `now` is the clock at the stat: the cookie's
    /// place on it.
    pub fn request_start(
        &mut self,
        id: RequestId,
        file: &FileUri,
        seen: &D,
        git: &GitSeen<G>,
        now: Stamp,
    ) -> Result<Started, ViewError> {
        if let Some(r) = self.requests.get(&id) {
            if r.phase == RequestPhase::Waiting || r.phase == RequestPhase::Pinned {
                return Err(ViewError::WrongPhase);
            }
            if r.cookie == Cookie::Queued {
                return Err(ViewError::CookieQueued);
            }
        }
        let (root, path) = self.bind(file).ok_or(ViewError::Unbound)?;
        let root = root.clone();
        let view = self.view_mut(&root)?;
        let published = &view.published;
        let file_fresh = published
            .index
            .get(&path)
            .is_some_and(|e| e.seen.proves_unchanged(seen));
        let git_fresh = published.git.proves_unchanged(git);
        let need = if file_fresh && git_fresh {
            published.generation
        } else {
            next(
                published.generation,
                if view.build.is_some() { 2 } else { 1 },
            )?
        };
        if !file_fresh {
            view.dirty.insert(path.clone());
        }
        if !git_fresh {
            view.rescan = true;
        }
        let write_cookie = view.watched;
        self.requests.insert(
            id,
            Request {
                phase: RequestPhase::Waiting,
                view: root.clone(),
                path: path.clone(),
                need,
                cookie: if write_cookie {
                    Cookie::Queued
                } else {
                    Cookie::None
                },
                cookie_at: now,
                cookies: u8::from(write_cookie),
                pin: None,
            },
        );
        Ok(Started {
            view: root,
            path,
            write_cookie,
        })
    }

    /// Asks for the request's second cookie, written `now`: due when its first came back and the
    /// published generation holds a summary read after it. Returns the view to write it into.
    pub fn request_recookie(&mut self, id: RequestId, now: Stamp) -> Result<FileUri, ViewError> {
        let request = self.requests.get(&id).ok_or(ViewError::UnknownRequest)?;
        let view = self
            .views
            .get(&request.view)
            .ok_or(ViewError::UnknownView)?;
        let due = request.phase == RequestPhase::Waiting
            && request.cookie == Cookie::Delivered
            && request.cookies < 2
            && view.watched
            && late_reads(&view.published, request.cookie_at);
        if !due {
            return Err(ViewError::NoRecookie);
        }
        let root = request.view.clone();
        let request = self.request_mut(id)?;
        request.cookie = Cookie::Queued;
        request.cookie_at = now;
        request.cookies = request.cookies.saturating_add(1);
        Ok(root)
    }

    /// Pins the request to its view's published generation once that generation is at least the
    /// one it needs and its cookie is not still queued, and computes its completeness from the
    /// request's own view at that generation, with `HEAD` and the index re-statted (`git`); a
    /// change there asks for a rescan. Complete only when the cookie came back, no published
    /// summary was read after it, the view is watched and certain, nothing is delivered and
    /// unbuilt, no rescan is due, no build runs, and the re-stat proves `HEAD` and the index
    /// unchanged (ADR 0037).
    pub fn request_pin(
        &mut self,
        id: RequestId,
        git: &GitSeen<G>,
    ) -> Result<&Pin<D, G, K>, ViewError> {
        let request = self.requests.get(&id).ok_or(ViewError::UnknownRequest)?;
        let view = self
            .views
            .get(&request.view)
            .ok_or(ViewError::UnknownView)?;
        if request.phase != RequestPhase::Waiting {
            return Err(ViewError::WrongPhase);
        }
        if view.published.generation < request.need {
            return Err(ViewError::NotPinnable);
        }
        if request.cookie == Cookie::Queued {
            return Err(ViewError::CookieQueued);
        }
        let generation = Arc::clone(&view.published);
        let restat = generation.git.proves_unchanged(git);
        let (completeness, complete_view) = completeness_at(request, view, restat);
        let root = request.view.clone();
        if !restat {
            self.view_mut(&root)?.rescan = true;
        }
        let request = self.request_mut(id)?;
        request.phase = RequestPhase::Pinned;
        Ok(request.pin.insert(Pin {
            generation,
            completeness,
            complete_view,
        }))
    }

    /// Reads `path` from the request's pinned generation, and only from it.
    pub fn read(&self, id: RequestId, path: &RelPath) -> Result<&Entry<D, K>, ViewError> {
        let request = self.requests.get(&id).ok_or(ViewError::UnknownRequest)?;
        let pin = match (&request.phase, &request.pin) {
            (RequestPhase::Pinned, Some(pin)) => pin,
            _ => return Err(ViewError::WrongPhase),
        };
        pin.generation
            .index
            .get(path)
            .ok_or(ViewError::NotInGeneration)
    }

    /// The request answered.
    pub fn finish(&mut self, id: RequestId) -> Result<(), ViewError> {
        let request = self.request_mut(id)?;
        if request.phase != RequestPhase::Pinned {
            return Err(ViewError::WrongPhase);
        }
        request.phase = RequestPhase::Done;
        Ok(())
    }

    /// Forgets a finished request, dropping its pin, so the generation it pinned is freed once no
    /// other request holds it. Not a transition of the model (whose requests are reused, never
    /// removed): the daemon's shell forgets each request it finishes, the `views` driver does not.
    /// A request whose cookie is still queued is kept, so a late cookie event still finds it.
    ///
    /// Pinned by: `views::tests::should_free_a_pinned_generation_when_its_request_is_forgotten`.
    pub fn forget(&mut self, id: RequestId) -> Result<(), ViewError> {
        let request = self.requests.get(&id).ok_or(ViewError::UnknownRequest)?;
        if request.phase != RequestPhase::Done {
            return Err(ViewError::WrongPhase);
        }
        self.requests.remove(&id);
        Ok(())
    }

    /// Starts a `sync` of the view: a rescan is due, and the `sync` returns only once a rescan
    /// that started after this call has published.
    pub fn sync_start(&mut self, root: &FileUri) -> Result<u64, ViewError> {
        let view = self.view_mut(root)?;
        if view.sync.phase == SyncPhase::Waiting {
            return Err(ViewError::SyncState);
        }
        let epoch = view.sync.epoch.checked_add(1).ok_or(ViewError::Overflow)?;
        view.sync = SyncState {
            phase: SyncPhase::Waiting,
            epoch,
            scanned: false,
        };
        view.rescan = true;
        Ok(epoch)
    }

    /// The `sync` returns: only once its rescan has published.
    pub fn sync_done(&mut self, root: &FileUri) -> Result<(), ViewError> {
        let view = self.view_mut(root)?;
        if view.sync.phase != SyncPhase::Waiting || !view.sync.scanned {
            return Err(ViewError::SyncState);
        }
        view.sync.phase = SyncPhase::Done;
        Ok(())
    }
}

/// Whether `published` holds a summary read after a cookie written `cookie_at`.
fn late_reads<D, G, K>(published: &Published<D, G, K>, cookie_at: Stamp) -> bool {
    published.latest_read > Some(cookie_at)
}

/// The completeness of a worktree's one module at `generation`: `files` present, every one
/// indexed, `stale` of them dirty. Only a module with no file is empty (and complete); counts
/// [`ModuleIndex::new`] refuses otherwise make the answer incomplete, never complete.
///
/// Pinned by: `views::tests::should_answer_incomplete_when_the_module_counts_are_refused`.
fn module_completeness(generation: Generation, files: u32, stale: u32) -> Completeness {
    let module = ModuleName::worktree();
    let state = match ModuleIndex::new(module.clone(), generation, files, files, stale, 0) {
        Ok(index) => index,
        Err(IndexStateError::NoFiles) => ModuleIndex::empty(module.clone(), generation, 0),
        Err(_) => {
            return Completeness::because(IncompleteReason::ModuleNotIndexed { module });
        }
    };
    match IndexState::new(generation, vec![state]) {
        Ok(state) => completeness(&[module], &state),
        Err(_) => Completeness::because(IncompleteReason::ModuleNotIndexed { module }),
    }
}

/// The request's completeness at its view's published generation (`formal/views.qnt`'s
/// `completeAt`), through IDX-02's [`completeness`] over the view's one module (before M3 a
/// worktree is one module, plan §2.1), keyed by the request's own view; with the root of the view
/// whose state it read, which [`Pin::complete_view`] reports.
fn completeness_at<D: FileObserved, G, K>(
    request: &Request<D, G, K>,
    view: &ViewState<D, G, K>,
    restat: bool,
) -> (Completeness, FileUri) {
    let published = &view.published;
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let files = count(published.present);
    let stale = count(
        view.dirty
            .iter()
            .filter(|p| published.index.get(*p).is_some_and(|e| e.seen.present()))
            .count(),
    );
    let mut result = module_completeness(published.generation, files, stale);
    let pending = [
        request.cookie != Cookie::Delivered,
        late_reads(published, request.cookie_at),
        view.rescan,
        view.build.is_some(),
        !restat,
    ]
    .into_iter()
    .filter(|b| *b)
    .count()
    .saturating_add(view.dirty.len());
    if pending > 0 {
        result = result.and(IncompleteReason::PendingChanges {
            events: count(pending),
        });
    }
    if !view.watched {
        result = result.and(IncompleteReason::NotWatched);
    }
    if !view.certain {
        result = result.and(IncompleteReason::WatcherLostEvents);
    }
    (result, view.root.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(size: u64, secs: i64) -> FileStat {
        FileStat {
            size,
            mtime: MTime { secs, nanos: 0 },
            inode: 7,
        }
    }

    fn uri(path: &str) -> Result<FileUri, String> {
        FileUri::from_path(path).map_err(|e| e.to_string())
    }

    #[test]
    fn should_never_prove_a_racy_entry_unchanged_when_the_stat_is_equal() {
        // given
        let granularity = Duration::from_millis(10);
        let written = stat(10, 100);
        let racy = FileSeen::new(
            Some(written),
            MTime {
                secs: 100,
                nanos: 5,
            },
            granularity,
        );
        let settled = FileSeen::new(
            Some(written),
            MTime {
                secs: 101,
                nanos: 0,
            },
            granularity,
        );

        // when
        let racy_proves = racy.proves_unchanged(&settled);
        let settled_proves = settled.proves_unchanged(&settled);

        // then
        assert!(racy.racy);
        assert!(!racy_proves);
        assert!(settled_proves);
        assert!(!settled.proves_unchanged(&FileSeen::new(
            None,
            MTime {
                secs: 102,
                nanos: 0
            },
            granularity
        )));
    }

    #[test]
    fn should_answer_incomplete_when_the_module_counts_are_refused() {
        // given
        let generation = Generation(3);

        // when
        let refused = module_completeness(generation, 1, 2);
        let empty = module_completeness(generation, 0, 0);
        let whole = module_completeness(generation, 2, 0);

        // then
        assert!(!refused.is_complete());
        assert!(empty.is_complete());
        assert!(whole.is_complete());
    }

    #[test]
    fn should_free_a_pinned_generation_when_its_request_is_forgotten() -> Result<(), String> {
        // given
        let mut registry: ViewRegistry<FileSeen, FileSeen, u8> = ViewRegistry::new();
        let absent = FileSeen {
            stat: None,
            racy: false,
        };
        let git = GitSeen {
            head: absent,
            index: absent,
        };
        let root = uri("/w/m")?;
        let path = RelPath::new("A.java").map_err(|e| e.to_string())?;
        let seen = FileSeen::new(
            Some(stat(1, 10)),
            MTime { secs: 99, nanos: 0 },
            Duration::from_millis(10),
        );
        registry
            .open(root.clone(), false, git.clone(), [(path, seen, 1)])
            .map_err(|e| e.to_string())?;
        let id = RequestId(1);
        registry
            .request_start(id, &uri("/w/m/A.java")?, &seen, &git, Stamp(1))
            .map_err(|e| e.to_string())?;
        let pinned = Arc::downgrade(
            &registry
                .request_pin(id, &git)
                .map_err(|e| e.to_string())?
                .generation,
        );
        let early = registry.forget(id);
        registry.finish(id).map_err(|e| e.to_string())?;
        registry
            .deliver(Event::File {
                view: root.clone(),
                path: RelPath::new("A.java").map_err(|e| e.to_string())?,
            })
            .map_err(|e| e.to_string())?;
        registry
            .reconcile_start(&root, None, |_| seen, || git.clone())
            .map_err(|e| e.to_string())?;
        registry
            .summarised(
                &root,
                &RelPath::new("A.java").map_err(|e| e.to_string())?,
                seen,
                2,
                Stamp(2),
            )
            .map_err(|e| e.to_string())?;
        struct All;
        impl Stored<u8> for All {
            fn holds(&self, _: &u8) -> bool {
                true
            }
        }
        registry.publish(&root, &All).map_err(|e| e.to_string())?;
        let kept = pinned.upgrade().is_some();

        // when
        registry.forget(id).map_err(|e| e.to_string())?;

        // then
        assert_eq!(early, Err(ViewError::WrongPhase));
        assert!(kept);
        assert!(pinned.upgrade().is_none());
        assert!(registry.request(id).is_none());
        Ok(())
    }

    #[test]
    fn should_bind_to_the_innermost_view_when_views_nest() -> Result<(), String> {
        // given
        let mut registry: ViewRegistry<FileSeen, FileSeen, u8> = ViewRegistry::new();
        let git = GitSeen {
            head: FileSeen {
                stat: None,
                racy: false,
            },
            index: FileSeen {
                stat: None,
                racy: false,
            },
        };
        let outer = uri("/w/m")?;
        let inner = uri("/w/m/.claude/worktrees/n")?;
        registry
            .open(outer.clone(), true, git.clone(), [])
            .map_err(|e| e.to_string())?;
        registry
            .open(inner.clone(), true, git, [])
            .map_err(|e| e.to_string())?;

        // when
        let nested = registry.bind(&uri("/w/m/.claude/worktrees/n/A.java")?);
        let own = registry.bind(&uri("/w/m/B.java")?);
        let outside = registry.bind(&uri("/w/x/C.java")?);

        // then
        assert_eq!(
            nested.map(|(root, p)| (root.clone(), p.as_str().to_owned())),
            Some((inner, "A.java".to_owned()))
        );
        assert_eq!(
            own.map(|(root, p)| (root.clone(), p.as_str().to_owned())),
            Some((outer, "B.java".to_owned()))
        );
        assert!(outside.is_none());
        Ok(())
    }
}
