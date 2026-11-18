//! A scratch directory per test under cargo's `CARGO_TARGET_TMPDIR`, removed when dropped, and
//! the adapter set to look for no root at or above it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_domain::FileUri;
use jfl_testkit::exec::InlinePool;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh, canonical directory, unique per test name and per call within the test's process.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("jfl-adapter-git")
            .join(format!("{test}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The adapter, with this directory as its ceiling.
    pub(crate) fn probe(&self) -> GitRepoProbe<InlinePool> {
        GitRepoProbe::new(
            InlinePool,
            GitConfig {
                ceiling: Some(self.0.clone()),
                ..GitConfig::default()
            },
        )
    }

    /// `base/path` as a URI.
    pub(crate) fn uri(&self, path: &str) -> FileUri {
        FileUri::from_path(self.0.join(path).to_str().unwrap()).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
