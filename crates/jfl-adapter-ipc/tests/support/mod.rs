//! A scratch directory per test under `/tmp`, removed when dropped, with short names: a socket
//! path must fit `sun_path` (107 bytes on Linux, 103 on macOS) with a notify socket's name, and a
//! target directory (`CARGO_TARGET_TMPDIR`) can be too long for that. The crate holds no env grant
//! to read `TMPDIR`, and `/tmp` exists on every platform of ADR 0006.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const BASE: &str = "/tmp";

/// A fresh directory, unique per call within the test's process.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(BASE).join(format!("jfl-ipc-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A path for the state directory, not created.
    pub(crate) fn state(&self) -> PathBuf {
        self.0.join("s")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A current-thread runtime with IO, for the daemon side.
pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap()
}
