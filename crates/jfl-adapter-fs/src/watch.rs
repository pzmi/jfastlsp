//! Watcher hints (plan §4.3) from `notify`'s recommended backend, in the order watch, then hint.
//!
//! **Linux (inotify).** The adapter manages its watches itself: one non-recursive watch per
//! directory the exclusion rules accept (`walk::watched_dirs`: `.git`, nested worktrees and
//! repositories, `.claude/worktrees/` and build output get none; `.gitignore`d directories do,
//! since a `.gitignore` edit can bring them back). When the backend reports a directory created or
//! moved in, the handler records it as pending before it queues its hint, and the consumer's
//! `try_next`/`next` watches every pending directory (and the accepted directories below it)
//! before it hands out any hint. So once a new directory's hint is taken, the directory is
//! watched: a scan on the hint misses nothing written afterwards. A directory that cannot be
//! watched, or read for its subdirectories, is a `Rescan(WatcherError)`, never skipped. A backend
//! overflow re-watches the whole tree before its rescan is handed out, since directory creations
//! were lost with it.
//!
//! **macOS (FSEvents).** FSEvents watches a whole tree with one stream and restarts the stream for
//! every path added, so the root is watched recursively and every new directory is covered from
//! the start; excluded directories are watched too and only `hint_excluded` filters their events.
//! FSEvents says nothing about a directory it cannot read, so the start walks the accepted
//! directories as Linux's does, and one that cannot be read is a `Rescan(WatcherError)` there too.
//!
//! Everything flows through a bounded, coalescing `jfl_application::hints` queue (no debounce
//! timer, no unbounded channel). Access events are dropped (inotify reports every open, and the
//! indexer's own reads would flood the queue), except a close after writing. `notify` does not
//! follow links (it does by default), and its event-loop thread is its own (fnd §2.4: the crate
//! holds no `spawn` grant; the thread is notify's, stopped when the watch drops).
//!
//! **What is reported:** the backend's overflow (inotify `IN_Q_OVERFLOW`, FSEvents' must-scan
//! flags), every backend error, every directory that could not be watched, a change at a path with
//! no `RelPath` (the root itself, a name that is not UTF-8), and the end of the watch (the backend
//! thread gone: a last `Rescan(WatchEnded)`, then `next()` resolves to `None`).
//!
//! **What is not** (IDX-09's obligations, on its card): `notify` ignores inotify's `IN_UNMOUNT`
//! and `IN_IGNORED` (a watched directory's filesystem unmounted, or a watch removed by the kernel);
//! FSEvents never reports the root moved or deleted (`notify` does not ask for `RootChanged`);
//! network and FUSE filesystems may deliver no events at all; and the first walk races with
//! directories created during it (watch, then list, makes the listing see them).
//!
//! Pinned by: `watch::tests` (the event mapping), `tests/watch.rs` (the watched set) and the
//! `source_tree`
//! contract suite (a real watcher's hints, the order, and the queue's overflow).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::Poll;

use jfl_application::BlockingPool;
use jfl_application::hints::{HintQueue, HintSink, hint_queue};
use jfl_application::source_tree::{
    Hint, HintSubject, Hints, RescanReason, SourceTreeError, WatchErrorKind, hint_excluded,
};
use jfl_domain::RelPath;
use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::walk;

/// Whether the adapter watches each directory itself (inotify) or the root recursively
/// (FSEvents, which covers new directories from the start).
#[cfg(not(target_os = "macos"))]
const PER_DIRECTORY: bool = true;
#[cfg(target_os = "macos")]
const PER_DIRECTORY: bool = false;

