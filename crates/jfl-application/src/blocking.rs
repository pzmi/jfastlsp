//! The `BlockingPool` port: where an adapter runs a blocking job (fnd §4.4 rule 3).
//!
//! File reads, walks and stats are blocking system calls; run on a runtime worker they would stall
//! every request behind them. The adapter that owns such a job hands it to a `BlockingPool` and
//! awaits the result. Composition binds the port to Tokio's bounded blocking pool; tests bind it
//! to `jfl_testkit::exec::InlinePool`, which runs the job in place.
//!
//! Rejected alternatives: an adapter owning its own threads needs the `spawn` grant, which the fs
//! adapters do not hold (fnd §2.4); a synchronous port contradicts fnd §4.4 rule 1 and leaves every
//! caller to remember which calls block.

use core::future::Future;

/// The job did not run to completion: the pool shut down first, or the job panicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the blocking job was interrupted")]
pub struct Interrupted;

/// A bounded pool of threads that may block.
pub trait BlockingPool: Send + Sync + 'static {
    /// Runs `job` off the async runtime and resolves to its result.
    ///
    /// Cancel-safe: dropping the future discards the result; a job already started runs to its
    /// end, so a job must hold no state another call depends on (each `SourceTree` job only reads).
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static;
}
