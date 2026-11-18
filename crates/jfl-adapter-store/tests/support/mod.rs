//! A scratch directory per test under cargo's `CARGO_TARGET_TMPDIR`, removed when dropped, and the
//! disk store's contract harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jfl_adapter_store::DiskSummaryStore;
use jfl_application::LruClock;
use jfl_domain::SyntaxKey;
use jfl_testkit::contracts::summary_store::Harness;
use jfl_testkit::exec::InlinePool;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory, unique per test name and per call within the test's process.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("jfl-adapter-store")
            .join(format!("{test}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The store's root under this directory.
    pub(crate) fn root(&self) -> PathBuf {
        self.0.join("summaries")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) type Store = DiskSummaryStore<InlinePool>;

/// Opens the store under `scratch` on the inline pool.
pub(crate) fn open(scratch: &Scratch, cap: u64, clock: Arc<dyn LruClock>) -> Result<Store, String> {
    DiskSummaryStore::open(&scratch.root(), cap, clock, InlinePool)
        .map(|(store, _)| store)
        .map_err(|e| e.to_string())
}

/// Cuts `key`'s entry to half its length: a write torn by a crash.
pub(crate) fn truncate(store: &Store, key: &SyntaxKey) -> Result<(), String> {
    let path = store.path_of(key);
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let half = bytes.get(..bytes.len() / 2).unwrap_or(&[]);
    fs::write(&path, half).map_err(|e| e.to_string())
}

/// The contract harness over a scratch directory.
pub(crate) struct DiskHarness(pub(crate) Scratch);

impl Harness for DiskHarness {
    type Store = Store;

    fn open(&mut self, cap: u64, clock: Arc<dyn LruClock>) -> Result<Store, String> {
        open(&self.0, cap, clock)
    }

    fn damage(&mut self, store: &Store, key: &SyntaxKey) -> Result<(), String> {
        truncate(store, key)
    }

    fn wipe(&mut self) -> Result<(), String> {
        match fs::remove_dir_all(self.0.root()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}
