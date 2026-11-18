//! The view registry on the pinned corpus (IDX-09, ADR 0031): `just bench-views SIZE`.
//!
//! Reads the checkout's path and its tracked `.java` files (stdin: the path, a newline, then
//! `git ls-files -z '*.java'`), stats each with `lstat` and measures `jfl-application`'s view
//! registry over those real stats, warm:
//!
//! - the stat of every file (what a rescan's stat pass costs) and the open of the view from them
//!   (generation 1 published);
//! - one request's freshness path: the `lstat` of its file, `request_start` (bind, the stat
//!   compared with the published entry, the racy rule), the cookie delivered back, the pin (the
//!   re-stat compared, completeness computed) and `finish`, over 2,000 files spread over the view:
//!   p50 and p95 of the registry's part and of the `lstat` alone;
//! - one changed file reconciled and published (a build beside the published generation, the
//!   new generation's index built from the old one);
//! - a full rescan (`sync`): every file re-statted, nothing changed, published.
//!
//! The summaries are not read or stored here (`just bench-syntax` and `just bench-store` time
//! those): each key is a number, held by an in-memory set. No cookie file is written: the cookie's
//! round trip through the watcher is the daemon's (the shell's) cost, measured when it lands.
//! Medians of five runs after a warm-up. Here, not in `jfl-application`, because timing needs the
//! clock grant only composition holds (fnd §2.4). Reports, never gates.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jfl_application::source_tree::{FileStat, MTime};
use jfl_application::views::{Event, FileSeen, GitSeen, RequestId, Stamp, Stored, ViewRegistry};
use jfl_domain::{FileUri, RelPath};

/// Timed runs; the median is reported.
const RUNS: usize = 5;
/// Requests timed per run.
const REQUESTS: usize = 2_000;
/// The coarsest timestamp step assumed (Linux's coarse clock tick).
const GRANULARITY: Duration = Duration::from_millis(10);

type Registry = ViewRegistry<FileSeen, FileSeen, u32>;

struct Keys(BTreeSet<u32>);

impl Stored<u32> for Keys {
    fn holds(&self, key: &u32) -> bool {
        self.0.contains(key)
    }
}

fn now() -> MTime {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    MTime {
        secs: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        nanos: since.subsec_nanos(),
    }
}

fn lstat(path: &Path) -> Option<FileStat> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(FileStat {
        size: meta.len(),
        mtime: MTime {
            secs: meta.mtime(),
            nanos: u32::try_from(meta.mtime_nsec()).unwrap_or(0),
        },
        inode: meta.ino(),
    })
}

fn seen(path: &Path) -> FileSeen {
    let observed = now();
    FileSeen::new(lstat(path), observed, GRANULARITY)
}

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort_unstable();
    runs.get(runs.len() / 2).copied().unwrap_or_default()
}

fn percentile(mut samples: Vec<Duration>, pct: usize) -> Duration {
    samples.sort_unstable();
    let at = samples.len().saturating_mul(pct) / 100;
    samples
        .get(at.min(samples.len().saturating_sub(1)))
        .copied()
        .unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000_000.0
}

fn git_seen(root: &Path) -> GitSeen<FileSeen> {
    GitSeen {
        head: seen(&root.join(".git/HEAD")),
        index: seen(&root.join(".git/index")),
    }
}

