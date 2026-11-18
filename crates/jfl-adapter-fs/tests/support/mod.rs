//! A scratch directory per test under cargo's `CARGO_TARGET_TMPDIR`, removed when dropped.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use jfl_domain::FileUri;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory, unique per test name and per call within the test's process.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("jfl-adapter-fs")
            .join(format!("{test}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes `rel` under `base`, creating its parents.
pub(crate) fn write(base: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let path = base.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir for {rel}: {e}"))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("write {rel}: {e}"))
}

/// The URI of an absolute, UTF-8 path.
pub(crate) fn uri(path: &Path) -> FileUri {
    FileUri::from_path(path.to_str().unwrap()).unwrap()
}
