//! Fixture builders: real files and real `git` in a caller's scratch directory.

pub mod git;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh directory under a caller's base (the system's temporary directory, short, so socket
/// paths under it fit `sun_path`; this crate holds no env grant to read it), removed with
/// everything in it when dropped: for test crates that hold no `fs` grant of their own
/// (`jfl-cli`'s).
#[derive(Debug)]
pub struct ScratchDir(PathBuf);

static SCRATCH: AtomicUsize = AtomicUsize::new(0);

impl ScratchDir {
    /// `<base>/<prefix>-<pid>-<n>`, empty.
    pub fn new(base: &Path, prefix: &str) -> Result<Self, String> {
        let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("{prefix}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).map_err(|e| format!("mkdir {}: {e}", path.display()))?;
        Ok(Self(path))
    }

    /// Its path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file under `dir`, by its path relative to `dir` (`/`-separated), with its bytes: a
/// fixture tree to hand [`git::Repo::init`].
pub fn files_under(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        let entries = std::fs::read_dir(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned();
                let bytes = std::fs::read(&path).map_err(|e| format!("{rel}: {e}"))?;
                out.push((rel, bytes));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Makes a FIFO at `path` with `mkfifo` (the test crates hold no `libc` and forbid `unsafe`).
pub fn fifo(path: &Path) -> Result<(), String> {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .map_err(|e| format!("mkfifo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mkfifo exited with {status}"))
    }
}