/// The most new directories waiting to be watched; past it, the whole tree is re-watched before
/// the next hint. A checkout that creates more directories than this costs one walk instead.
const MAX_PENDING_DIRS: usize = 4096;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// New directories to watch before the next hint is handed out.
#[derive(Debug, Default)]
struct Pending {
    dirs: BTreeSet<PathBuf>,
    everything: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.dirs.is_empty() && !self.everything
    }

    fn add(&mut self, dir: PathBuf) {
        if self.dirs.len() >= MAX_PENDING_DIRS {
            self.dirs.clear();
            self.everything = true;
        } else if !self.everything {
            self.dirs.insert(dir);
        }
    }
}

fn kind(error: &notify::Error) -> WatchErrorKind {
    match &error.kind {
        notify::ErrorKind::MaxFilesWatch => WatchErrorKind::LimitReached,
        notify::ErrorKind::Io(e) if e.raw_os_error() == Some(ENOSPC) => {
            WatchErrorKind::LimitReached
        }
        notify::ErrorKind::Io(_) => WatchErrorKind::Io,
        _ => WatchErrorKind::Other,
    }
}

/// `ENOSPC`, what `inotify_add_watch` returns at `max_user_watches` (the same number on Linux's
/// architectures).
const ENOSPC: i32 = 28;

/// Whether a failed watch is a directory that went away: its removal is hinted by its parent.
fn vanished(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::PathNotFound | notify::ErrorKind::WatchNotFound => true,
        notify::ErrorKind::Io(e) => matches!(
            e.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ),
        _ => false,
    }
}

/// Watches what `pending` names: the directories the rules accept at and below each pending
/// directory, or below the root, the whole tree on `walk_threads` threads
/// ([`walk::watched_dirs`]). Returns every failure; the caller reports them.
fn settle(
    watcher: &Mutex<RecommendedWatcher>,
    root: &Path,
    pending: Pending,
    walk_threads: usize,
) -> Vec<WatchErrorKind> {
    let starts: Vec<PathBuf> = if pending.everything {
        vec![root.to_path_buf()]
    } else {
        pending.dirs.into_iter().collect()
    };
    let mut failures = Vec::new();
    for start in starts {
        let (dirs, unreadable) = walk::watched_dirs(root, &start, walk_threads);
        failures.extend(read_failures(&unreadable));
        let mut watcher = lock(watcher);
        for dir in dirs {
            if let Err(error) = watcher.watch(&dir, RecursiveMode::NonRecursive)
                && !vanished(&error)
            {
                failures.push(kind(&error));
            }
        }
    }
    failures
}

/// One failure per accepted directory that could not be read for its subdirectories.
fn read_failures(unreadable: &[std::io::ErrorKind]) -> impl Iterator<Item = WatchErrorKind> + '_ {
    unreadable.iter().map(|_| WatchErrorKind::Io)
}

/// Runs on the backend's thread.
struct Handler {
    sink: HintSink,
    pending: Arc<Mutex<Pending>>,
    root: PathBuf,
    /// The root with its links resolved: FSEvents reports real paths.
    canonical: Option<PathBuf>,
}

/// What an event names, for the extension rule.
fn subject(kind: &EventKind) -> HintSubject {
    match kind {
        EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder) => {
            HintSubject::Directory
        }
        EventKind::Create(CreateKind::File)
        | EventKind::Remove(RemoveKind::File)
        | EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Access(AccessKind::Close(AccessMode::Write)) => HintSubject::File,
        _ => HintSubject::Unknown,
    }
}

/// Whether an event may name a directory that was not there before (created or moved in).
fn may_add_a_directory(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Folder | CreateKind::Any | CreateKind::Other)
            | EventKind::Modify(ModifyKind::Name(_) | ModifyKind::Any)
            | EventKind::Any
            | EventKind::Other
    )
}

impl Handler {
    fn relative(&self, path: &Path) -> Option<RelPath> {
        let below = path
            .strip_prefix(&self.root)
            .ok()
            .or_else(|| path.strip_prefix(self.canonical.as_ref()?).ok())?;
        RelPath::new(below.to_str()?).ok()
    }
}

