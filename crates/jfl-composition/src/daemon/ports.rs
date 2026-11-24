//! The daemon's bindings of the application's runtime ports (fnd §4.4): the blocking pool every
//! adapter runs its blocking jobs on, and the wall clock with the runtime's timer.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jfl_application::source_tree::MTime;
use jfl_application::{BlockingPool, Interrupted, WallClock};

use super::timer;

/// How many threads the blocking pool may start: the view service keeps its parallelism's worth of
/// steps in flight (16 by default) over four threads, the corpus benchmarks' shape (IDX-09).
pub const BLOCKING_THREADS: usize = 4;

/// Tokio's blocking pool of the daemon's runtime, bounded to [`BLOCKING_THREADS`] when the runtime
/// is built ([`runtime`]): every adapter's blocking job runs there (fnd §4.4 rule 3). The jobs in
/// flight are bounded by their callers (the view service's parallelism and read budget), not by a
/// queue here.
#[derive(Debug, Clone)]
pub struct TokioPool(tokio::runtime::Handle);

impl TokioPool {
    /// The pool of the runtime this is called on (`run::run`'s factory runs inside the daemon's);
    /// `None` outside a runtime.
    #[must_use]
    pub fn current() -> Option<Self> {
        tokio::runtime::Handle::try_current().ok().map(Self)
    }
}

impl BlockingPool for TokioPool {
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let handle = self.0.spawn_blocking(job);
        async move { handle.await.map_err(|_| Interrupted) }
    }
}

/// The daemon's current-thread runtime, its blocking pool bounded to [`BLOCKING_THREADS`].
pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .max_blocking_threads(BLOCKING_THREADS)
        .build()
}

/// The system clock, and the runtime's timer for deadlines.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

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
        timer::deadline(after)
    }
}
