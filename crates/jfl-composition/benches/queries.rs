//! The queries on the pinned corpus (IDX-10, ADR 0031, plan §1.7): `just bench-queries SIZE`.
//!
//! Reads the checkout's path (stdin: the path, a newline, then the tracked `.java` files), opens
//! the view through `jfl-application`'s view service over the real adapters (the fs tree, the git
//! probe, the Java indexer, a scratch summary store; every port on one pool of four threads, as
//! in `view_service.rs`), settles it, and then measures each query over the published generation:
//!
//! - **definition** and **hover** at reference sites picked by a fixed seed over the view's
//!   summaries (`--picks N`, default 200): the query alone (the request already pinned), and the
//!   request plus the query (stat, cookie through inotify, pin, query: what a carrier waits for
//!   inside the daemon); p50 and p95, and what the answers were (exact, ambiguous, possible only,
//!   outside the index, unresolved, refused);
//! - **the first query of a generation**, which builds its name tables (`QueryCache`): what the
//!   first query after each publish paid before IDX-18, and what a fresh cache still pays;
//! - **the first query after an edit** (IDX-18), on a scratch repository of the checkout's files:
//!   a file rewritten (a comment appended), then a definition whose request reconciles and
//!   publishes it and whose query makes the new generation's name tables from the previous ones;
//!   then the file put back, the same again;
//! - **definitions repeated in one file** (IDX-18): ten sites of each of twenty files in a row,
//!   the file's first and the later ones apart (what the cache of parsed trees serves);
//! - **the syntax facts** a definition proves `exact` from (IDX-10's syntax-tree resolver), for
//!   every file of the view: the parse of the file alone, the parse and the walk from one site
//!   (what each definition pays: the design taken, a parse at query time), and the walk from every
//!   site of the file beyond its parse (what keeping the facts in the summary would add to the
//!   index: the design not taken); p50, p95 and the largest file;
//! - **references** (IDX-11) at method declarations picked by seed among names used at 200 or
//!   more sites of the view (common) and at 1 to 10 (rare): the query alone, p50 and p95, and
//!   what the answers held; and hover at the same declarations, which counts their references;
//! - **workspace symbols**: the first three letters of picked declaration names (prefix), the
//!   whole names (exact), their humps' initials (`NPE` for `NullPointerException`), and one
//!   letter (the widest scan), and the prefixes again with a limit of one (the scan with a single
//!   file read, to tell the scan from the reads of the listed symbols' files); p50 and p95 per kind.
//!
//! Plan §1.7 measures `jfl def` and `jfl hover` end to end through the CLI (from M2) and through
//! `jfl lsp` (from M1); this bench is the in-process share of those rows, the floor a carrier adds
//! its framing and IPC to. Here, not in `jfl-application`, because timing needs the clock grant
//! only composition holds (fnd §2.4). Reports, never gates; prints the process's peak and
//! current RSS after the open and at the end. Writes nothing into the checkout but the cookie
//! files, each removed at once: the edits go to a scratch repository under cargo's
//! `CARGO_TARGET_TMPDIR`, removed after (and by the next run, if one is cut short).

use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_adapter_store::{Blake3, DiskSummaryStore};
use jfl_application::queries::{definition, hover, references, workspace_symbols};
use jfl_application::source_tree::MTime;
use jfl_application::{
    Answer, BlockingPool, ClockDeadline, CountingClock, Interrupted, Pinned, QueryCache,
    QueryConfig, QueryError, ReferencesQuery, ServiceConfig, Summary, SymbolQuery, ViewService,
    WallClock,
};
use jfl_domain::{
    ByteOffset, ByteRange, Envelope, FileUri, Freshness, LineIndex, PositionEncoding, RelPath,
    TargetStatus,
};

/// Reference sites picked when `--picks` is not given.
const PICKS: usize = 200;
/// Symbol queries of each kind.
const SYMBOL_QUERIES: usize = 50;

/// The value after `flag` on the command line.
fn arg_after(flag: &str) -> Option<usize> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == flag)?;
    args.get(at.checked_add(1)?)?.parse().ok()
}

type Job = Box<dyn FnOnce() + Send>;

