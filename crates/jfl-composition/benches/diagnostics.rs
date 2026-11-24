//! Syntax diagnostics through the coordinator on the pinned corpus (LSP-04, ADR 0031):
//! `just bench-diagnostics SIZE`.
//!
//! Reads every listed `.java` file into memory first (stdin: the checkout's path on the first
//! line, then `git ls-files -z '*.java'`). Then, per file, on one thread, as the daemon will run
//! it: the document is opened and its first compile published; then a `didChange` that breaks
//! the file (an unclosed class appended) is timed from the coordinator's `did_change` to the
//! publication it decides: start the job, the syntax pass (`jfl-adapter-java`: parse, summary and
//! diagnostics, as `index_blocking`), deliver. Prints the per-edit p50, p95 and max, the
//! coordinator's own share (the same cycle without the parse), the largest file's cost, and how
//! many diagnostics the edit produced.
//!
//! **In process, no transport:** the publication path through the daemon and `jfl lsp` is not
//! built yet (LSP-04's card), so this is the compute part of didChange to `publishDiagnostics`,
//! not the end-to-end latency a client sees. Here, not in the adapter, because timing needs the
//! clock grant only composition holds (fnd §2.4). Reports, never gates.

use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_application::diagnostics::{Coordinator, Delivery, Reply, Text};

/// Appended by the timed edit: a class left open, so every file gets at least one diagnostic.
const BREAK: &[u8] = b"\nclass Broken {\n";

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    let at = sorted.len().saturating_mul(p).checked_div(100).unwrap_or(0);
    sorted
        .get(at.min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// One compile cycle of `doc` at its pending epoch: start, the syntax pass of `bytes` (or none),
/// deliver. Returns the diagnostics published.
fn cycle(
    coordinator: &mut Coordinator<usize>,
    indexer: &JavaIndexer<()>,
    doc: usize,
    bytes: Option<&[u8]>,
) -> Result<usize, String> {
    let job = coordinator.start(&doc).map_err(|e| e.to_string())?;
    let diagnostics = match bytes {
        Some(bytes) => indexer
            .index_blocking(bytes)
            .map(|i| i.diagnostics.len())
            .unwrap_or_default(),
        None => 0,
    };
    match coordinator.deliver(Reply {
        job: job.id,
        diagnostics,
    }) {
        Delivery::Publish(p) => Ok(p.diagnostics),
        other => Err(format!("not published: {other:?}")),
    }
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
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let indexer = JavaIndexer::new(grammar, ());
    println!(
        "corpus {}: {} files, {:.1} MB",
        root.display(),
        files.len(),
        total as f64 / 1e6
    );

    let mut coordinator: Coordinator<usize> = Coordinator::new(1, []);
    coordinator
        .worker_started(jfl_application::diagnostics::Incarnation(1))
        .map_err(|e| e.to_string())?;
    let mut edits = Vec::with_capacity(files.len());
    let mut own = Vec::with_capacity(files.len());
    let mut published = 0_usize;
    let mut largest = (0_usize, Duration::ZERO, String::new());
    let mut stamp = 0_u64;
    for (doc, (bytes, name)) in files.iter().zip(&names).enumerate() {
        let text = String::from_utf8_lossy(bytes).into_owned();
        stamp = stamp.saturating_add(1);
        let _opened = coordinator
            .did_open(doc, 1, Text::new(stamp, text.clone()))
            .map_err(|e| e.to_string())?;
        let _first = cycle(&mut coordinator, &indexer, doc, Some(bytes))?;

        let mut broken = bytes.clone();
        broken.extend_from_slice(BREAK);
        let edited = format!("{text}{}", String::from_utf8_lossy(BREAK));
        stamp = stamp.saturating_add(1);
        let start = Instant::now();
        let _changed = coordinator
            .did_change(doc, 2, Text::new(stamp, edited.clone()))
            .map_err(|e| e.to_string())?;
        let diagnostics = cycle(&mut coordinator, &indexer, doc, Some(&broken))?;
        let took = start.elapsed();
        edits.push(took);
        published = published.saturating_add(diagnostics);
        if bytes.len() > largest.0 {
            largest = (bytes.len(), took, name.clone());
        }

        stamp = stamp.saturating_add(1);
        let start = Instant::now();
        let _changed = coordinator
            .did_change(doc, 3, Text::new(stamp, edited))
            .map_err(|e| e.to_string())?;
        let _none = cycle(&mut coordinator, &indexer, doc, None)?;
        own.push(start.elapsed());

        let _closed = coordinator.did_close(doc).map_err(|e| e.to_string())?;
    }
    edits.sort_unstable();
    own.sort_unstable();
    println!(
        "didChange to publication (in process, syntax pass): p50 {:.3} ms, p95 {:.3} ms, max {:.1} ms over {} edits",
        ms(percentile(&edits, 50)),
        ms(percentile(&edits, 95)),
        ms(edits.last().copied().unwrap_or_default()),
        edits.len()
    );
    println!(
        "the coordinator alone (no parse): p50 {:.1} us, p95 {:.1} us, max {:.1} us",
        ms(percentile(&own, 50)) * 1000.0,
        ms(percentile(&own, 95)) * 1000.0,
        ms(own.last().copied().unwrap_or_default()) * 1000.0
    );
    println!(
        "largest file: {} ({:.2} MB): {:.1} ms",
        largest.2,
        largest.0 as f64 / 1e6,
        ms(largest.1)
    );
    println!(
        "diagnostics published by the breaking edits: {published} ({:.1} per file)",
        published as f64 / edits.len().max(1) as f64
    );
    Ok(())
}
