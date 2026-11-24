//! Every timer of the daemon, in one place: the run loop's tick (the idle period, the lock-path
//! and socket-path checks, the accept pause) and the deadlines on a new connection's handshake
//! and on each reply's write: Tokio's `interval` and `timeout`, under the `timer` grant
//! `jfl-composition` holds for this module alone (ADR 0039, by review). Only a deadline on an event
//! or a periodic tick belongs here; a plain sleep or a polling loop is banned everywhere. The proxy
//! waits with `poll(2)` and needs no grant.

use std::future::Future;
use std::time::Duration;

/// A tick every `period`, the first at once.
pub(crate) fn ticker(period: Duration) -> tokio::time::Interval {
    tokio::time::interval(period)
}

/// `future`'s output, or `None` when `deadline` passes first (the future is dropped then).
pub(crate) async fn within<F: Future>(deadline: Duration, future: F) -> Option<F::Output> {
    tokio::time::timeout(deadline, future).await.ok()
}

/// Resolves once `after` has passed: the view service's cookie deadline
/// (`jfl_application::WallClock::deadline`), which it races against the cookie's event, so it is a
/// deadline on an event, never a wait for its own sake (ADR 0039). Tokio's `timeout` over a future
/// that never resolves, since a plain sleep is banned even here.
pub(crate) async fn deadline(after: Duration) {
    let _passed = tokio::time::timeout(after, std::future::pending::<()>()).await;
}
