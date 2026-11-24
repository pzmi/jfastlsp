//! The view service on the pinned corpus (IDX-09, ADR 0031): the second half of
//! `just bench-views SIZE`.
//!
//! Reads the checkout's path (stdin: the path, a newline, then the tracked `.java` files) and
//! drives `jfl-application`'s view service over the real adapters: the fs tree (inotify), the git
//! probe, the Java indexer and the on-disk summary store in a scratch directory, every port bound
//! to one pool of four threads (what composition's bounded blocking pool does; the bench has no
//! runtime). `--parallelism N` and `--open-whole N` override `ServiceConfig`'s, `--pool N` the
//! pool's threads.
//!
//! - **Stages:** each step of a cold open alone over the adapters' blocking calls, on 1 and 4
//!   threads (the listing, stat, read, hash, parse, encode, store write, store miss) and one
//!   hand-off to the pool and back: where the open's time goes.
//! - **View open, empty store, warm page cache:** a fresh service with an empty store, after the
//!   stages have read every file. The first request's answer (the
//!   view opened with only its file's directory read: plan §1.7's first useful answer), then
//!   `settle` in the daemon's slices (`--settle-slice N` overrides `SETTLE_SLICE`; every other
//!   file read and published a slice at a time: the full syntax index, which leaves out a request
//!   on the first file asked between each two slices), then `write_back` in the daemon's slices
//!   (the store written, after the publish). Each slice's time is what the daemon's loop is held
//!   for (IDX-09d); the request between slices is what a client waits beyond the slice it arrived
//!   in. One run. With the argument `cold` (`just
//!   bench-views SIZE cold`) only this, three times, each after a page-cache drop, as plan §1.7's
//!   cold rows mean it (Linux, root).
//! - **View open, warm:** the same with the store filled (a daemon restart). Three runs.
//! - **Cookie round trip:** a request on an unchanged file writes its cookie into the checkout's
//!   root and waits for the watcher to deliver it (plan §1.7's 100 ms freshness target); p50 and
//!   p95 over the requests, with the whole request's time beside it.
//! - **Rescan with listing:** `sync`: the listing unioned with `git ls-files`, every indexed file
//!   re-statted, nothing changed, published. Median of five.
//! - **Rescans during a settle** (IDX-09e): the checkout's `.java` files copied into a scratch
//!   repository with a linked agent worktree (`.claude/worktrees/bench`, the target layout, whose
//!   `HEAD` lives outside its root), opened there with an empty store, one slice settled, then a
//!   rescan triggered three ways, each on a fresh service: a commit in the worktree, which a
//!   request's stat sees and runs the rescan inline; a hint naming no `.java` file (a build file
//!   written, which the shell takes as it takes a `.git` change), which the next request or
//!   settle step runs; a `.java` file created and asked for, which the request escalates to a
//!   rescan. For each: the trigger's request, the longest step (what the
//!   loop is held for), the settle's steps, and the full index without the requests.
//!
//! The cookie's deadline here is a future that polls the clock: the bench has no runtime timer.
//! Here, not in `jfl-application`, because timing needs the clock grant only composition holds
//! (fnd §2.4). Reports, never gates. Writes nothing into the checkout but the cookie files, each
//! removed at once.

use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_adapter_store::{Blake3, DiskSummaryStore, codec};
use jfl_application::source_tree::{ListMode, MTime};
use jfl_application::syntax_indexer::MAX_SOURCE_BYTES;
use jfl_application::{
    BlockingPool, CountingClock, Interrupted, ServiceConfig, Summary, SummaryStore as _,
    ViewService, WallClock,
};
use jfl_composition::daemon::views::{SETTLE_SLICE, WRITE_SLICE};
use jfl_domain::{FileSummary, FileUri, RelPath, SyntaxInputs, SyntaxKey};

/// Requests timed for the cookie round trip.
const REQUESTS: usize = 200;
/// Opens timed after a page-cache drop (`cold`); the median is reported.
const COLD_RUNS: usize = 3;

