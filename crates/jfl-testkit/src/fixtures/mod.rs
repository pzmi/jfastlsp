//! Fixture builders: real files and real `git` in a caller's scratch directory.

pub mod git;

use std::path::Path;
use std::process::Command;

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
