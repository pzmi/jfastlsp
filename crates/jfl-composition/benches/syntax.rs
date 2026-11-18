//! The syntax index on the pinned corpus (IDX-07, ADR 0031): `just bench-syntax SIZE`.
//!
//! Reads every listed `.java` file into memory first (stdin: the checkout's path on the first
//! line, then `git ls-files -z '*.java'`), then indexes them all with `jfl-adapter-java` on 1 and
//! 4 threads, each thread with its own parser and query cursor, a warm-up and three timed runs
//! per thread count. Prints files per second, MB per second, the per-file p50, p95 and max, the
//! wall time (median run), and per file the summary's bytes, declarations and reference sites,
//! files with syntax errors and refused files (each named), and the most of its work budget any
//! file used. Here, not in the adapter, because timing and
//! threads need the clock and spawn grants only composition holds (fnd §2.4); and not in
//! `jfl-bench`, which imports no product crate (fnd §2.1). Reports, never gates.

use std::io::Read as _;
use std::mem::size_of;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_domain::FileSummary;

/// Thread counts: E1's (plan §1.7).
const THREADS: [usize; 2] = [1, 4];
/// Timed runs per thread count; the median is reported.
const RUNS: usize = 3;

struct Run {
    wall: Duration,
    per_file: Vec<Duration>,
    summary_bytes: Vec<usize>,
    decls: usize,
    refs: usize,
    error_files: usize,
    refused: usize,
}

/// One file's time, and its summary's bytes, declarations, reference sites and whether it has
/// syntax errors (`None`: refused).
type FileResult = (Duration, Option<(usize, usize, usize, bool)>);

fn index_all(indexer: &JavaIndexer<()>, files: &[Vec<u8>], threads: usize) -> Run {
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    let parts: Vec<Vec<FileResult>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(bytes) = files.get(at) else { break };
                        let start = Instant::now();
                        let result = indexer.index_blocking(bytes);
                        let took = start.elapsed();
                        out.push((
                            took,
                            result.ok().map(|i| {
                                let s = &i.summary;
                                (
                                    s.heap_bytes().saturating_add(size_of::<FileSummary>()),
                                    s.declarations().count(),
                                    s.references().len(),
                                    !i.diagnostics.is_empty(),
                                )
                            }),
                        ));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().filter_map(|w| w.join().ok()).collect()
    });
    let wall = started.elapsed();
    let mut run = Run {
        wall,
        per_file: Vec::new(),
        summary_bytes: Vec::new(),
        decls: 0,
        refs: 0,
        error_files: 0,
        refused: 0,
    };
    for (took, result) in parts.into_iter().flatten() {
        run.per_file.push(took);
        match result {
            Some((bytes, decls, refs, errors)) => {
                run.summary_bytes.push(bytes);
                run.decls = run.decls.saturating_add(decls);
                run.refs = run.refs.saturating_add(refs);
                run.error_files = run.error_files.saturating_add(usize::from(errors));
            }
            None => run.refused = run.refused.saturating_add(1),
        }
    }
    run
}

fn percentile<T: Copy + Ord + Default>(sorted: &[T], p: usize) -> T {
    let at = sorted.len().saturating_mul(p).checked_div(100).unwrap_or(0);
    sorted
        .get(at.min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> Result<(), String> {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    let newline = input
        .iter()
        .position(|&b| b == b'\n')
        .ok_or("stdin: the checkout's path, a newline, then NUL-separated paths")?;
    let (root, list) = input.split_at(newline);
    let root = Path::new(std::str::from_utf8(root).map_err(|e| e.to_string())?);
    let mut files = Vec::new();
    let mut names = Vec::new();
    for rel in list.get(1..).unwrap_or(&[]).split(|&b| b == 0) {
        if rel.is_empty() {
            continue;
        }
        let rel = std::str::from_utf8(rel).map_err(|e| e.to_string())?;
        files.push(std::fs::read(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?);
        names.push(rel.to_owned());
    }
    let total: usize = files.iter().map(Vec::len).sum();
    let mb = total as f64 / 1e6;
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let indexer = JavaIndexer::new(grammar, ());
    println!(
        "corpus {}: {} files, {mb:.1} MB",
        root.display(),
        files.len()
    );
    println!("threads | wall ms (median of {RUNS}) | files/s | MB/s | p50 ms | p95 ms | max ms");
    let mut last = None;
    for threads in THREADS {
        let _warm = index_all(&indexer, &files, threads);
        let mut runs: Vec<Run> = (0..RUNS)
            .map(|_| index_all(&indexer, &files, threads))
            .collect();
        runs.sort_by_key(|r| r.wall);
        let Some(run) = runs.into_iter().nth(RUNS / 2) else {
            continue;
        };
        let mut per_file = run.per_file.clone();
        per_file.sort_unstable();
        let secs = run.wall.as_secs_f64();
        println!(
            "{threads} | {:.0} | {:.0} | {:.1} | {:.3} | {:.3} | {:.1}",
            ms(run.wall),
            files.len() as f64 / secs,
            mb / secs,
            ms(percentile(&per_file, 50)),
            ms(percentile(&per_file, 95)),
            ms(per_file.last().copied().unwrap_or_default()),
        );
        last = Some(run);
    }
    if let Some(run) = last {
        let mut bytes = run.summary_bytes.clone();
        bytes.sort_unstable();
        let summarised = bytes.len().max(1);
        let sum: usize = bytes.iter().sum();
        println!(
            "summary bytes per file: mean {:.0}, p50 {}, p95 {}, max {}; all summaries {:.1} MiB",
            sum as f64 / summarised as f64,
            percentile(&bytes, 50),
            percentile(&bytes, 95),
            bytes.last().copied().unwrap_or_default(),
            sum as f64 / (1024.0 * 1024.0)
        );
        println!(
            "per file: {:.1} declarations, {:.1} reference sites; {} files with syntax errors; {} refused",
            run.decls as f64 / summarised as f64,
            run.refs as f64 / summarised as f64,
            run.error_files,
            run.refused
        );
    }
    let mut worst = (0.0_f64, String::new());
    for (name, bytes) in names.iter().zip(&files) {
        let (result, work) = indexer.index_blocking_with_work(bytes);
        let ratio = work.used as f64 / work.limit.max(1) as f64;
        if ratio > worst.0 {
            worst = (ratio, name.clone());
        }
        match result {
            Ok(indexed) if !indexed.diagnostics.is_empty() => {
                let first = indexed
                    .diagnostics
                    .first()
                    .map(|d| (d.range, d.problem.clone()));
                println!(
                    "syntax errors: {name}: {} (first {first:?})",
                    indexed.diagnostics.len()
                );
            }
            Err(e) => println!("refused: {name}: {e}"),
            Ok(_) => {}
        }
    }
    println!(
        "work budget: at most {:.2} % used (by {})",
        worst.0 * 100.0,
        worst.1
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
        {
            println!("{line}");
        }
    }
    Ok(())
}