impl notify::EventHandler for Handler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                self.sink.rescan(RescanReason::WatcherError(kind(&error)));
                return;
            }
        };
        if event.need_rescan() {
            if PER_DIRECTORY {
                lock(&self.pending).everything = true;
            }
            self.sink.rescan(RescanReason::WatcherOverflow);
            return;
        }
        if let EventKind::Access(access) = event.kind
            && access != AccessKind::Close(AccessMode::Write)
        {
            return;
        }
        let subject = subject(&event.kind);
        let adds = PER_DIRECTORY && may_add_a_directory(&event.kind);
        for path in &event.paths {
            let Some(relative) = self.relative(path) else {
                self.sink.rescan(RescanReason::Unnamed);
                continue;
            };
            if hint_excluded(&relative, subject) {
                continue;
            }
            // Pending before hinted: whoever takes this hint finds the directory pending, and
            // watches it first.
            let is_dir = subject == HintSubject::Directory
                || std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            if adds && is_dir {
                lock(&self.pending).add(path.clone());
            }
            self.sink.changed(relative);
        }
    }
}

impl Drop for Handler {
    /// The backend dropped its handler: its thread ended (the watch was dropped, or the thread
    /// died). Either way nothing more is hinted, and the queue says so.
    fn drop(&mut self) {
        self.sink.close();
    }
}

/// A live watch; dropping it stops the backend.
pub struct FsWatch<P> {
    queue: HintQueue,
    sink: HintSink,
    pending: Arc<Mutex<Pending>>,
    watcher: Arc<Mutex<RecommendedWatcher>>,
    root: Arc<PathBuf>,
    walk_threads: usize,
    pool: P,
}

impl<P> std::fmt::Debug for FsWatch<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsWatch").field("root", &self.root).finish()
    }
}

/// What a poll of the queue found.
enum Step {
    Settle,
    Took(Option<Hint>),
}

impl<P> FsWatch<P> {
    fn report(&self, failures: &[WatchErrorKind]) {
        for failure in failures {
            self.sink.rescan(RescanReason::WatcherError(*failure));
        }
    }

    fn take_pending(&self) -> Option<Pending> {
        let mut pending = lock(&self.pending);
        (!pending.is_empty()).then(|| std::mem::take(&mut *pending))
    }

    /// Takes a hint only while nothing is pending, holding the pending lock: a directory whose
    /// hint is in the queue was recorded as pending before its hint was pushed, so every hint
    /// taken here names a directory that is already watched.
    fn poll_step(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Step> {
        let pending = lock(&self.pending);
        if !pending.is_empty() {
            return Poll::Ready(Step::Settle);
        }
        let taken = self.queue.poll_take(cx);
        drop(pending);
        taken.map(Step::Took)
    }
}

impl<P: BlockingPool + Clone> Hints for FsWatch<P> {
    fn try_next(&mut self) -> Option<Hint> {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let Some(pending) = self.take_pending() {
                let failures = settle(&self.watcher, &self.root, pending, self.walk_threads);
                self.report(&failures);
                continue;
            }
            match self.poll_step(&mut cx) {
                Poll::Ready(Step::Settle) => {}
                Poll::Ready(Step::Took(hint)) => return hint,
                Poll::Pending => return None,
            }
        }
    }

    async fn next(&mut self) -> Option<Hint> {
        loop {
            if let Some(pending) = self.take_pending() {
                let (watcher, root) = (Arc::clone(&self.watcher), Arc::clone(&self.root));
                let walk_threads = self.walk_threads;
                let failures = self
                    .pool
                    .run(move || settle(&watcher, &root, pending, walk_threads))
                    .await
                    .unwrap_or_else(|_| vec![WatchErrorKind::Other]);
                self.report(&failures);
                continue;
            }
            match std::future::poll_fn(|cx| self.poll_step(cx)).await {
                Step::Settle => {}
                Step::Took(hint) => return hint,
            }
        }
    }
}

