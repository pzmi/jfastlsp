//! The walk: `ignore`'s parallel walker with the exclusions of plan §4.2.
//!
//! Each walker thread collects into its own [`Listing`] and merges it once, when the walker drops
//! the thread's visitor, so threads never contend per file. The exclusion rules run in
//! `filter_entry`, before the walker reads a directory, so a skipped subtree costs one or two
//! `lstat`s and is never read.
//!
//! What is left out, and why:
//! - `.gitignore` (every level below the root) and `$GIT_DIR/info/exclude` apply, without needing
//!   a `.git` here; ignore files above the root, the user's global gitignore and ripgrep's `.ignore`
//!   files do not, so a listing depends on the worktree alone. A file that is tracked and also
//!   matches a `.gitignore` pattern is left out, where Git keeps it (`just bench-fs` compares the
//!   listing with `git ls-files` on the corpus to catch that).
//! - Hidden directories are walked: Git tracks them.
//! - `jfl_application::source_tree::exclusion`'s rules, and `.git` itself.
//! - Symbolic links are never followed; one to a directory or to a `.java` file is reported as
//!   skipped (plan §11).
//!
//! Gaps (an unreadable directory, a `.java` name that is not UTF-8 or too long, a stat that fails,
//! the file cap) are recorded in [`Listing::unlisted`], never dropped. Other JVM languages' sources
//! and build scripts are recorded by name in [`Listing::other`] (IDX-16): no stat, no read, and
//! never against the file cap.

use std::ffi::OsStr;
use std::fs;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ignore::{DirEntry, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use jfl_application::languages::indexed_language_of_bytes;
use jfl_application::source_tree::{
    BUILD_FILES, Exclusion, GitEntry, GitMarker, ListMode, ListedFile, Listing, Skipped, Unlisted,
    UnlistedReason, exclusion, is_git_metadata, is_other_source_name, nested_root,
};
use jfl_domain::RelPath;

use crate::FsConfig;
use crate::file::file_stat;

/// Lists the `.java` files under `root`, a directory, and counts the other JVM languages' sources
/// and build scripts beside them.
///
/// Pinned by the `source_tree` contract suite (`tests/contract.rs`), `tests/worktrees.rs` and
/// `tests/symlinks.rs`.
#[must_use]
pub fn list(root: &Path, mode: ListMode, config: &FsConfig) -> Listing {
    list_on(root, mode, config, device_of)
}

/// The device `path` lives on, links not followed.
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    fs::symlink_metadata(path).ok().map(|meta| meta.dev())
}

/// [`list`], with `device` telling which device a directory lives on (`lstat`'s `st_dev` in
/// [`list`]; a test stands in a mount point with it). A directory on another device than the
/// root is a mount point: it is not walked and is reported as a gap
/// ([`UnlistedReason::OtherFilesystem`]), since the root's filesystem type, which decides whether
/// the view is watched, says nothing about it (a network or FUSE mount inside a worktree).
///
/// Pinned by: `tests/files.rs::should_report_a_gap_and_not_walk_it_when_a_directory_is_another_filesystem`.
pub fn list_on(
    root: &Path,
    mode: ListMode,
    config: &FsConfig,
    device: fn(&Path) -> Option<u64>,
) -> Listing {
    let skipped: Arc<Mutex<Vec<Skipped>>> = Arc::default();
    let crossed: Arc<Mutex<Vec<Option<RelPath>>>> = Arc::default();
    let root_device = device(root);
    let shared = Mutex::new(Listing::default());
    let files = AtomicUsize::new(0);

    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .threads(config.walk_threads);
    let filter_root = root.to_path_buf();
    let filter_skipped = Arc::clone(&skipped);
    let filter_crossed = Arc::clone(&crossed);
    builder.filter_entry(move |entry| {
        if entry.depth() > 0
            && entry.file_type().is_some_and(|t| t.is_dir())
            && device(entry.path()) != root_device
        {
            filter_crossed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(relative(&filter_root, entry.path()).ok());
            return false;
        }
        keep(entry, &filter_root, &filter_skipped)
    });

    let mut visitors = Visitors {
        root,
        mode,
        max_files: config.max_files,
        files: &files,
        shared: &shared,
    };
    builder.build_parallel().visit(&mut visitors);

    let mut listing = shared.into_inner().unwrap_or_else(PoisonError::into_inner);
    let skipped = mem::take(&mut *skipped.lock().unwrap_or_else(PoisonError::into_inner));
    listing.skipped = skipped;
    for path in mem::take(&mut *crossed.lock().unwrap_or_else(PoisonError::into_inner)) {
        listing.record_unlisted(Unlisted {
            path,
            why: UnlistedReason::OtherFilesystem,
        });
    }
    listing.sort();
    listing
}

