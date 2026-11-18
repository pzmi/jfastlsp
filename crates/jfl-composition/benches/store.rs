//! The summary store on the pinned corpus (IDX-08, ADR 0031): `just bench-store SIZE [cold]`.
//!
//! Reads every listed `.java` file (stdin: `cold` or `warm`, a newline, the checkout's path, a
//! newline, then `git ls-files -z '*.java'`), summarises them with `jfl-adapter-java` on 4
//! threads, keys each by its `SyntaxKey` (blake3), then measures `jfl-adapter-store` in a scratch
//! directory under cargo's `CARGO_TARGET_TMPDIR`, removed at the end:
//!
//! - the encoded bytes per file against the summary's bytes in memory;
//! - a full write of every summary into an empty store, on 1 and 4 threads, then every entry read
//!   back and compared with what was put (whole and as declarations): the real-corpus check;
//! - the store's open (the scan the cap needs), then the load of the whole view, warm: every whole
//!   summary (the small and medium tier), and every file's declarations plus the name postings
//!   built from them (the large tier, plan §5.5); with `cold`, the same after dropping the page
//!   cache (`/proc/sys/vm/drop_caches`, Linux, root: it disturbs everything else on the machine,
//!   so only when asked);
//! - the postings: distinct names, bytes, the lookup time over every name, and a `refs`-shaped
//!   read of the most used names' files whole;
//! - the bytes each tier holds in memory, and both projected to elasticsearch's 33,850 files
//!   (plan §1.7's 250 MB syntax-tier budget) from this corpus's mean file.
//!
//! Medians of three timed runs after a warm-up. Here, not in the adapter, because timing and
//! threads need the clock and spawn grants only composition holds (fnd §2.4). Reports, never
//! gates.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer, SUMMARISER_VERSION};
use jfl_adapter_store::{Blake3, DiskSummaryStore, SortedPostings, codec};
use jfl_application::syntax_indexer::strip_bom;
use jfl_application::{CountingClock, Declarations, NamePostings};
use jfl_domain::{FileSummary, SyntaxInputs, SyntaxKey};

/// Timed runs; the median is reported.
const RUNS: usize = 3;
/// elasticsearch's Java files (plan §1.7), for the projections.
const LARGE_FILES: f64 = 33_850.0;
/// The scratch store's cap: everything fits.
const CAP: u64 = u64::MAX;

type Store = DiskSummaryStore<()>;

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort_unstable();
    runs.get(runs.len() / 2).copied().unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn mib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0)
}

/// Runs `job` over `items` on `threads` threads, each taking the next item; results in order.
fn parallel<I: Sync, T: Send>(items: &[I], threads: usize, job: impl Fn(&I) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let parts: Vec<Vec<(usize, T)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(at) else { break };
                        out.push((at, job(item)));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().filter_map(|w| w.join().ok()).collect()
    });
    let mut all: Vec<(usize, T)> = parts.into_iter().flatten().collect();
    all.sort_unstable_by_key(|(at, _)| *at);
    all.into_iter().map(|(_, value)| value).collect()
}

fn timed<T>(job: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let value = job();
    (start.elapsed(), value)
}

fn open(root: &Path) -> Result<Store, String> {
    DiskSummaryStore::open(root, CAP, Arc::new(CountingClock::default()), ())
        .map(|(store, _)| store)
        .map_err(|e| e.to_string())
}

fn drop_caches() -> bool {
    std::fs::write("/proc/sys/vm/drop_caches", b"3\n").is_ok()
}

fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

struct Loaded {
    whole: Duration,
    declarations: Duration,
    postings: Duration,
    misses: usize,
}

fn load(root: &Path, keys: &[SyntaxKey]) -> Result<(Duration, Loaded), String> {
    let (opened, store) = timed(|| open(root));
    let store = store?;
    let (whole, got) = timed(|| parallel(keys, 4, |k| store.get_blocking(k)));
    let misses = got.iter().filter(|g| g.is_none()).count();
    drop(got);
    let (declarations, got) = timed(|| parallel(keys, 4, |k| store.get_declarations_blocking(k)));
    let held: Vec<(u32, &Declarations)> = (0_u32..)
        .zip(&got)
        .filter_map(|(n, d)| d.as_ref().map(|d| (n, d)))
        .collect();
    let (postings, index) = timed(|| SortedPostings::build(held.iter().copied()));
    drop(index);
    Ok((
        opened,
        Loaded {
            whole,
            declarations,
            postings,
            misses,
        },
    ))
}

/// Whether cold runs were asked for, the checkout, and each file's path and bytes.
type Input = (bool, PathBuf, Vec<(String, Vec<u8>)>);