/// Starts watching `root`, a directory, with a hint queue of `capacity`, walking the whole tree on
/// `walk_threads` threads (`FsConfig::walk_threads`, as the listing does). Every directory the rules
/// accept is watched before this returns; reaching the system's watch limit is a refusal, any
/// other directory that cannot be watched a `Rescan(WatcherError)` waiting in the queue.
///
/// Pinned by the `source_tree` contract suite (`watch_hints_a_write`, `watch_reports_queue_full`,
/// `watch_leaves_out_agent_worktrees`, `watch_watches_a_new_directory_before_its_hint`, the
/// dotted-directory cases).
pub fn watch<P>(
    root: &Path,
    capacity: usize,
    walk_threads: usize,
    pool: P,
) -> Result<FsWatch<P>, SourceTreeError> {
    let (sink, queue) = hint_queue(capacity);
    let pending: Arc<Mutex<Pending>> = Arc::default();
    let handler = Handler {
        sink: sink.clone(),
        pending: Arc::clone(&pending),
        root: root.to_path_buf(),
        canonical: std::fs::canonicalize(root).ok(),
    };
    let config = notify::Config::default().with_follow_symlinks(false);
    let mut watcher = RecommendedWatcher::new(handler, config)
        .map_err(|e| SourceTreeError::WatchFailed(kind(&e)))?;
    let failures = if PER_DIRECTORY {
        let watcher = Mutex::new(watcher);
        let failures = settle(
            &watcher,
            root,
            Pending {
                everything: true,
                ..Pending::default()
            },
            walk_threads,
        );
        let watcher = watcher.into_inner().unwrap_or_else(PoisonError::into_inner);
        (failures, watcher)
    } else {
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| SourceTreeError::WatchFailed(kind(&e)))?;
        // The recursive stream reports no directory it cannot read: walk, after watching, the
        // directories the per-directory branch would have watched, for their read failures.
        let (_, unreadable) = walk::watched_dirs(root, root, walk_threads);
        (read_failures(&unreadable).collect(), watcher)
    };
    let (failures, watcher) = failures;
    if failures.contains(&WatchErrorKind::LimitReached) {
        return Err(SourceTreeError::WatchFailed(WatchErrorKind::LimitReached));
    }
    let watch = FsWatch {
        queue,
        sink,
        pending,
        watcher: Arc::new(Mutex::new(watcher)),
        root: Arc::new(root.to_path_buf()),
        walk_threads,
        pool,
    };
    watch.report(&failures);
    Ok(watch)
}

#[cfg(test)]
mod tests {
    use jfl_application::source_tree::{Rescan, RescanReasons};
    use notify::EventHandler;
    use notify::event::{CreateKind, Flag};

    use super::*;

    fn handler(capacity: usize) -> (Handler, HintQueue) {
        let (sink, queue) = hint_queue(capacity);
        let handler = Handler {
            sink,
            pending: Arc::default(),
            root: PathBuf::from("/w/root"),
            canonical: Some(PathBuf::from("/real/root")),
        };
        (handler, queue)
    }

    fn rescan(reason: RescanReason, dropped: u64) -> Option<Hint> {
        Some(Hint::Rescan(Rescan {
            reasons: RescanReasons::of(reason),
            dropped,
        }))
    }

