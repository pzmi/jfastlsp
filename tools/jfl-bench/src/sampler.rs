//! Run one command to completion while sampling its resident set: the `jfl-bench sample`
//! debugging command, and nothing on a measured path.
//!
//! The suites take RSS elsewhere: `rg-floor` from hyperfine's per-run `memory_peak_resident`
//! (the kernel's rusage of that child), `e1` from the child's own `VmHWM` at exit. This sampler
//! exists to look at a process by hand. It reads `/proc/<pid>/status` in a loop while the child
//! lives (the kernel's high-water mark means a coarse interval cannot miss the peak except in the
//! last instants before exit; the steady figure is the median of the current resident sets). The
//! loop spins (`try_wait`, a `/proc` read, no sleep: the harness holds no sleep grant), so it
//! **competes for a core** with a child that uses all of them and distorts its timing: never
//! time through it.
//!
//! Pinned by: `tests/sampler.rs`.

use std::ffi::OsString;
use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::error::{BenchError, Context, Result};
use crate::procfs::{self, MemStatus};
use crate::stats;

/// What one sampled run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Sampled {
    /// Wall time of the run in milliseconds.
    pub wall_ms: f64,
    /// The exit code, `None` when a signal ended the child.
    pub exit_code: Option<i32>,
    /// What the child wrote to stdout.
    pub stdout: String,
    /// Peak resident set in KiB; `None` where there is no `/proc`.
    pub peak_kib: Option<u64>,
    /// Median of the sampled resident sets in KiB; `None` where there is no `/proc`.
    pub steady_kib: Option<u64>,
    /// How many samples were taken.
    pub samples: usize,
}

/// Run `argv` with stdout captured to `stdout_file` (read back after the run, so no reader
/// thread is needed) and stderr discarded.
pub fn run(argv: &[OsString], stdout_file: &Path) -> Result<Sampled> {
    let (program, args) = argv.split_first().context("sampler: empty command")?;
    let sink = File::create(stdout_file).context(format!("create {}", stdout_file.display()))?;
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(sink))
        .stderr(Stdio::null())
        .spawn()
        .context(format!("spawn {}", program.to_string_lossy()))?;
    let pid = child.id();
    let mut peak: Option<u64> = None;
    let mut rss: Vec<f64> = Vec::new();
    let status = loop {
        if let Some(status) = child.try_wait().context("wait for the command")? {
            break status;
        }
        if let Some(MemStatus { peak_kib, rss_kib }) = procfs::read_status(Some(pid)) {
            peak = peak.max(peak_kib);
            if let Some(r) = rss_kib {
                rss.push(r as f64);
            }
        }
        std::thread::yield_now();
    };
    let wall_ms = started.elapsed().as_secs_f64() * 1e3;
    let stdout = std::fs::read_to_string(stdout_file)
        .map_err(|e| BenchError::new(format!("read {}: {e}", stdout_file.display())))?;
    let steady_kib = (!rss.is_empty()).then(|| stats::median(&rss) as u64);
    Ok(Sampled {
        wall_ms,
        exit_code: status.code(),
        stdout,
        peak_kib: peak,
        steady_kib,
        samples: rss.len(),
    })
}