/// The path of `path` under `root`, when it is a valid [`RelPath`].
fn relative(root: &Path, path: &Path) -> Result<RelPath, UnlistedReason> {
    let below = path
        .strip_prefix(root)
        .map_err(|_| UnlistedReason::Unreadable)?;
    let text = below.to_str().ok_or(UnlistedReason::NonUtf8Name)?;
    RelPath::new(text).map_err(|error| match error {
        jfl_domain::PathError::TooLong { .. } => UnlistedReason::PathTooLong,
        _ => UnlistedReason::Unreadable,
    })
}

/// `jfl_application::source_tree::nested_root` on what `lstat` finds at `dir/.git`: any entry,
/// links not followed (the same rule `jfl-adapter-git`'s root resolution applies).
fn git_entry(dir: &Path) -> Option<GitEntry> {
    let found = fs::symlink_metadata(dir.join(".git")).ok().map(|meta| {
        let kind = meta.file_type();
        GitMarker::of(kind.is_dir(), kind.is_file(), kind.is_symlink())
    });
    nested_root(found)
}

fn has_build_file(dir: &Path) -> bool {
    BUILD_FILES
        .iter()
        .any(|name| fs::symlink_metadata(dir.join(name)).is_ok())
}

/// Whether the registry gives this name an indexed language, by its bytes: a name that is not
/// UTF-8 is still listed, as a gap.
fn is_indexed_name(name: &OsStr) -> bool {
    indexed_language_of_bytes(name.as_bytes()).is_some()
}

/// The exclusion rule for the directory `dir` strictly below `root`; `None` to walk into it.
fn dir_exclusion(root: &Path, dir: &Path) -> Option<Exclusion> {
    let name = dir.file_name()?.to_str()?;
    let parent = dir
        .parent()
        .filter(|parent| *parent != root)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str);
    exclusion(
        parent,
        name,
        || git_entry(dir),
        || dir.parent().is_some_and(has_build_file),
    )
}

/// The directories at and below `start` (`root` or a directory under it) that a watch covers:
/// every directory the exclusion rules accept, `.gitignore`d ones included (a `.gitignore` edit
/// can bring them back), never through a link. `start` itself is left out when it is a link, not
/// a directory, or excluded. The second value is each directory that could not be read (`NotFound`
/// left out: a directory removed meanwhile is hinted by its parent's watch).
///
/// The whole tree (`start` is `root`, a watch's start) is walked by `ignore`'s parallel walker on
/// `threads` threads (`FsConfig::walk_threads`; 0 lets `ignore` choose), as the listing is: after a page-cache drop each of medium's 2,800 directories is a wait on the
/// disk, and one thread waiting on each in turn took about 100 ms of the first answer
/// (`doc/bench/README.md`, IDX-19). A directory below the root (a new one, from a hint) is walked
/// on the calling thread.
///
/// Pinned by: `tests/watch.rs::should_watch_only_what_the_rules_accept_when_a_tree_is_walked`.
#[must_use]
pub fn watched_dirs(
    root: &Path,
    start: &Path,
    threads: usize,
) -> (Vec<PathBuf>, Vec<std::io::ErrorKind>) {
    if start != root {
        let is_dir = fs::symlink_metadata(start).is_ok_and(|m| m.is_dir());
        if !is_dir || dir_exclusion(root, start).is_some() || inside_excluded(root, start) {
            return (Vec::new(), Vec::new());
        }
    }
    let mut builder = WalkBuilder::new(start);
    builder.standard_filters(false).follow_links(false);
    let filter_root = root.to_path_buf();
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        is_dir
            && !entry.file_name().to_str().is_some_and(is_git_metadata)
            && dir_exclusion(&filter_root, entry.path()).is_none()
    });
    let found: Mutex<(Vec<PathBuf>, Vec<std::io::ErrorKind>)> = Mutex::default();
    let take = |entry: Result<DirEntry, ignore::Error>| {
        let mut found = found.lock().unwrap_or_else(PoisonError::into_inner);
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|t| t.is_dir()) => {
                found.0.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => match error.io_error().map(std::io::Error::kind) {
                Some(std::io::ErrorKind::NotFound) | None => {}
                Some(kind) => found.1.push(kind),
            },
        }
    };
    if start == root {
        builder.threads(threads).build_parallel().run(|| {
            Box::new(|entry| {
                take(entry);
                WalkState::Continue
            })
        });
    } else {
        builder.build().for_each(take);
    }
    found.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `dir` lies under `.git` or `.claude/worktrees/` below `root`.
fn inside_excluded(root: &Path, dir: &Path) -> bool {
    relative(root, dir).is_ok_and(|path| {
        jfl_application::source_tree::hint_excluded(
            &path,
            jfl_application::source_tree::HintSubject::Directory,
        )
    })
}

