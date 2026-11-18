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
//! the file cap) are recorded in [`Listing::unlisted`], never dropped.

use std::ffi::OsStr;
use std::fs;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ignore::{DirEntry, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use jfl_application::source_tree::{
    BUILD_FILES, Exclusion, GitEntry, ListMode, ListedFile, Listing, Skipped, Unlisted,
    UnlistedReason, exclusion, is_git_metadata,
};
use jfl_domain::RelPath;

use crate::FsConfig;
use crate::file::file_stat;

/// Lists the `.java` files under `root`, a directory.
///
/// Pinned by the `source_tree` contract suite (`tests/contract.rs`), `tests/worktrees.rs` and
/// `tests/symlinks.rs`.
#[must_use]
pub fn list(root: &Path, mode: ListMode, config: &FsConfig) -> Listing {
    let skipped: Arc<Mutex<Vec<Skipped>>> = Arc::default();
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
    builder.filter_entry(move |entry| keep(entry, &filter_root, &filter_skipped));

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

fn git_entry(dir: &Path) -> Option<GitEntry> {
    let meta = fs::symlink_metadata(dir.join(".git")).ok()?;
    Some(if meta.is_dir() {
        GitEntry::Dir
    } else {
        GitEntry::File
    })
}

fn has_build_file(dir: &Path) -> bool {
    BUILD_FILES
        .iter()
        .any(|name| fs::symlink_metadata(dir.join(name)).is_ok())
}

fn is_java_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    bytes.len() > b".java".len() && bytes.ends_with(b".java")
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
/// Pinned by: `tests/watch.rs::should_watch_only_what_the_rules_accept_when_a_tree_is_walked`.
#[must_use]
pub fn watched_dirs(root: &Path, start: &Path) -> (Vec<PathBuf>, Vec<std::io::ErrorKind>) {
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
    let mut dirs = Vec::new();
    let mut errors = Vec::new();
    for entry in builder.build() {
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|t| t.is_dir()) => {
                dirs.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => match error.io_error().map(std::io::Error::kind) {
                Some(std::io::ErrorKind::NotFound) | None => {}
                Some(kind) => errors.push(kind),
            },
        }
    }
    (dirs, errors)
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
        let reported = is_java_name(name) || fs::metadata(path).is_ok_and(|m| m.is_dir());
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
                if is_file && is_java_name(entry.file_name()) {
                    return self.file(&entry);
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
        for gap in local.unlisted {
            if shared.unlisted.len() < jfl_application::source_tree::MAX_REPORTED_UNLISTED {
                shared.unlisted.push(gap);
            }
        }
    }
}
