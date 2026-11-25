//! `SourceTree` over the filesystem: scanning with nested-worktree exclusion, reading, stat and
//! watcher hints. Role: adapter.
//!
//! - The walk ([`walk`]) is `ignore`'s parallel walker (ripgrep's): `.gitignore`, `.git/info/exclude`
//!   and nothing outside the root (no parent directories' ignore files, no global gitignore, no
//!   `.ignore`), hidden directories walked, symbolic links never followed, and the exclusions of
//!   `jfl_application::source_tree::exclusion` (plan §4.2).
//! - Reads are bounded and refuse anything but a regular file; the file is opened non-blocking, so
//!   a FIFO cannot block a read ([`file`]).
//! - The watcher ([`watch`]) is `notify`'s recommended backend: on Linux one inotify watch per
//!   directory the exclusion rules accept, added before a new directory's hint is handed out
//!   (watch, then hint); on macOS one recursive FSEvents stream. Its hints go through a bounded,
//!   coalescing `HintQueue`; what it reports and what it cannot are in [`watch`]'s doc (plan
//!   §4.3).
//!
//! Every blocking job runs on the [`BlockingPool`] composition gives the adapter (fnd §4.4); the
//! `*_blocking` methods are the same jobs for callers already off the runtime (the benchmark).
//!
//! What it does is the `source_tree` contract suite, run against it in `tests/contract.rs`.

use std::path::{Path, PathBuf};

use jfl_application::source_tree::{
    FileStat, FsKind, ListMode, Listing, MTime, ReadBytes, SourceTree, SourceTreeError, is_cookie,
    is_cookie_name,
};
use jfl_application::{BlockingPool, DEFAULT_HINT_CAPACITY};
use jfl_domain::{FileUri, RelPath};

pub mod file;
pub mod walk;
pub mod watch;

pub use watch::FsWatch;

/// The most files one listing holds before it stops and reports `TooManyFiles`: ten times the
/// largest corpus repository (elasticsearch, 33.9k files), far past any real worktree, so only a
/// pathological tree is cut.
pub const DEFAULT_MAX_FILES: usize = 1_000_000;

/// How the adapter walks and watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsConfig {
    /// The walker's threads; 0 lets `ignore` choose (the available parallelism, at most 12).
    pub walk_threads: usize,
    /// Each watch's hint queue capacity (`jfl_application::hints`).
    pub hint_capacity: usize,
    /// The file cap of one listing.
    pub max_files: usize,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            walk_threads: 0,
            hint_capacity: DEFAULT_HINT_CAPACITY,
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

/// The filesystem `SourceTree`.
#[derive(Debug, Clone)]
pub struct FsSourceTree<P> {
    pool: P,
    config: FsConfig,
}

impl<P: BlockingPool> FsSourceTree<P> {
    /// An adapter running its blocking jobs on `pool`.
    pub fn new(pool: P, config: FsConfig) -> Self {
        Self { pool, config }
    }

    /// [`SourceTree::list`], on the calling thread.
    pub fn list_blocking(
        &self,
        root: &FileUri,
        mode: ListMode,
    ) -> Result<Listing, SourceTreeError> {
        let root = root_dir(root)?;
        Ok(walk::list(&root, mode, &self.config))
    }

    /// [`SourceTree::read`], on the calling thread.
    pub fn read_blocking(
        &self,
        root: &FileUri,
        path: &RelPath,
        limit: usize,
    ) -> Result<ReadBytes, SourceTreeError> {
        file::read(&root_dir(root)?, path, limit)
    }

    /// [`SourceTree::stat`], on the calling thread.
    pub fn stat_blocking(
        &self,
        root: &FileUri,
        path: &RelPath,
    ) -> Result<FileStat, SourceTreeError> {
        file::stat(&root_dir(root)?, path)
    }

    /// [`SourceTree::watch`], on the calling thread.
    pub fn watch_blocking(&self, root: &FileUri) -> Result<FsWatch<P>, SourceTreeError>
    where
        P: Clone,
    {
        watch::watch(
            &root_dir(root)?,
            self.config.hint_capacity,
            self.config.walk_threads,
            self.pool.clone(),
        )
    }
}

impl<P: BlockingPool + Clone> SourceTree for FsSourceTree<P> {
    type Watch = FsWatch<P>;

