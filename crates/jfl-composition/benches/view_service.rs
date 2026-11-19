//! The view service on the pinned corpus (IDX-09, ADR 0031): the second half of
//! `just bench-views SIZE`.
//!
//! Reads the checkout's path (stdin: the path, a newline, then the tracked `.java` files, which
//! this half ignores) and drives `jfl-application`'s view service over the real adapters: the fs
//! tree (inotify), the git probe, the Java indexer and the on-disk summary store in a scratch
//! directory. Reads, stats and the store run on the calling thread; each parse runs on a thread of
//! its own, four at once (`ServiceConfig::parallelism`).
//!
//! - **View open, cold:** the first request with an empty store: filesystem check, watch, the Git
//!   stat set, the listing unioned with `git ls-files`, every file read, parsed and stored, then
//!   the request's own cookie and pin. One run.
//! - **View open, warm:** the same with the store filled (a daemon restart): every file read and
//!   hashed, its summary read back from the store. Median of three.
//! - **Cookie round trip:** a request on an unchanged file writes its cookie into the checkout's
//!   root and waits for the watcher to deliver it (plan §1.7's 100 ms freshness target); p50 and
//!   p95 over the requests, with the whole request's time beside it.
//! - **Rescan with listing:** `sync`: the listing unioned with `git ls-files`, every indexed file
//!   re-statted, nothing changed, published. Median of five.
//!
//! The cookie's deadline here is a future that polls the clock: the bench has no runtime timer.
//! Here, not in `jfl-application`, because timing needs the clock grant only composition holds
//! (fnd §2.4). Reports, never gates. Writes nothing into the checkout but the cookie files, each
//! removed at once.

use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_adapter_store::{Blake3, DiskSummaryStore};
use jfl_application::source_tree::MTime;
use jfl_application::{
    BlockingPool, CountingClock, Interrupted, ServiceConfig, ViewService, WallClock,
};
use jfl_domain::{FileUri, RelPath};

/// Requests timed for the cookie round trip.
const REQUESTS: usize = 200;

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

/// Runs each job on a thread of its own and wakes the caller when it ends: the parse of each
/// file, so the service's `parallelism` files parse at once (composition's runtime binds the
/// port to its bounded blocking pool; this bench has no runtime).
#[derive(Debug, Clone, Copy)]
struct Threads;

type Slot<T> = Arc<Mutex<(Option<T>, Option<Waker>)>>;

impl BlockingPool for Threads {
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let slot: Slot<T> = Arc::new(Mutex::new((None, None)));
        let done = Arc::clone(&slot);
        let spawned = thread::Builder::new().spawn(move || {
            let out = job();
            let mut guard = done.lock().unwrap_or_else(PoisonError::into_inner);
            guard.0 = Some(out);
            if let Some(waker) = guard.1.take() {
                waker.wake();
            }
        });
        let failed = spawned.is_err();
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
    FsSourceTree<Inline>,
    GitRepoProbe<Inline>,
    JavaIndexer<Threads>,
    DiskSummaryStore<Inline>,
    SystemClock,
    Blake3,
>;

fn service(store: &Path) -> Result<Service, String> {
    let grammar = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    let (store, _) =
        DiskSummaryStore::open(store, u64::MAX, Arc::new(CountingClock::default()), Inline)
            .map_err(|e| e.to_string())?;
    Ok(ViewService::new(
        (
            FsSourceTree::new(Inline, FsConfig::default()),
            GitRepoProbe::new(Inline, GitConfig::default()),
            JavaIndexer::new(grammar, Threads),
            store,
        ),
        SystemClock,
        Blake3::default(),
        ServiceConfig::new(&format!("bench{}", std::process::id())),
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

    let started = Instant::now();
    let mut cold = service(&store)?;
    let answer = block_on(cold.request(&first)).map_err(|e| e.to_string())?;
    let open_cold = started.elapsed();
    println!(
        "  view open, cold store: {:.0} ms ({} files; filesystem {:?}; first answer complete: {})",
        ms(open_cold),
        answer.generation.index.len(),
        cold.filesystem(&answer.view),
        answer.completeness.is_complete()
    );
    cold.finish(answer.id).map_err(|e| e.to_string())?;
    drop(cold);

    let mut warm_runs = Vec::new();
    let mut warm = None;
    for _ in 0..3 {
        let started = Instant::now();
        let mut service = service(&store)?;
        let answer = block_on(service.request(&first)).map_err(|e| e.to_string())?;
        warm_runs.push(started.elapsed());
        service.finish(answer.id).map_err(|e| e.to_string())?;
        warm = Some(service);
    }
    println!(
        "  view open, warm store: median {:.0} ms",
        ms(percentile(warm_runs, 50))
    );
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
    let _ = std::fs::remove_dir_all(&store);
    Ok(())
}