fn read_input() -> Result<Input, String> {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    let mut lines = input.splitn(3, |&b| b == b'\n');
    let cold = lines.next() == Some(b"cold");
    let root = lines
        .next()
        .ok_or("stdin: cold|warm, the checkout's path, then NUL-separated paths")?;
    let root = PathBuf::from(std::str::from_utf8(root).map_err(|e| e.to_string())?);
    let mut files = Vec::new();
    for rel in lines.next().unwrap_or(&[]).split(|&b| b == 0) {
        if rel.is_empty() {
            continue;
        }
        let rel = std::str::from_utf8(rel).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        files.push((rel.to_owned(), bytes));
    }
    Ok((cold, root, files))
}

fn main() -> Result<(), String> {
    let (cold, root, files) = read_input()?;
    let total: usize = files.iter().map(|(_, b)| b.len()).sum();
    println!(
        "corpus {}: {} files, {:.1} MB",
        root.display(),
        files.len(),
        total as f64 / 1e6
    );
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let id = grammar.id();
    let indexer = JavaIndexer::new(grammar, ());
    let indexed = parallel(&files, 4, |(_, bytes)| {
        let key = SyntaxKey::derive(
            &SyntaxInputs {
                file: Blake3::digest(strip_bom(bytes)),
                grammar: id.name,
                grammar_version: id.version,
                summariser_version: SUMMARISER_VERSION,
            },
            Blake3::new(),
        );
        indexer.index_blocking(bytes).ok().map(|i| (key, i.summary))
    });
    let summaries: Vec<(SyntaxKey, Arc<FileSummary>)> = indexed
        .into_iter()
        .flatten()
        .map(|(k, s)| (k, Arc::new(s)))
        .collect();
    let keys: Vec<SyntaxKey> = summaries.iter().map(|(k, _)| *k).collect();
    let n = summaries.len();
    let nf = n.max(1) as f64;
    println!(
        "summarised {n} files ({} refused)",
        files.len().saturating_sub(n)
    );

    // Encoded size.
    let mut encoded: Vec<usize> = Vec::with_capacity(n);
    let mut heap = 0_usize;
    let mut decl_heap = 0_usize;
    for (key, summary) in &summaries {
        encoded.push(
            codec::encode(key, summary)
                .map_err(|e| e.to_string())?
                .len(),
        );
        heap = heap
            .saturating_add(summary.heap_bytes())
            .saturating_add(size_of::<FileSummary>());
        decl_heap = decl_heap
            .saturating_add(Declarations::of(summary).heap_bytes())
            .saturating_add(size_of::<Declarations>());
    }
    let encoded_total: usize = encoded.iter().sum();
    encoded.sort_unstable();
    let at = |p: usize| {
        encoded
            .get(n.saturating_mul(p) / 100)
            .copied()
            .unwrap_or_default()
    };
    println!(
        "encoded bytes per file: mean {:.0}, p50 {}, p95 {}, max {}; all {:.1} MiB ({:.2} of the {:.1} MiB in memory)",
        encoded_total as f64 / nf,
        at(50),
        at(95),
        encoded.last().copied().unwrap_or_default(),
        mib(encoded_total as f64),
        encoded_total as f64 / heap.max(1) as f64,
        mib(heap as f64)
    );

    // Writes.
    let scratch =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("bench-store-{}", std::process::id()));
    let store_root = scratch.join("summaries");
    println!("threads | full write ms (median of {RUNS}) | files/s | MiB/s");
    for threads in [1, 4] {
        let mut runs = Vec::new();
        for run in 0..=RUNS {
            let _ = std::fs::remove_dir_all(&scratch);
            let store = open(&store_root)?;
            let (took, results) = timed(|| {
                parallel(&summaries, threads, |(key, summary)| {
                    store.put_blocking(*key, summary)
                })
            });
            if let Some(e) = results.iter().find_map(|r| r.as_ref().err()) {
                return Err(e.to_string());
            }
            if run > 0 {
                runs.push(took);
            }
        }
        let took = median(runs);
        println!(
            "{threads} | {:.0} | {:.0} | {:.1}",
            ms(took),
            nf / took.as_secs_f64(),
            mib(encoded_total as f64) / took.as_secs_f64()
        );
    }

    // Correctness on the real corpus: every entry reads back as what was put.
    {
        let store = open(&store_root)?;
        let equal = parallel(&summaries, 4, |(key, summary)| {
            store.get_blocking(key).as_ref() == Some(&**summary)
                && store.get_declarations_blocking(key) == Some(Declarations::of(summary))
        });
        let same = equal.iter().filter(|e| **e).count();
        println!("round trip: {same} of {n} entries read back equal, whole and as declarations");
        if same != n {
            return Err("an entry read back differently".to_owned());
        }
    }

    // Loads.
    let report = |label: &str, runs: Vec<(Duration, Loaded)>| {
        let opened = median(runs.iter().map(|r| r.0).collect());
        let whole = median(runs.iter().map(|r| r.1.whole).collect());
        let declarations = median(runs.iter().map(|r| r.1.declarations).collect());
        let postings = median(runs.iter().map(|r| r.1.postings).collect());
        let misses: usize = runs.iter().map(|r| r.1.misses).sum();
        println!(
            "{label} (4 threads, median of {}): open {:.0} ms; whole view {:.0} ms; declarations {:.0} ms + postings {:.0} ms = {:.0} ms; {misses} misses",
            runs.len(),
            ms(opened),
            ms(whole),
            ms(declarations),
            ms(postings),
            ms(declarations.saturating_add(postings))
        );
        (opened, whole, declarations.saturating_add(postings))
    };
    let _warm_up = load(&store_root, &keys)?;
    let warm: Vec<_> = (0..RUNS)
        .map(|_| load(&store_root, &keys))
        .collect::<Result<_, _>>()?;
    let (w_open, w_whole, w_decl) = report("warm load", warm);
    if !cold {
        println!("cold load skipped: not asked for (`just bench-store SIZE cold`)");
    } else if !drop_caches() {
        println!("cold load skipped: the page cache cannot be dropped here (Linux, root)");
    } else {
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            drop_caches();
            runs.push(load(&store_root, &keys)?);
        }
        report("cold load", runs);
    }
    let scale = LARGE_FILES / nf;
    println!(
        "warm load projected to elasticsearch ({LARGE_FILES} files, linear in files): open {:.0} ms; whole {:.0} ms; declarations and postings {:.0} ms",
        ms(w_open) * scale,
        ms(w_whole) * scale,
        ms(w_decl) * scale
    );

    // Postings.
    let store = open(&store_root)?;
    let declarations: Vec<Declarations> =
        summaries.iter().map(|(_, s)| Declarations::of(s)).collect();
    let postings = SortedPostings::build((0_u32..).zip(&declarations));
    let mut names: Vec<(&str, usize)> = Vec::new();
    for d in &declarations {
        for name in d.names() {
            names.push((name, 0));
        }
    }
    names.sort_unstable();
    names.dedup_by_key(|(name, _)| *name);
    for entry in &mut names {
        entry.1 = postings.files(entry.0).len();
    }
    let lookups: Vec<Duration> = (0..RUNS)
        .map(|_| {
            timed(|| {
                names
                    .iter()
                    .map(|(name, _)| postings.files(name).len())
                    .sum::<usize>()
            })
            .0
        })
        .collect();
    let per_lookup = median(lookups).as_secs_f64() * 1e9 / names.len().max(1) as f64;
    let entries: usize = names.iter().map(|(_, files)| files).sum();
    println!(
        "postings: {} names, {entries} postings ({:.1} names per file), {:.2} MiB; lookup {per_lookup:.0} ns per name (all names, median of {RUNS})",
        postings.names(),
        entries as f64 / nf,
        mib(postings.heap_bytes() as f64)
    );
    names.sort_unstable_by_key(|(_, files)| core::cmp::Reverse(*files));
    for (name, files) in names.iter().take(3) {
        let listed = postings.files(name).to_vec();
        let (took, sites) = timed(|| {
            parallel(&listed, 4, |ordinal| {
                let at = usize::try_from(*ordinal).unwrap_or(usize::MAX);
                keys.get(at)
                    .and_then(|k| store.get_blocking(k))
                    .map_or(0, |s| {
                        s.references()
                            .iter()
                            .filter(|r| s.str(r.name) == *name)
                            .count()
                    })
            })
        });
        println!(
            "refs-shaped read of `{name}`: {files} files listed ({:.1} % of the view), {} sites, {:.1} ms warm",
            *files as f64 * 100.0 / nf,
            sites.iter().sum::<usize>(),
            ms(took)
        );
    }

    // Memory.
    let large = decl_heap as f64 + postings.heap_bytes() as f64;
    println!(
        "in memory: whole summaries {:.1} MiB ({:.0} B per file); declarations {:.1} MiB + postings {:.1} MiB = {:.1} MiB ({:.0} B per file)",
        mib(heap as f64),
        heap as f64 / nf,
        mib(decl_heap as f64),
        mib(postings.heap_bytes() as f64),
        mib(large),
        large / nf
    );
    println!(
        "projected to elasticsearch ({LARGE_FILES} files): whole summaries {:.0} MiB; declarations and postings {:.0} MiB (postings scaled linearly: an upper bound, names repeat across files); store on disk {:.0} MiB",
        mib(heap as f64 * scale),
        mib(large * scale),
        mib(encoded_total as f64 * scale)
    );
    println!("process: {}", rss());
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}