/// Drops the page cache, dentries and inodes (Linux, root), as E1's cold runs did (plan §1.7):
/// the next open reads every file from the disk. On this kind of VM the host's own cache may
/// still serve the guest's reads.
fn drop_caches() -> bool {
    std::fs::write("/proc/sys/vm/drop_caches", b"3\n").is_ok()
}

/// The blocking pool's threads: E1's four cores (plan §1.7).
const POOL_THREADS: usize = 4;
/// Store writes per [`ViewService::write_back`] call: the daemon's slice between requests.
const WRITE_BACK_BUDGET: usize = WRITE_SLICE;

/// The value after `flag` on the command line.
fn arg_after(flag: &str) -> Option<usize> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == flag)?;
    args.get(at.checked_add(1)?)?.parse().ok()
}

/// One view open: the first answer, then the rest of the view, then the store's writes.
struct Opened {
    service: Service,
    first_answer: Duration,
    first_files: usize,
    first_indexed: usize,
    first_complete: bool,
    settled: Duration,
    written: Duration,
    files: usize,
    complete: bool,
    /// Each settle slice's time: what the daemon's loop is held for, one step at a time.
    slices: Vec<Duration>,
    /// Each write-back slice's time.
    writes: Vec<Duration>,
    /// A request on the first file asked after each slice but the last: its whole time.
    between: Vec<Duration>,
    /// How many of those answers were incomplete (the view still settling).
    between_incomplete: usize,
}

impl Opened {
    fn line(&self) -> String {
        format!(
            "first answer {:.0} ms ({} of {} files indexed, complete: {}); whole view published {:.0} ms ({} files, then complete: {}); store written {:.0} ms",
            ms(self.first_answer),
            self.first_indexed,
            self.first_files,
            self.first_complete,
            ms(self.settled),
            self.files,
            self.complete,
            ms(self.written)
        )
    }

    /// The loop's holds and the requests between the slices (IDX-09d).
    fn holds(&self) -> String {
        let max = |v: &[Duration]| v.iter().max().copied().unwrap_or_default();
        format!(
            "settle in {} slices: each p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms; write-back in {} slices: p95 {:.0} ms, max {:.0} ms; a request between slices: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms ({} of {} incomplete)",
            self.slices.len(),
            ms(percentile(self.slices.clone(), 50)),
            ms(percentile(self.slices.clone(), 95)),
            ms(max(&self.slices)),
            self.writes.len(),
            ms(percentile(self.writes.clone(), 95)),
            ms(max(&self.writes)),
            ms(percentile(self.between.clone(), 50)),
            ms(percentile(self.between.clone(), 95)),
            ms(max(&self.between)),
            self.between_incomplete,
            self.between.len()
        )
    }
}

fn open(
    store: &Path,
    pool: &Pool,
    config: &ServiceConfig,
    first: &FileUri,
    slice: usize,
) -> Result<Opened, String> {
    let started = Instant::now();
    let mut service = service(store, pool, config.clone())?;
    let answer = block_on(service.request(first)).map_err(|e| e.to_string())?;
    let first_answer = started.elapsed();
    let first_files = answer.generation.index.len();
    let first_indexed = answer
        .generation
        .index
        .values()
        .filter(|e| matches!(e.summary, Some(Summary::Indexed { .. })))
        .count();
    let first_complete = answer.completeness.is_complete();
    service.finish(answer.id).map_err(|e| e.to_string())?;
    let mut slices = Vec::new();
    let mut between = Vec::new();
    let mut between_incomplete = 0_usize;
    let mut asking = Duration::ZERO;
    loop {
        let step = Instant::now();
        let more = block_on(service.settle(slice)).map_err(|e| e.to_string())?;
        slices.push(step.elapsed());
        if !more {
            break;
        }
        let asked = Instant::now();
        let answer = block_on(service.request(first)).map_err(|e| e.to_string())?;
        between_incomplete =
            between_incomplete.saturating_add(usize::from(!answer.completeness.is_complete()));
        service.finish(answer.id).map_err(|e| e.to_string())?;
        let took = asked.elapsed();
        between.push(took);
        asking = asking.saturating_add(took);
    }
    // The full index's time leaves out the requests asked between the slices.
    let settled = started.elapsed().saturating_sub(asking);
    let mut writes = Vec::new();
    loop {
        let step = Instant::now();
        let waiting = block_on(service.write_back(WRITE_BACK_BUDGET));
        writes.push(step.elapsed());
        if waiting == 0 {
            break;
        }
    }
    let written = started.elapsed().saturating_sub(asking);
    let answer = block_on(service.request(first)).map_err(|e| e.to_string())?;
    let files = answer.generation.index.len();
    let complete = answer.completeness.is_complete();
    service.finish(answer.id).map_err(|e| e.to_string())?;
    Ok(Opened {
        service,
        first_answer,
        first_files,
        first_indexed,
        first_complete,
        settled,
        written,
        files,
        complete,
        slices,
        writes,
        between,
        between_incomplete,
    })
}

