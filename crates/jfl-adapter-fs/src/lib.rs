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
    FileStat, ListMode, Listing, ReadBytes, SourceTree, SourceTreeError,
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

    async fn watch(&self, root: &FileUri) -> Result<FsWatch<P>, SourceTreeError> {
        let root = root.clone();
        let capacity = self.config.hint_capacity;
        let pool = self.pool.clone();
        self.pool
            .run(move || watch::watch(&root_dir(&root)?, capacity, pool))
            .await?
    }
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