fn main() -> Result<(), String> {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(input).map_err(|e| e.to_string())?;
    let (checkout, listing) = text.split_once('\n').ok_or("no checkout path on stdin")?;
    let checkout = Path::new(checkout);
    let root = FileUri::from_path(&checkout.display().to_string()).map_err(|e| e.to_string())?;
    let files: Vec<RelPath> = listing
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(|p| RelPath::new(p).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let n = files.len();
    let keys = Keys((0..u32::try_from(n).map_err(|e| e.to_string())?).collect());
    println!(
        "bench-views: {} ({n} tracked .java files)",
        checkout.display()
    );

    let stat_all = |files: &[RelPath]| -> Vec<FileSeen> {
        files
            .iter()
            .map(|f| seen(&checkout.join(f.as_str())))
            .collect()
    };
    let open = |seen: &[FileSeen]| -> Result<Registry, String> {
        let mut registry = Registry::new();
        registry
            .open(
                root.clone(),
                true,
                git_seen(checkout),
                files
                    .iter()
                    .zip(seen)
                    .zip(0_u32..)
                    .map(|((f, s), k)| (f.clone(), *s, k)),
            )
            .map_err(|e| e.to_string())?;
        Ok(registry)
    };

    let _ = open(&stat_all(&files))?;
    let (mut stat_runs, mut open_runs) = (Vec::new(), Vec::new());
    let mut stats = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        stats = stat_all(&files);
        stat_runs.push(started.elapsed());
        let started = Instant::now();
        let _ = open(&stats)?;
        open_runs.push(started.elapsed());
    }
    println!(
        "  stat of every file: {:.1} ms; open (generation 1 of {n} files): {:.2} ms",
        ms(median(stat_runs)),
        ms(median(open_runs))
    );

    let mut registry = open(&stats)?;
    let step = (n / REQUESTS).max(1);
    let picks: Vec<&RelPath> = files.iter().step_by(step).take(REQUESTS).collect();
    let uris: Vec<FileUri> = picks
        .iter()
        .map(|p| root.join(p).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let git = git_seen(checkout);
    let (mut registry_part, mut stat_part, mut complete) = (Vec::new(), Vec::new(), 0_usize);
    for run in 0..RUNS {
        for (i, (path, uri)) in picks.iter().zip(&uris).enumerate() {
            let id = RequestId(
                u64::try_from(run.saturating_mul(REQUESTS).saturating_add(i)).unwrap_or(0),
            );
            let started = Instant::now();
            let now_seen = seen(&checkout.join(path.as_str()));
            let statted = started.elapsed();
            let started = Instant::now();
            registry
                .request_start(id, uri, &now_seen, &git, Stamp(0))
                .map_err(|e| e.to_string())?;
            registry
                .deliver(Event::Cookie { id, at: Stamp(0) })
                .map_err(|e| e.to_string())?;
            let pin = registry.request_pin(id, &git).map_err(|e| e.to_string())?;
            if pin.completeness.is_complete() {
                complete = complete.saturating_add(1);
            }
            registry.finish(id).map_err(|e| e.to_string())?;
            registry_part.push(started.elapsed());
            stat_part.push(statted);
        }
    }
    println!(
        "  request (start, cookie back, pin, finish), registry only: p50 {:.2} µs, p95 {:.2} µs; its lstat: p50 {:.2} µs, p95 {:.2} µs; {complete} of {} complete",
        us(percentile(registry_part.clone(), 50)),
        us(percentile(registry_part, 95)),
        us(percentile(stat_part.clone(), 50)),
        us(percentile(stat_part, 95)),
        RUNS.saturating_mul(picks.len()),
    );

    let listed: BTreeSet<RelPath> = files.iter().cloned().collect();
    let changed = files.get(n / 2).ok_or("no file")?.clone();
    let mut one_runs = Vec::new();
    let mut rescan_runs = Vec::new();
    for run in 0..RUNS {
        let mut registry = open(&stats)?;
        let started = Instant::now();
        registry
            .deliver(Event::File {
                view: root.clone(),
                path: changed.clone(),
            })
            .map_err(|e| e.to_string())?;
        registry
            .reconcile_start(
                &root,
                None,
                |p| seen(&checkout.join(p.as_str())),
                || git.clone(),
            )
            .map_err(|e| e.to_string())?;
        let key = u32::try_from(n / 2).map_err(|e| e.to_string())?;
        registry
            .summarised(
                &root,
                &changed,
                seen(&checkout.join(changed.as_str())),
                key,
                Stamp(1),
            )
            .map_err(|e| e.to_string())?;
        registry.publish(&root, &keys).map_err(|e| e.to_string())?;
        one_runs.push(started.elapsed());

        let started = Instant::now();
        registry.sync_start(&root).map_err(|e| e.to_string())?;
        let pending = registry
            .reconcile_start(
                &root,
                Some(&listed),
                |p| seen(&checkout.join(p.as_str())),
                || git_seen(checkout),
            )
            .map_err(|e| e.to_string())?;
        if !pending.is_empty() && run == 0 {
            println!("  (rescan found {} changed files)", pending.len());
        }
        for path in &pending {
            registry
                .summarised(
                    &root,
                    path,
                    seen(&checkout.join(path.as_str())),
                    0,
                    Stamp(2),
                )
                .map_err(|e| e.to_string())?;
        }
        registry.publish(&root, &keys).map_err(|e| e.to_string())?;
        registry.sync_done(&root).map_err(|e| e.to_string())?;
        rescan_runs.push(started.elapsed());
    }
    println!(
        "  one changed file reconciled and published: {:.2} ms; full rescan (sync, every file re-statted, published): {:.1} ms",
        ms(median(one_runs)),
        ms(median(rescan_runs))
    );
    Ok(())
}