    #[test]
    fn should_hint_a_path_under_either_spelling_of_the_root_when_a_file_is_created() {
        // given
        let (mut handler, mut queue) = handler(8);
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/w/root/a/A.java"))
            .add_path(PathBuf::from("/real/root/b/B.java"));

        // when
        handler.handle_event(Ok(event));

        // then
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("a/A.java").unwrap()))
        );
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("b/B.java").unwrap()))
        );
    }

    #[test]
    fn should_report_a_rescan_when_the_backend_overflows() {
        // given
        let (mut handler, mut queue) = handler(8);
        let overflow = Event::new(EventKind::Other).set_flag(Flag::Rescan);

        // when
        handler.handle_event(Ok(overflow));

        // then
        assert_eq!(queue.try_next(), rescan(RescanReason::WatcherOverflow, 0));
    }

    #[test]
    fn should_report_a_rescan_when_the_backend_fails() {
        // given
        let (mut handler, mut queue) = handler(8);

        // when
        handler.handle_event(Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch)));

        // then
        assert_eq!(
            queue.try_next(),
            rescan(RescanReason::WatcherError(WatchErrorKind::LimitReached), 0)
        );
    }

    #[test]
    fn should_report_a_rescan_when_a_path_has_no_name_under_the_root() {
        // given
        let (mut handler, mut queue) = handler(8);
        let outside = Event::new(EventKind::Any).add_path(PathBuf::from("/elsewhere/A.java"));
        let root = Event::new(EventKind::Any).add_path(PathBuf::from("/w/root"));

        // when
        handler.handle_event(Ok(outside));
        handler.handle_event(Ok(root));

        // then
        assert_eq!(queue.try_next(), rescan(RescanReason::Unnamed, 0));
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_hint_nothing_when_a_file_is_only_opened_or_read() {
        // given
        let (mut handler, mut queue) = handler(8);
        let path = PathBuf::from("/w/root/A.java");
        let reads = [
            AccessKind::Open(notify::event::AccessMode::Read),
            AccessKind::Read,
            AccessKind::Close(AccessMode::Read),
        ];

        // when
        for access in reads {
            handler.handle_event(Ok(
                Event::new(EventKind::Access(access)).add_path(path.clone())
            ));
        }
        let after_reads = queue.try_next();
        handler.handle_event(Ok(Event::new(EventKind::Access(AccessKind::Close(
            AccessMode::Write,
        )))
        .add_path(path)));

        // then
        assert_eq!(after_reads, None);
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("A.java").unwrap()))
        );
    }

    #[test]
    fn should_hint_nothing_when_git_an_agent_worktree_or_a_class_file_changes() {
        // given
        let (mut handler, mut queue) = handler(8);
        let event = Event::new(EventKind::Any)
            .add_path(PathBuf::from("/w/root/.git/index"))
            .add_path(PathBuf::from("/w/root/.claude/worktrees/a/src/A.java"));
        let class = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/w/root/build/classes/A.class"));

        // when
        handler.handle_event(Ok(event));
        handler.handle_event(Ok(class));

        // then
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_record_a_new_directory_as_pending_before_its_hint_when_one_is_created() {
        // given
        let (mut handler, mut queue) = handler(8);
        let pending = Arc::clone(&handler.pending);
        let event = Event::new(EventKind::Create(CreateKind::Folder))
            .add_path(PathBuf::from("/w/root/java.base"));

        // when
        handler.handle_event(Ok(event));

        // then
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("java.base").unwrap()))
        );
        let pending = lock(&pending);
        assert_eq!(pending.dirs.len(), usize::from(PER_DIRECTORY));
    }

    #[test]
    fn should_hint_a_dotted_directory_when_it_is_renamed() {
        // given
        let (mut handler, mut queue) = handler(8);
        let event = Event::new(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Both,
        )))
        .add_path(PathBuf::from("/w/root/mod.a"))
        .add_path(PathBuf::from("/w/root/mod.b"));

        // when
        handler.handle_event(Ok(event));

        // then
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("mod.a").unwrap()))
        );
        assert_eq!(
            queue.try_next(),
            Some(Hint::Changed(RelPath::new("mod.b").unwrap()))
        );
    }

    #[test]
    fn should_end_the_hints_when_the_backend_drops_its_handler() {
        // given
        let (handler, mut queue) = handler(8);

        // when
        drop(handler);

        // then
        assert_eq!(queue.try_next(), rescan(RescanReason::WatchEnded, 0));
        assert_eq!(queue.try_next(), None);
    }
}
