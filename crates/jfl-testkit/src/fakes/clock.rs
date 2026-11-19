//! [`FakeWallClock`]: a `WallClock` whose time moves only when a test moves it, and whose
//! deadlines have always passed.
//!
//! A deadline that is already due keeps a test from waiting for anything: the view service polls
//! the watch first and the deadline second, so a hint already queued (every fake watch's hints
//! are queued by the time the change returns) is always taken before the deadline is seen, and a
//! hint that never comes ([`crate::fakes::source_tree::FakeSourceTree::lose_cookies`]) meets a
//! deadline at once.

use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use jfl_application::WallClock;
use jfl_application::source_tree::MTime;

/// The fake wall clock; clones share it.
#[derive(Debug, Clone)]
pub struct FakeWallClock(Arc<AtomicI64>);

impl FakeWallClock {
    /// A clock at `secs` seconds since the epoch.
    #[must_use]
    pub fn at(secs: i64) -> Self {
        Self(Arc::new(AtomicI64::new(secs)))
    }

    /// Moves the clock `secs` seconds on.
    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, Ordering::Relaxed);
    }
}

impl WallClock for FakeWallClock {
    fn now(&self) -> MTime {
        MTime {
            secs: self.0.load(Ordering::Relaxed),
            nanos: 0,
        }
    }

    fn deadline(&self, after: Duration) -> impl Future<Output = ()> + Send {
        let _ = after;
        core::future::ready(())
    }
}
