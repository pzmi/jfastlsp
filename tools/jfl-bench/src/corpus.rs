//! Locating a fetched corpus checkout and describing it.
//!
//! `scripts/bench-corpus.sh` (BENCH-01) fetches and verifies the pinned repositories into a cache
//! directory outside this repository; this module only asks it where a size lives (`path SIZE`)
//! and reads back what is there. It never fetches: a benchmark that clones on the measured path
//! would time the network.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::e1;
use crate::error::{BenchError, Context, Result};

/// Facts about the corpus checkout a run measured, for the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusFacts {
    /// The size name (`small`, `medium`, ...).
    pub size: String,
    /// The repository's directory name.
    pub repo: String,
    /// The checked-out commit.
    pub commit: String,
    /// `.java` files a `.gitignore`-aware walk finds.
    pub java_files: u64,
}

/// The checkout of `size`: `explicit` when given, else what `scripts/bench-corpus.sh path SIZE`
/// prints from `repo_root`. Fails, naming the fetch command, when nothing is checked out there.
pub fn locate(repo_root: &Path, size: &str, explicit: Option<&Path>) -> Result<PathBuf> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => {
            let script = repo_root.join("scripts/bench-corpus.sh");
            let out = Command::new(&script)
                .args(["path", size])
                .output()
                .context(format!("run {} path {size}", script.display()))?;
            if !out.status.success() {
                return Err(BenchError::new(format!(
                    "bench-corpus.sh path {size}: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            PathBuf::from(String::from_utf8_lossy(&out.stdout).trim())
        }
    };
    if !path.is_dir() {
        return Err(BenchError::new(format!(
            "no corpus checkout at {}; fetch it with `scripts/bench-corpus.sh fetch {size}`",
            path.display()
        )));
    }
    Ok(path)
}

/// Describe the checkout at `root` as the size `size`.
pub fn describe(root: &Path, size: &str) -> Result<CorpusFacts> {
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let repo = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());
    Ok(CorpusFacts {
        size: size.to_string(),
        repo,
        commit,
        java_files: e1::list_java(root).len() as u64,
    })
}