/// A fixed pool of threads taking jobs from one queue: what composition's bounded blocking pool
/// does for every port (this bench has no runtime). Each job wakes its caller when it ends.
#[derive(Debug, Clone)]
struct Pool(mpsc::SyncSender<Job>);

/// The pool's queue bound: far above what the service keeps in flight
/// (`ServiceConfig::parallelism` files, each one step at a time), so a send never waits.
const QUEUE: usize = 1024;

type Slot<T> = Arc<Mutex<(Option<T>, Option<Waker>)>>;

impl Pool {
    fn new(threads: usize) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<Job>(QUEUE);
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..threads {
            let rx = Arc::clone(&rx);
            thread::Builder::new()
                .spawn(move || {
                    loop {
                        let job = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        match job {
                            Ok(job) => job(),
                            Err(_) => return,
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(Self(tx))
    }
}

impl BlockingPool for Pool {
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let slot: Slot<T> = Arc::new(Mutex::new((None, None)));
        let done = Arc::clone(&slot);
        let sent = self.0.send(Box::new(move || {
            let out = job();
            let mut guard = done.lock().unwrap_or_else(PoisonError::into_inner);
            guard.0 = Some(out);
            if let Some(waker) = guard.1.take() {
                waker.wake();
            }
        }));
        let failed = sent.is_err();
        std::future::poll_fn(move |cx| {
            if failed {
                return Poll::Ready(Err(Interrupted));
            }
            let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.0.take() {
                Some(out) => Poll::Ready(Ok(out)),
                None => {
                    guard.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}

/// The system clock; a deadline polls it.
#[derive(Debug, Clone, Copy)]
struct SystemClock;

impl WallClock for SystemClock {
    fn now(&self) -> MTime {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        MTime {
            secs: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
            nanos: since.subsec_nanos(),
        }
    }

    fn deadline(&self, after: Duration) -> impl Future<Output = ()> + Send {
        let end = Instant::now().checked_add(after);
        std::future::poll_fn(move |cx| {
            if end.is_none_or(|end| Instant::now() >= end) {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
    }
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        thread::park();
    }
}

type Service = ViewService<
    FsSourceTree<Pool>,
    GitRepoProbe<Pool>,
    JavaIndexer<Pool>,
    DiskSummaryStore<Pool>,
    SystemClock,
    Blake3,
>;

fn service(store: &Path, pool: &Pool, config: ServiceConfig) -> Result<Service, String> {
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let (store, _) = DiskSummaryStore::open(
        store,
        u64::MAX,
        Arc::new(CountingClock::default()),
        pool.clone(),
    )
    .map_err(|e| e.to_string())?;
    Ok(ViewService::new(
        (
            FsSourceTree::new(pool.clone(), FsConfig::default()),
            GitRepoProbe::new(pool.clone(), GitConfig::default()),
            JavaIndexer::new(grammar, pool.clone()),
            store,
        ),
        SystemClock,
        Blake3::default(),
        config,
    ))
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

fn line(label: &str, samples: &[Duration]) -> String {
    format!(
        "{label}: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms over {}",
        ms(percentile(samples.to_vec(), 50)),
        ms(percentile(samples.to_vec(), 95)),
        ms(samples.iter().copied().max().unwrap_or_default()),
        samples.len()
    )
}

/// A fixed-seed generator (xorshift64): the same picks on every machine for one pinned commit.
struct Picker(u64);

impl Picker {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(
            self.0
                .checked_rem(u64::try_from(n).unwrap_or(1))
                .unwrap_or(0),
        )
        .unwrap_or(0)
    }
}

/// What an answer was, for the tally.
fn kind(envelope: &Envelope) -> &'static str {
    match (
        envelope.target().map(|t| t.status),
        envelope.exact().is_empty(),
        envelope.possible().is_empty(),
    ) {
        (Some(TargetStatus::Exact), false, _) => "exact",
        (Some(TargetStatus::Ambiguous { .. }), _, false) => "ambiguous",
        (_, true, false) => "possible only",
        _ if envelope
            .incomplete_because()
            .iter()
            .any(|r| matches!(r, jfl_domain::IncompleteReason::TargetNotIndexed)) =>
        {
            "outside the index"
        }
        _ => "unresolved",
    }
}

fn freshness() -> Freshness {
    Freshness {
        synced_at: None,
        pending_events: 0,
    }
}

/// The humps' initials of `name` (`NPE`), or `None` when it has fewer than two.
fn initials(name: &str) -> Option<String> {
    let caps: String = name.chars().filter(char::is_ascii_uppercase).collect();
    (caps.len() >= 2).then_some(caps)
}

/// The syntax facts' cost over every file of `files` (each with its sites' ranges): the parse
/// alone, the parse and one site's walk (a definition, design (a)), and every site's walk beyond
/// the parse (design (b)'s addition to the index).
fn syntax_facts(checkout: &Path, files: &[(RelPath, Vec<ByteRange>)]) -> Result<(), String> {
    let indexer = JavaIndexer::new(
        Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?,
        (),
    );
    let mut parse = Vec::new();
    let mut one = Vec::new();
    let mut walks = Duration::ZERO;
    let mut all_parses = Duration::ZERO;
    let mut largest: (usize, Duration, Duration) = (0, Duration::ZERO, Duration::ZERO);
    let nowhere =
        ByteRange::new(ByteOffset::new(0), ByteOffset::new(0)).map_err(|e| e.to_string())?;
    for (path, ranges) in files {
        let Some(&first) = ranges.first() else {
            continue;
        };
        let bytes = std::fs::read(checkout.join(path.as_str())).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(jfl_application::syntax_indexer::strip_bom(&bytes))
            .into_owned();
        let started = Instant::now();
        let _ = indexer.site_syntax_blocking(&text, nowhere);
        let parsed = started.elapsed();
        let started = Instant::now();
        let _ = indexer.site_syntax_blocking(&text, first);
        let single = started.elapsed();
        let started = Instant::now();
        let _ = indexer.sites_syntax_blocking(&text, ranges);
        let every = started.elapsed();
        parse.push(parsed);
        one.push(single);
        all_parses = all_parses.saturating_add(parsed);
        walks = walks.saturating_add(every.saturating_sub(parsed));
        if text.len() > largest.0 {
            largest = (text.len(), parsed, single);
        }
    }
    println!("  {}", line("syntax facts: parse of the file", &parse));
    println!(
        "  {}",
        line(
            "syntax facts: parse and one site's walk (each definition)",
            &one
        )
    );
    println!(
        "  syntax facts: the largest file ({} bytes): parse {:.2} ms, with one site {:.2} ms",
        largest.0,
        ms(largest.1),
        ms(largest.2)
    );
    println!(
        "  syntax facts kept at index time: every site's walk {:.0} ms over {} files, beside their parses' {:.0} ms (one thread)",
        ms(walks),
        parse.len(),
        ms(all_parses)
    );
    Ok(())
}

/// The definition at `offset` of `path`: the request (the reconcile and publish of what changed
/// before it) and the query, and the query alone.
fn timed_definition(
    service: &mut Service,
    root: &FileUri,
    checkout: &Path,
    (path, offset): (&RelPath, u32),
    cache: &QueryCache,
) -> Result<Option<(Duration, Duration)>, String> {
    let config = QueryConfig::default();
    let encoding = PositionEncoding::Utf16;
    let uri = root.join(path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(checkout.join(path.as_str())).map_err(|e| e.to_string())?;
    let text =
        String::from_utf8_lossy(jfl_application::syntax_indexer::strip_bom(&bytes)).into_owned();
    let lines = LineIndex::new(&text).map_err(|e| e.to_string())?;
    let Ok(position) = lines.position(ByteOffset::new(offset), encoding) else {
        return Ok(None);
    };
    let whole = Instant::now();
    let answer: Answer = block_on(service.request(&uri)).map_err(|e| e.to_string())?;
    let pinned = Pinned {
        answer: &answer,
        head: None,
        freshness: freshness(),
    };
    let alone = Instant::now();
    let deadline = ClockDeadline::new(&SystemClock, config.definition_budget);
    let _ = block_on(definition(
        &pinned,
        &service.sources(),
        position,
        encoding,
        &config,
        &deadline,
        cache,
    ));
    let times = (whole.elapsed(), alone.elapsed());
    service.finish(answer.id).map_err(|e| e.to_string())?;
    Ok(Some(times))
}

/// The first query after an edit (IDX-18, plan §1.7's freshness and `jfl def` rows), on a
/// scratch repository of the checkout's `.java` files made beside `store` (never the pinned
/// corpus, which other runs read) and removed after: its view opened and settled on a service of
/// its own, then a file rewritten with a comment appended (its key changes), then a definition
/// at a picked site, whose request reconciles and publishes the edit and whose query makes the
/// new generation's name tables; then the file put back, and the same again. [`EDIT_ROUNDS`]
/// files, each rewritten and put back once. A run cut short leaves only the scratch directory,
/// which the next run removes.
fn edits_then_queries(
    checkout: &Path,
    files: &[RelPath],
    store: &Path,
    pool: &Pool,
    chosen: &[(RelPath, u32)],
) -> Result<(), String> {
    let scratch = store.with_extension("edits");
    let _ = std::fs::remove_dir_all(&scratch);
    let made = Instant::now();
    let mut copied = Vec::with_capacity(files.len());
    for file in files {
        let bytes = std::fs::read(checkout.join(file.as_str())).map_err(|e| e.to_string())?;
        copied.push((file.as_str().to_owned(), bytes));
    }
    let borrowed: Vec<(&str, &[u8])> = copied
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let repo = jfl_testkit::fixtures::git::Repo::init(&scratch.join("main"), &borrowed)?;
    drop(copied);
    let made = made.elapsed();
    let copy = repo.root().to_path_buf();
    let root = FileUri::from_path(copy.to_str().ok_or("scratch path is not UTF-8")?)
        .map_err(|e| e.to_string())?;
    let opened = Instant::now();
    let mut service = service(
        &scratch.join("store"),
        pool,
        ServiceConfig::new(&format!("edits{}", std::process::id())),
    )?;
    let first = root
        .join(files.first().ok_or("no .java file")?)
        .map_err(|e| e.to_string())?;
    let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
    service.finish(answer.id).map_err(|e| e.to_string())?;
    while block_on(service.settle(usize::MAX)).map_err(|e| e.to_string())? {}
    let opened = opened.elapsed();
    println!(
        "  edits: on a scratch repository of the {} files, made in {:.1} s, its view settled in {:.1} s",
        files.len(),
        made.as_secs_f64(),
        opened.as_secs_f64()
    );
    let cache = QueryCache::new();
    let mut sites = chosen.iter().cycle().skip(chosen.len() / 2);
    // The tables of the settled generation, made before the first edit (not timed).
    let (site, offset) = sites.next().ok_or("no site")?;
    let _ = timed_definition(&mut service, &root, &copy, (site, *offset), &cache)?;
    let (builds, updates) = (cache.builds(), cache.updates());
    let mut whole = Vec::new();
    let mut alone = Vec::new();
    let mut edited: Vec<&RelPath> = chosen.iter().map(|(p, _)| p).collect();
    edited.sort_unstable();
    edited.dedup();
    for (round, path) in edited.into_iter().take(EDIT_ROUNDS).enumerate() {
        let file = copy.join(path.as_str());
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let mut changed = bytes.clone();
        changed.extend_from_slice(format!("\n// jfl-bench edit {round}\n").as_bytes());
        for content in [changed.as_slice(), bytes.as_slice()] {
            std::fs::write(&file, content).map_err(|e| e.to_string())?;
            let (site, offset) = sites.next().ok_or("no site")?;
            if let Some((w, a)) =
                timed_definition(&mut service, &root, &copy, (site, *offset), &cache)?
            {
                whole.push(w);
                alone.push(a);
            }
        }
    }
    drop(service);
    let _ = std::fs::remove_dir_all(&scratch);
    println!(
        "  {}",
        line(
            "after an edit: request (reconcile, publish) and definition",
            &whole
        )
    );
    let made = cache.builds().saturating_sub(builds);
    let updated = cache.updates().saturating_sub(updates);
    println!(
        "  {}; name tables made {made} times: {updated} from the previous generation's, {} afresh",
        line(
            "after an edit: definition alone (the new generation's name tables)",
            &alone
        ),
        made.saturating_sub(updated)
    );
    Ok(())
}

/// Definitions repeated in one file, as an agent or an editor asks them: [`REPEAT_FILES`] files
/// with at least [`REPEAT_SITES`] reference sites, picked by seed, and that many sites of each in
/// a row, twice (the query alone). The first pass parses the file at its first site that reads
/// syntax facts, as before IDX-18, and walks a kept tree at the rest; the second pass finds every
/// tree kept (without the cache both passes parse at every such site). What a cache of parsed
/// trees serves, where the random sites above rarely ask one file twice.
fn repeated_files(
    service: &mut Service,
    root: &FileUri,
    checkout: &Path,
    by_file: &[(RelPath, Vec<ByteRange>)],
    cache: &QueryCache,
) -> Result<(), String> {
    let candidates: Vec<&(RelPath, Vec<ByteRange>)> = by_file
        .iter()
        .filter(|(_, sites)| sites.len() >= REPEAT_SITES)
        .collect();
    let mut picker = Picker(0x2545_F491_4F6C_DD1D);
    let mut passes = [Vec::new(), Vec::new()];
    for _ in 0..REPEAT_FILES {
        let Some((path, sites)) = candidates.get(picker.below(candidates.len())) else {
            break;
        };
        let step = sites.len() / REPEAT_SITES;
        for pass in &mut passes {
            for range in sites.iter().step_by(step.max(1)).take(REPEAT_SITES) {
                let at = (path, range.start().get());
                if let Some((_, alone)) = timed_definition(service, root, checkout, at, cache)? {
                    pass.push(alone);
                }
            }
        }
    }
    let [first, again] = passes;
    println!(
        "  {}",
        line(
            "same-file definitions: first pass over a file's sites, query alone",
            &first
        )
    );
    println!(
        "  {}",
        line(
            "same-file definitions: the same sites again, query alone",
            &again
        )
    );
    Ok(())
}

/// The process's peak and current resident set (`/proc/self/status`).
fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The process's resident set in KiB (`VmRSS`).
fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// `--memory` (IDX-18; a fresh process, nothing else measured): what the name tables of the
/// settled view and a full cache of parsed trees add to the resident set. The trees are the
/// view's largest files with sites, up to the cache's byte bound: the worst case it can hold.
fn memory(
    service: &mut Service,
    first: &FileUri,
    checkout: &Path,
    by_file: &[(RelPath, Vec<ByteRange>)],
) -> Result<(), String> {
    let settled = rss_kib();
    let answer = block_on(service.request(first)).map_err(|e| e.to_string())?;
    let pinned = Pinned {
        answer: &answer,
        head: None,
        freshness: freshness(),
    };
    let config = QueryConfig::default();
    let cache = QueryCache::new();
    let deadline = ClockDeadline::new(&SystemClock, config.symbols_budget);
    let _ = block_on(workspace_symbols(
        &pinned,
        &service.sources(),
        SymbolQuery {
            text: "",
            filter: None,
            limit: Some(1),
        },
        PositionEncoding::Utf16,
        &config,
        &deadline,
        &cache,
    ));
    let tables = rss_kib();
    let (cache_files, cache_bytes) = TREE_CACHE;
    let cache_bytes = u64::try_from(cache_bytes).unwrap_or(u64::MAX);
    let mut sized: Vec<(u64, &RelPath, ByteRange)> = by_file
        .iter()
        .filter_map(|(path, sites)| {
            let size = std::fs::metadata(checkout.join(path.as_str())).ok()?.len();
            Some((size, path, *sites.first()?))
        })
        .filter(|(size, _, _)| *size <= cache_bytes)
        .collect();
    sized.sort_unstable_by_key(|s| std::cmp::Reverse(s.0));
    let mut held = 0_u64;
    let mut trees = 0_usize;
    let sources = service.sources();
    for (size, path, site) in sized {
        if trees == cache_files || held.saturating_add(size) > cache_bytes {
            continue;
        }
        let bytes = std::fs::read(checkout.join(path.as_str())).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(jfl_application::syntax_indexer::strip_bom(&bytes))
            .into_owned();
        let Some(text) = jfl_application::FileText::new(jfl_domain::Language::Java, text) else {
            continue;
        };
        let _ = block_on(jfl_application::Sources::site_syntax(
            &sources,
            &Arc::new(text),
            site,
        ));
        held = held.saturating_add(size);
        trees = trees.saturating_add(1);
    }
    let full = rss_kib();
    service.finish(answer.id).map_err(|e| e.to_string())?;
    println!(
        "  memory: settled view {settled} KiB; the name tables add {} KiB; {trees} trees of the largest files ({} KiB of text) add {} KiB",
        tables.saturating_sub(settled),
        held / 1024,
        full.saturating_sub(tables)
    );
    Ok(())
}

/// The indexer's bounds on the parsed trees it keeps: files, bytes of text.
const TREE_CACHE: (usize, usize) = (
    jfl_adapter_java::TREE_CACHE_FILES,
    jfl_adapter_java::TREE_CACHE_BYTES,
);
/// Files rewritten and put back by [`edits_then_queries`].
const EDIT_ROUNDS: usize = 10;
/// Files of [`repeated_files`].
const REPEAT_FILES: usize = 20;
/// Definitions asked in each of them.
const REPEAT_SITES: usize = 10;

fn main() -> Result<(), String> {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(input).map_err(|e| e.to_string())?;
    let (checkout, listing) = text.split_once('\n').ok_or("no checkout path on stdin")?;
    let files: Vec<RelPath> = listing
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter_map(|p| RelPath::new(p).ok())
        .collect();
    let root = FileUri::from_path(checkout).map_err(|e| e.to_string())?;
    let first = root
        .join(files.first().ok_or("no .java file")?)
        .map_err(|e| e.to_string())?;
    let store = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("bench-queries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&store);
    println!("bench-queries: {checkout}");
    let pool = Pool::new(4)?;
    let mut service = service(
        &store,
        &pool,
        ServiceConfig::new(&format!("bench{}", std::process::id())),
    )?;
    let started = Instant::now();
    let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
    service.finish(answer.id).map_err(|e| e.to_string())?;
    while block_on(service.settle(usize::MAX)).map_err(|e| e.to_string())? {}
    let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
    println!(
        "  view opened and settled in {:.0} ms: {} files, complete: {}",
        ms(started.elapsed()),
        answer.generation.index.len(),
        answer.completeness.is_complete()
    );
    println!("  memory after the open: {}", rss());

    // The sites and declaration names to query, picked by seed from the generation.
    let mut sites: Vec<(RelPath, u32)> = Vec::new();
    let mut by_file: Vec<(RelPath, Vec<ByteRange>)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    // Methods by name, and how many reference sites of the view use each name.
    let mut methods: Vec<(RelPath, u32, String)> = Vec::new();
    let mut uses: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (path, entry) in &answer.generation.index {
        if let Some(Summary::Indexed { summary, .. }) = &entry.summary {
            for r in summary.references() {
                let count = uses.entry(summary.str(r.name).to_owned()).or_default();
                *count = count.saturating_add(1);
            }
            methods.extend(
                summary
                    .declarations()
                    .filter(|(_, d)| d.kind == jfl_domain::DeclKind::Method)
                    .map(|(_, d)| {
                        (
                            path.clone(),
                            d.name_range.start().get(),
                            summary.str(d.name).to_owned(),
                        )
                    }),
            );
            sites.extend(
                summary
                    .references()
                    .iter()
                    .map(|r| (path.clone(), r.range.start().get())),
            );
            by_file.push((
                path.clone(),
                summary.references().iter().map(|r| r.range).collect(),
            ));
            names.extend(
                summary
                    .declarations()
                    .filter(|(_, d)| d.kind.is_type())
                    .map(|(_, d)| summary.str(d.name).to_owned()),
            );
        }
    }
    service.finish(answer.id).map_err(|e| e.to_string())?;
    if std::env::args().any(|a| a == "--memory") {
        return memory(&mut service, &first, Path::new(checkout), &by_file);
    }
    let picks = arg_after("--picks").unwrap_or(PICKS);
    let mut picker = Picker(0x9E37_79B9_7F4A_7C15);
    let chosen: Vec<(RelPath, u32)> = (0..picks)
        .filter_map(|_| sites.get(picker.below(sites.len())).cloned())
        .collect();
    let config = QueryConfig::default();
    let clock = SystemClock;
    let encoding = PositionEncoding::Utf16;
    let cache = QueryCache::new();

    // The first query of a generation builds its name tables (`QueryCache`): timed alone.
    {
        let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
        let pinned = Pinned {
            answer: &answer,
            head: None,
            freshness: freshness(),
        };
        let started = Instant::now();
        let deadline = ClockDeadline::new(&clock, config.symbols_budget);
        let _ = block_on(workspace_symbols(
            &pinned,
            &service.sources(),
            SymbolQuery {
                text: "",
                filter: None,
                limit: Some(1),
            },
            encoding,
            &config,
            &deadline,
            &cache,
        ));
        println!(
            "  first query of the generation (builds the name tables): {:.1} ms",
            ms(started.elapsed())
        );
        service.finish(answer.id).map_err(|e| e.to_string())?;
    }
    let mut def_alone = Vec::new();
    let mut def_whole = Vec::new();
    let mut hover_alone = Vec::new();
    let mut tally: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (path, offset) in &chosen {
        let uri = root.join(path).map_err(|e| e.to_string())?;
        let bytes =
            std::fs::read(Path::new(checkout).join(path.as_str())).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(jfl_application::syntax_indexer::strip_bom(&bytes))
            .into_owned();
        let lines = LineIndex::new(&text).map_err(|e| e.to_string())?;
        let Ok(position) = lines.position(ByteOffset::new(*offset), encoding) else {
            continue;
        };
        let whole = Instant::now();
        let answer: Answer = block_on(service.request(&uri)).map_err(|e| e.to_string())?;
        let pinned = Pinned {
            answer: &answer,
            head: None,
            freshness: freshness(),
        };
        let sources = service.sources();
        let alone = Instant::now();
        let deadline = ClockDeadline::new(&clock, config.definition_budget);
        let envelope = block_on(definition(
            &pinned, &sources, position, encoding, &config, &deadline, &cache,
        ));
        def_alone.push(alone.elapsed());
        def_whole.push(whole.elapsed());
        let label = match &envelope {
            Ok(envelope) => kind(envelope),
            Err(QueryError::NoSymbol) => "no symbol",
            Err(_) => "refused",
        };
        let count = tally.entry(label).or_default();
        *count = count.saturating_add(1);
        let alone = Instant::now();
        let deadline = ClockDeadline::new(&clock, config.definition_budget);
        let _ = block_on(hover(
            &pinned, &sources, position, encoding, &config, &deadline, &cache,
        ));
        hover_alone.push(alone.elapsed());
        service.finish(answer.id).map_err(|e| e.to_string())?;
    }
    println!("  {}", line("definition, query alone", &def_alone));
    println!("  {}", line("definition, request and query", &def_whole));
    println!("  {}", line("hover, query alone", &hover_alone));
    println!(
        "  definition answers: {tally:?}; name tables built {} times",
        cache.builds()
    );
    edits_then_queries(Path::new(checkout), &files, &store, &pool, &chosen)?;
    repeated_files(&mut service, &root, Path::new(checkout), &by_file, &cache)?;
    syntax_facts(Path::new(checkout), &by_file)?;

    let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
    let pinned = Pinned {
        answer: &answer,
        head: None,
        freshness: freshness(),
    };
    let sources = service.sources();
    let picked: Vec<String> = (0..SYMBOL_QUERIES)
        .filter_map(|_| names.get(picker.below(names.len())).cloned())
        .collect();
    let prefixes: Vec<String> = picked.iter().map(|n| n.chars().take(3).collect()).collect();
    let kinds: [(&str, Vec<String>, Option<usize>); 5] = [
        ("prefix (3 letters)", prefixes.clone(), None),
        (
            "prefix (3 letters), limit 1: the scan and one read",
            prefixes,
            Some(1),
        ),
        ("exact name", picked.clone(), None),
        (
            "hump initials",
            picked.iter().filter_map(|n| initials(n)).collect(),
            None,
        ),
        (
            "one letter",
            picked.iter().map(|n| n.chars().take(1).collect()).collect(),
            None,
        ),
    ];
    for (label, queries, limit) in kinds {
        let mut times = Vec::new();
        let mut capped = 0_usize;
        for text in &queries {
            let alone = Instant::now();
            let deadline = ClockDeadline::new(&clock, config.symbols_budget);
            let symbols = block_on(workspace_symbols(
                &pinned,
                &sources,
                SymbolQuery {
                    text,
                    filter: None,
                    limit,
                },
                encoding,
                &config,
                &deadline,
                &cache,
            ));
            times.push(alone.elapsed());
            capped = capped.saturating_add(usize::from(!symbols.completeness.is_complete()));
        }
        println!(
            "  {}; {capped} incomplete",
            line(&format!("workspace symbols, {label}"), &times)
        );
    }
    service.finish(answer.id).map_err(|e| e.to_string())?;

    // References (IDX-11): methods of widely used names and of rare ones, by how many sites of
    // the view use the name, asked at the declaration; hover there counts them too.
    let sites_of = |name: &str| uses.get(name).copied().unwrap_or(0);
    let common: Vec<&(RelPath, u32, String)> = methods
        .iter()
        .filter(|m| sites_of(&m.2) >= COMMON_SITES)
        .collect();
    let rare: Vec<&(RelPath, u32, String)> = methods
        .iter()
        .filter(|m| (1..=RARE_SITES).contains(&sites_of(&m.2)))
        .collect();
    for (label, pool) in [("common names", &common), ("rare names", &rare)] {
        let mut alone = Vec::new();
        let mut hovers = Vec::new();
        let (mut exact, mut possible, mut complete, mut asked) =
            (0_usize, 0_usize, 0_usize, 0_usize);
        let mut widest = 0_usize;
        let mut reasons: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for _ in 0..REFS_QUERIES {
            let Some((path, offset, name)) = pool.get(picker.below(pool.len())).copied() else {
                break;
            };
            let uri = root.join(path).map_err(|e| e.to_string())?;
            let bytes = std::fs::read(Path::new(checkout).join(path.as_str()))
                .map_err(|e| e.to_string())?;
            let text = String::from_utf8_lossy(jfl_application::syntax_indexer::strip_bom(&bytes))
                .into_owned();
            let lines = LineIndex::new(&text).map_err(|e| e.to_string())?;
            let Ok(position) = lines.position(ByteOffset::new(*offset), encoding) else {
                continue;
            };
            let answer: Answer = block_on(service.request(&uri)).map_err(|e| e.to_string())?;
            let pinned = Pinned {
                answer: &answer,
                head: None,
                freshness: freshness(),
            };
            let sources = service.sources();
            let started = Instant::now();
            let deadline = ClockDeadline::new(&clock, config.references_budget);
            let envelope = block_on(references(
                &pinned,
                &sources,
                ReferencesQuery {
                    position,
                    ..ReferencesQuery::default()
                },
                encoding,
                &config,
                &deadline,
                &cache,
            ));
            alone.push(started.elapsed());
            if let Ok(envelope) = &envelope {
                asked = asked.saturating_add(1);
                exact = exact.saturating_add(envelope.exact().len());
                possible = possible.saturating_add(envelope.possible().len());
                complete = complete.saturating_add(usize::from(envelope.complete()));
                widest = widest.max(sites_of(name));
                for reason in envelope.incomplete_because() {
                    let text = reason.to_string();
                    // The words before a count: one key per kind of reason.
                    let key = text.split(" (").next().unwrap_or(&text).to_owned();
                    let n = reasons.entry(key).or_default();
                    *n = n.saturating_add(1);
                }
            }
            let started = Instant::now();
            let deadline = ClockDeadline::new(&clock, config.definition_budget);
            let _ = block_on(hover(
                &pinned, &sources, position, encoding, &config, &deadline, &cache,
            ));
            hovers.push(started.elapsed());
            service.finish(answer.id).map_err(|e| e.to_string())?;
        }
        println!(
            "  {}; {asked} answered: {exact} exact, {possible} possible, {complete} complete; widest name {widest} sites; {} methods to pick from",
            line(&format!("references, {label}, query alone"), &alone),
            pool.len()
        );
        println!(
            "  {}",
            line(
                &format!("hover on a declaration, {label} (counts its references)"),
                &hovers
            )
        );
        println!("  references, {label}, why incomplete: {reasons:?}");
    }
    println!("  memory at the end: {}", rss());
    drop(service);
    let _ = std::fs::remove_dir_all(&store);
    Ok(())
}

/// Reference queries of each kind.
const REFS_QUERIES: usize = 50;
/// A method name used at this many sites of the view or more is a common one.
const COMMON_SITES: usize = 200;
/// One used at this many or fewer is a rare one.
const RARE_SITES: usize = 10;