    async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
        let root = root.clone();
        let config = self.config;
        self.pool
            .run(move || {
                let root = root_dir(&root)?;
                Ok(walk::list(&root, mode, &config))
            })
            .await?
    }

    async fn read(
        &self,
        root: &FileUri,
        path: &RelPath,
        limit: usize,
    ) -> Result<ReadBytes, SourceTreeError> {
        let (root, path) = (root.clone(), path.clone());
        self.pool
            .run(move || file::read(&root_dir(&root)?, &path, limit))
            .await?
    }

    async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
        let (root, path) = (root.clone(), path.clone());
        self.pool
            .run(move || file::stat(&root_dir(&root)?, &path))
            .await?
    }

    async fn prefetch(&self, root: &FileUri, paths: &[RelPath], limit: usize) {
        let (root, paths) = (root.clone(), paths.to_vec());
        let _ = self
            .pool
            .run(move || {
                if let Ok(root) = root_dir(&root) {
                    file::prefetch(&root, &paths, limit);
                }
            })
            .await;
    }

    async fn watch(&self, root: &FileUri) -> Result<FsWatch<P>, SourceTreeError> {
        let root = root.clone();
        let (capacity, threads) = (self.config.hint_capacity, self.config.walk_threads);
        let pool = self.pool.clone();
        self.pool
            .run(move || watch::watch(&root_dir(&root)?, capacity, threads, pool))
            .await?
    }

    async fn filesystem(&self, root: &FileUri) -> Result<FsKind, SourceTreeError> {
        let root = root.clone();
        self.pool.run(move || filesystem(&root_dir(&root)?)).await?
    }

    async fn sweep_cookies(&self, root: &FileUri, before: MTime) -> Result<u64, SourceTreeError> {
        let root = root.clone();
        self.pool
            .run(move || sweep_cookies(&root_dir(&root)?, before))
            .await?
    }

    async fn write_cookie(&self, root: &FileUri, name: &RelPath) -> Result<(), SourceTreeError> {
        let (root, name) = (root.clone(), name.clone());
        self.pool
            .run(move || write_cookie(&root_dir(&root)?, &name))
            .await?
    }
}

/// The kind of filesystem `dir` lives on, from `statfs(2)`'s `f_type` (every magic fits in 32
/// bits; a 32-bit `f_type` holds the larger ones as negative numbers, hence the mask).
///
/// Pinned by: the `source_tree` contract's `reports_a_local_filesystem` (`tests/contract.rs`).
#[cfg(target_os = "linux")]
fn filesystem(dir: &Path) -> Result<FsKind, SourceTreeError> {
    let stat = rustix::fs::statfs(dir).map_err(|e| file::map_io(&e.into()))?;
    let magic = u64::try_from(i128::from(stat.f_type) & 0xFFFF_FFFF).unwrap_or(0);
    Ok(FsKind::from_linux_magic(magic))
}

/// The kind of filesystem `dir` lives on, from `statfs(2)`'s `f_fstypename`.
///
/// Pinned by: the `source_tree` contract's `reports_a_local_filesystem` (`tests/contract.rs`).
#[cfg(target_os = "macos")]
fn filesystem(dir: &Path) -> Result<FsKind, SourceTreeError> {
    let stat = rustix::fs::statfs(dir).map_err(|e| file::map_io(&e.into()))?;
    let name: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|c| **c != 0)
        .flat_map(|c| c.to_ne_bytes())
        .collect();
    Ok(core::str::from_utf8(&name).map_or(FsKind::Other, FsKind::from_bsd_name))
}

/// Elsewhere the type is not read: never watched.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn filesystem(dir: &Path) -> Result<FsKind, SourceTreeError> {
    let _ = dir;
    Ok(FsKind::Other)
}

/// Removes the stale cookies directly in `root`: regular files (by `lstat`: a link is never
/// followed or removed) whose name is exactly a cookie's and whose mtime is before `before`.
///
/// Pinned by: `tests/files.rs::should_sweep_only_stale_regular_cookies_when_a_view_opens`.
fn sweep_cookies(root: &Path, before: MTime) -> Result<u64, SourceTreeError> {
    let mut removed = 0_u64;
    for entry in std::fs::read_dir(root).map_err(|e| file::map_io(&e))? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_cookie_name) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let stale = meta.file_type().is_file() && file::file_stat(&meta).mtime < before;
        if stale && std::fs::remove_file(&path).is_ok() {
            removed = removed.saturating_add(1);
        }
    }
    Ok(removed)
}

/// Creates the cookie `root/name` (mode 0600, never over an existing file, which is left alone:
/// `Io(AlreadyExists)`, and the caller takes the next name) and removes it: its events reach the
/// root's watch in order with every earlier change (ADR 0037), as the cookie's hint or folded
/// into a rescan already waiting (FSEvents' event on the root, for one). Only a cookie name,
/// directly in the root, is written.
///
/// Pinned by: the `source_tree` contract's `cookie_follows_earlier_changes` (`tests/contract.rs`).
fn write_cookie(root: &Path, name: &RelPath) -> Result<(), SourceTreeError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    if !is_cookie(name) {
        return Err(SourceTreeError::Io(std::io::ErrorKind::InvalidInput));
    }
    let path = root.join(name.as_str());
    let create = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
    };
    // `create_new` refuses an existing name, link or not, with `AlreadyExists`.
    drop(create().map_err(|e| file::map_io(&e))?);
    std::fs::remove_file(&path).map_err(|e| file::map_io(&e))
}

/// The root as a directory path, refused when it is missing or not a directory.
fn root_dir(root: &FileUri) -> Result<PathBuf, SourceTreeError> {
    let path = Path::new(root.path());
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(path.to_path_buf()),
        Ok(_) => Err(SourceTreeError::RootNotADirectory),
        Err(error) => Err(match file::map_io(&error) {
            SourceTreeError::NotFound => SourceTreeError::RootNotFound,
            other => other,
        }),
    }
}