/// Runs each job on the calling thread.
#[derive(Debug, Clone, Copy)]
struct Inline;

impl BlockingPool for Inline {
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        std::future::ready(Ok(job()))
    }
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

/// Runs `work(i)` for every `i` below `n` on `threads` threads, each taking the next index:
/// the wall time.
fn on_threads(threads: usize, n: usize, work: impl Fn(usize) + Sync) -> Duration {
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    if at >= n {
                        break;
                    }
                    work(at);
                }
            });
        }
    });
    started.elapsed()
}

/// A key no file of the corpus has: the store stages write under these.
fn synthetic_key(n: usize) -> SyntaxKey {
    SyntaxKey::derive(
        &SyntaxInputs {
            file: Blake3::digest(&n.to_le_bytes()),
            grammar: "bench",
            grammar_version: "0",
            summariser_version: 0,
        },
        Blake3::new(),
    )
}

/// Each stage of a cold open alone, over the real adapters' blocking calls, on 1 and
/// [`POOL_THREADS`] threads: where the open's time goes. The store stages write the corpus's
/// summaries under synthetic keys into a fresh store, in the files' order and in key order (the
/// order [`ViewService::write_back`] writes in), and look up keys it does not hold.
fn stages(root: &FileUri, files: &[RelPath], scratch: &Path) -> Result<(), String> {
    let tree = FsSourceTree::new(Inline, FsConfig::default());
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let indexer = JavaIndexer::new(grammar, Inline);
    let started = Instant::now();
    let listing = tree
        .list_blocking(root, ListMode::PathsAndStat)
        .map_err(|e| e.to_string())?;
    println!(
        "  stages: the listing with stats {:.0} ms ({} files)",
        ms(started.elapsed()),
        listing.files.len()
    );
    let n = files.len();
    let bytes: Vec<Mutex<Vec<u8>>> = files.iter().map(|_| Mutex::default()).collect();
    let summaries: Vec<Mutex<Option<Arc<FileSummary>>>> =
        files.iter().map(|_| Mutex::default()).collect();
    let summary = |i: usize| {
        summaries
            .get(i)
            .and_then(|s| s.lock().unwrap_or_else(PoisonError::into_inner).clone())
    };
    for threads in [1, POOL_THREADS] {
        let stat = on_threads(threads, n, |i| {
            if let Some(f) = files.get(i) {
                let _ = tree.stat_blocking(root, f);
            }
        });
        let read = on_threads(threads, n, |i| {
            if let (Some(f), Some(slot)) = (files.get(i), bytes.get(i))
                && let Ok(read) = tree.read_blocking(root, f, MAX_SOURCE_BYTES)
            {
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = read.bytes;
            }
        });
        let hash = on_threads(threads, n, |i| {
            if let Some(slot) = bytes.get(i) {
                let b = slot.lock().unwrap_or_else(PoisonError::into_inner);
                std::hint::black_box(Blake3::digest(&b));
            }
        });
        let parse = on_threads(threads, n, |i| {
            if let (Some(slot), Some(out)) = (bytes.get(i), summaries.get(i)) {
                let b = slot.lock().unwrap_or_else(PoisonError::into_inner).clone();
                if let Ok(indexed) = indexer.index_blocking(&b) {
                    *out.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(Arc::new(indexed.summary));
                }
            }
        });
        let encode = on_threads(threads, n, |i| {
            if let Some(s) = summary(i) {
                std::hint::black_box(codec::encode(&synthetic_key(i), &s).ok());
            }
        });
        let dir = scratch.join(format!("stages-{threads}"));
        let _ = std::fs::remove_dir_all(&dir);
        let (store, _) =
            DiskSummaryStore::open(&dir, u64::MAX, Arc::new(CountingClock::default()), Inline)
                .map_err(|e| e.to_string())?;
        let write = on_threads(threads, n, |i| {
            if let Some(s) = summary(i) {
                let _ = store.put_blocking(synthetic_key(i), &s);
            }
        });
        let mut sorted: Vec<(SyntaxKey, usize)> = (0..n)
            .map(|i| (synthetic_key(i.saturating_add(n)), i))
            .collect();
        sorted.sort_unstable();
        let write_sorted = on_threads(threads, n, |at| {
            if let Some(&(key, i)) = sorted.get(at)
                && let Some(s) = summary(i)
            {
                let _ = store.put_blocking(key, &s);
            }
        });
        let miss = on_threads(threads, n, |i| {
            std::hint::black_box(
                store.get_blocking(&synthetic_key(i.saturating_add(n.saturating_mul(2)))),
            );
        });
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
        println!(
            "  stages, {threads} thread(s): stat {:.0} ms, read {:.0} ms, hash {:.0} ms, parse {:.0} ms, encode {:.0} ms, encode and write {:.0} ms (in key order {:.0} ms), store miss {:.0} ms",
            ms(stat),
            ms(read),
            ms(hash),
            ms(parse),
            ms(encode),
            ms(write),
            ms(write_sorted),
            ms(miss)
        );
    }
    let pool = Pool::new(POOL_THREADS)?;
    let dir = scratch.join("stages-pool");
    let _ = std::fs::remove_dir_all(&dir);
    let (store, _) =
        DiskSummaryStore::open(&dir, u64::MAX, Arc::new(CountingClock::default()), pool)
            .map_err(|e| e.to_string())?;
    let first = summary(0).ok_or("no summary")?;
    let key = synthetic_key(0);
    let _ = block_on(store.put(key, Arc::clone(&first)));
    let rounds: u32 = 1000;
    let started = Instant::now();
    for _ in 0..rounds {
        let _ = block_on(store.put(key, Arc::clone(&first)));
    }
    println!(
        "  stages: one hand-off to the pool and back (a store write of a held key): {:.0} us",
        started.elapsed().as_secs_f64() * 1e6 / f64::from(rounds)
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// What sets the rescan during a settle (IDX-09e).
#[derive(Debug, Clone, Copy)]
enum Trigger {
    /// A commit in the linked worktree: no watcher event; the next request's stat sees it.
    Commit,
    /// A hint naming no `.java` file (a build file written): a settle step or the next request
    /// runs the rescan.
    Hint,
    /// A `.java` file created during the settle and asked for: escalated to a rescan.
    Created,
}

/// One trigger's run: a fresh service over `worktree` with an empty store, one slice settled,
/// the trigger, then the rest of the settle; the line it prints.
fn rescan_during_a_settle(
    worktree: &jfl_testkit::fixtures::git::Repo,
    first: &RelPath,
    store: &Path,
    pool: &Pool,
    config: &ServiceConfig,
    slice: usize,
    trigger: Trigger,
) -> Result<String, String> {
    let _ = std::fs::remove_dir_all(store);
    let root = FileUri::from_path(&worktree.root().to_string_lossy()).map_err(|e| e.to_string())?;
    let file = root.join(first).map_err(|e| e.to_string())?;
    let mut service = service(store, pool, config.clone())?;
    let answer = block_on(service.request(&file)).map_err(|e| e.to_string())?;
    service.finish(answer.id).map_err(|e| e.to_string())?;
    // The files still pending in the published generation and its number (outside the timing):
    // a step that publishes and reads none of them ran a rescan.
    let pending = |service: &Service| {
        service.registry().view(&root).map_or((0, 0), |v| {
            let left = v
                .published
                .index
                .values()
                .filter(|e| matches!(e.summary, Some(Summary::Pending)))
                .count();
            (left, v.published.generation.0)
        })
    };
    let mut rescans = Vec::new();
    let started = Instant::now();
    let mut steps = Vec::new();
    let before = pending(&service);
    let step = Instant::now();
    let mut more = block_on(service.settle(slice)).map_err(|e| e.to_string())?;
    steps.push(step.elapsed());
    let mut after = pending(&service);
    if after.0 == before.0 && after.1 > before.1 {
        rescans.push(format!("step 1 {:.0} ms", ms(step.elapsed())));
    }
    let asked_file = match trigger {
        Trigger::Commit => {
            worktree.git(&["commit", "--quiet", "--allow-empty", "--message", "bench"])?;
            file.clone()
        }
        Trigger::Hint => {
            worktree.write("build.gradle", b"// bench\n")?;
            file.clone()
        }
        Trigger::Created => {
            let dir = first.as_str().rsplit_once('/').map_or("", |(d, _)| d);
            let created = if dir.is_empty() {
                "JflBenchCreated.java".to_owned()
            } else {
                format!("{dir}/JflBenchCreated.java")
            };
            worktree.write(&created, b"class JflBenchCreated {}\n")?;
            root.join(&RelPath::new(&created).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        }
    };
    let asking = Instant::now();
    let answer = block_on(service.request(&asked_file)).map_err(|e| e.to_string())?;
    let request = asking.elapsed();
    let complete = answer.completeness.is_complete();
    service.finish(answer.id).map_err(|e| e.to_string())?;
    let by_request = pending(&service);
    let read_by_request = after.0.saturating_sub(by_request.0);
    after = by_request;
    while more {
        let before = after;
        let step = Instant::now();
        more = block_on(service.settle(slice)).map_err(|e| e.to_string())?;
        let took = step.elapsed();
        steps.push(took);
        after = pending(&service);
        if after.0 == before.0 && after.1 > before.1 {
            rescans.push(format!("step {} {:.0} ms", steps.len(), ms(took)));
        }
    }
    let settled = started.elapsed().saturating_sub(request);
    let answer = block_on(service.request(&file)).map_err(|e| e.to_string())?;
    let settled_complete = answer.completeness.is_complete();
    service.finish(answer.id).map_err(|e| e.to_string())?;
    let longest = steps.iter().copied().max().unwrap_or_default().max(request);
    Ok(format!(
        "{trigger:?}: the trigger's request {:.0} ms (complete: {complete}, {} generations, read {read_by_request} pending files); settle steps that ran a rescan: [{}]; longest hold {:.0} ms; {} settle steps, p50 {:.0} ms, max {:.0} ms; full index {:.0} ms without the request (then complete: {settled_complete})",
        ms(request),
        by_request.1.saturating_sub(before.1),
        rescans.join(", "),
        ms(longest),
        steps.len(),
        ms(percentile(steps.clone(), 50)),
        ms(steps.iter().copied().max().unwrap_or_default()),
        ms(settled)
    ))
}

/// The rescans during a settle (IDX-09e), on a scratch repository of the checkout's `.java`
/// files with a linked agent worktree, made beside `store` and removed after.
fn rescans_during_a_settle(
    checkout: &Path,
    files: &[RelPath],
    store: &Path,
    pool: &Pool,
    config: &ServiceConfig,
    slice: usize,
) -> Result<(), String> {
    let scratch = store.with_extension("rescans");
    let _ = std::fs::remove_dir_all(&scratch);
    let mut copied = Vec::with_capacity(files.len());
    for file in files {
        let bytes = std::fs::read(checkout.join(file.as_str())).map_err(|e| e.to_string())?;
        copied.push((file.as_str().to_owned(), bytes));
    }
    let borrowed: Vec<(&str, &[u8])> = copied
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let made = Instant::now();
    let main = jfl_testkit::fixtures::git::Repo::init(&scratch.join("main"), &borrowed)?;
    let worktree = main.add_agent_worktree("bench")?;
    drop(copied);
    println!(
        "  rescans during a settle, on a linked worktree of the {} files (made in {:.1} s):",
        files.len(),
        made.elapsed().as_secs_f64()
    );
    let first = files.first().ok_or("no .java file")?;
    for trigger in [Trigger::Commit, Trigger::Hint, Trigger::Created] {
        let line = rescan_during_a_settle(
            &worktree,
            first,
            &scratch.join("store"),
            pool,
            config,
            slice,
            trigger,
        )?;
        println!("    {line}");
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

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
    let probe_files: Vec<FileUri> = files
        .iter()
        .step_by((files.len() / REQUESTS).max(1))
        .take(REQUESTS)
        .filter_map(|f| root.join(f).ok())
        .collect();
    let first = probe_files.first().ok_or("no .java file")?.clone();
    let store = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("bench-view-service-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&store);
    println!("bench-views (view service): {checkout}");
    let cold = std::env::args().any(|a| a == "cold");
    if !cold {
        stages(&root, &files, &store)?;
    }

    let threads = arg_after("--pool").unwrap_or(POOL_THREADS);
    let pool = Pool::new(threads)?;
    let mut config = ServiceConfig::new(&format!("bench{}", std::process::id()));
    if let Some(width) = arg_after("--parallelism") {
        config.parallelism = width;
    }
    if let Some(whole) = arg_after("--open-whole") {
        config.open_whole = whole;
    }
    let slice = arg_after("--settle-slice").unwrap_or(SETTLE_SLICE);
    println!(
        "  {threads} pool threads, {} files in flight, views of up to {} files opened whole, settled in slices of {slice}",
        config.parallelism, config.open_whole
    );

    if cold {
        for _ in 0..COLD_RUNS {
            let _ = std::fs::remove_dir_all(&store);
            if !drop_caches() {
                return Err("dropping the page cache needs root on Linux".to_owned());
            }
            let opened = open(&store, &pool, &config, &first, slice)?;
            println!(
                "  view open, empty store, page cache dropped: {}",
                opened.line()
            );
            println!("    {}", opened.holds());
        }
        let _ = std::fs::remove_dir_all(&store);
        return Ok(());
    }
    let opened = open(&store, &pool, &config, &first, slice)?;
    println!(
        "  view open, empty store, warm page cache: {}",
        opened.line()
    );
    println!("    {}", opened.holds());
    let mut warm_runs = Vec::new();
    let mut warm = None;
    for _ in 0..3 {
        let opened = open(&store, &pool, &config, &first, slice)?;
        warm_runs.push(format!("{}\n    {}", opened.line(), opened.holds()));
        warm = Some(opened.service);
    }
    for line in warm_runs {
        println!("  view open, warm store: {line}");
    }
    let mut service = warm.ok_or("no service")?;

    let mut round_trips = Vec::new();
    let mut requests = Vec::new();
    let mut complete = 0_usize;
    for file in &probe_files {
        let started = Instant::now();
        let answer = block_on(service.request(file)).map_err(|e| e.to_string())?;
        requests.push(started.elapsed());
        round_trips.extend(answer.cookie_round_trips.iter().copied());
        complete = complete.saturating_add(usize::from(answer.completeness.is_complete()));
        service.finish(answer.id).map_err(|e| e.to_string())?;
    }
    println!(
        "  cookie round trip: p50 {:.3} ms, p95 {:.3} ms over {} cookies; whole request: p50 {:.3} ms, p95 {:.3} ms; {complete} of {} complete",
        ms(percentile(round_trips.clone(), 50)),
        ms(percentile(round_trips.clone(), 95)),
        round_trips.len(),
        ms(percentile(requests.clone(), 50)),
        ms(percentile(requests, 95)),
        probe_files.len()
    );

    let mut rescans = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        block_on(service.sync(&first)).map_err(|e| e.to_string())?;
        rescans.push(started.elapsed());
    }
    println!(
        "  rescan with listing (sync): median {:.1} ms",
        ms(percentile(rescans, 50))
    );
    drop(service);
    rescans_during_a_settle(Path::new(checkout), &files, &store, &pool, &config, slice)?;
    let _ = std::fs::remove_dir_all(&store);
    Ok(())
}