/// `filter_entry`: whether the walker yields `entry` and, for a directory, reads it.
fn keep(entry: &DirEntry, root: &Path, skipped: &Mutex<Vec<Skipped>>) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name();
    if name.to_str().is_some_and(is_git_metadata) {
        return false;
    }
    let Some(file_type) = entry.file_type() else {
        return true;
    };
    let path = entry.path();
    let why = if file_type.is_symlink() {
        let reported = is_indexed_name(name) || fs::metadata(path).is_ok_and(|m| m.is_dir());
        if !reported {
            return false;
        }
        Some(Exclusion::Symlink)
    } else if file_type.is_dir() {
        dir_exclusion(root, path)
    } else {
        None
    };
    let Some(why) = why else {
        return true;
    };
    if let Ok(path) = relative(root, path) {
        skipped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Skipped { path, why });
    }
    false
}

struct Visitors<'s> {
    root: &'s Path,
    mode: ListMode,
    max_files: usize,
    files: &'s AtomicUsize,
    shared: &'s Mutex<Listing>,
}

impl<'s> ParallelVisitorBuilder<'s> for Visitors<'s> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(Visitor {
            root: self.root,
            mode: self.mode,
            max_files: self.max_files,
            files: self.files,
            shared: self.shared,
            local: Listing::default(),
        })
    }
}

/// One walker thread's collector; merges into the shared listing when the walker drops it.
struct Visitor<'s> {
    root: &'s Path,
    mode: ListMode,
    max_files: usize,
    files: &'s AtomicUsize,
    shared: &'s Mutex<Listing>,
    local: Listing,
}

impl Visitor<'_> {
    fn gap(&mut self, path: Option<RelPath>, why: UnlistedReason) {
        self.local.record_unlisted(Unlisted { path, why });
    }

    fn file(&mut self, entry: &DirEntry) -> WalkState {
        let path = match relative(self.root, entry.path()) {
            Ok(path) => path,
            Err(why) => {
                let parent = entry
                    .path()
                    .parent()
                    .and_then(|parent| relative(self.root, parent).ok());
                self.gap(parent, why);
                return WalkState::Continue;
            }
        };
        if self.files.fetch_add(1, Ordering::Relaxed) >= self.max_files {
            self.gap(None, UnlistedReason::TooManyFiles);
            return WalkState::Quit;
        }
        let stat = match self.mode {
            ListMode::Paths => None,
            ListMode::PathsAndStat => match entry.metadata() {
                Ok(meta) => Some(file_stat(&meta)),
                Err(_) => {
                    self.gap(Some(path), UnlistedReason::StatFailed);
                    return WalkState::Continue;
                }
            },
        };
        self.local.files.push(ListedFile { path, stat });
        WalkState::Continue
    }
}

/// The deepest path an `ignore` error names.
fn error_path(error: &ignore::Error) -> Option<&Path> {
    match error {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        ignore::Error::Loop { child, .. } => Some(child),
        _ => None,
    }
}

impl ParallelVisitor for Visitor<'_> {
    fn visit(&mut self, entry: Result<DirEntry, ignore::Error>) -> WalkState {
        match entry {
            Ok(entry) => {
                let is_file = entry.file_type().is_some_and(|t| t.is_file());
                if is_file && is_indexed_name(entry.file_name()) {
                    return self.file(&entry);
                }
                if is_file && is_other_source_name(&entry.file_name().to_string_lossy()) {
                    self.local
                        .record_other(relative(self.root, entry.path()).ok());
                }
                WalkState::Continue
            }
            // A malformed `.gitignore` line is skipped by the walker as Git skips it; only an I/O
            // failure leaves part of the tree unseen.
            Err(error) if error.io_error().is_none() => WalkState::Continue,
            Err(error) => {
                let path: Option<PathBuf> = error_path(&error).map(Path::to_path_buf);
                let path = path.and_then(|p| relative(self.root, &p).ok());
                self.gap(path, UnlistedReason::Unreadable);
                WalkState::Continue
            }
        }
    }
}

impl Drop for Visitor<'_> {
    fn drop(&mut self) {
        let local = mem::take(&mut self.local);
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.files.extend(local.files);
        shared.unlisted_count = shared.unlisted_count.saturating_add(local.unlisted_count);
        shared.other_count = shared.other_count.saturating_add(local.other_count);
        let room =
            jfl_application::source_tree::MAX_LISTED_OTHER.saturating_sub(shared.other.len());
        shared.other.extend(local.other.into_iter().take(room));
        for gap in local.unlisted {
            if shared.unlisted.len() < jfl_application::source_tree::MAX_REPORTED_UNLISTED {
                shared.unlisted.push(gap);
            }
        }
    }
}
