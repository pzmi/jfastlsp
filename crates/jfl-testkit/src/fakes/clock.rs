//! [`FakeWallClock`]: a `WallClock` whose time moves only when a test moves it (or, for a
//! [`stepping`](FakeWallClock::stepping) clock, by a fixed step at each reading), and whose
//! deadlines have always passed.
//!
//! A deadline that is already due keeps a test from waiting for anything: the view service polls
//! the watch first and the deadline second, so a hint already queued (every fake watch's hints
//! are queued by the time the change returns) is always taken before the deadline is seen, and a
//! hint that never comes ([`crate::fakes::source_tree::FakeSourceTree::lose_cookies`]) meets a
//! deadline at once.
//!
//! A stepping clock is how a test reaches a query's internal deadline (plan §2.7) without
//! waiting: each reading is `step` later than the one before, so a query that reads the clock
//! once per unit of work passes its deadline after a known number of units.

use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use jfl_application::WallClock;
use jfl_application::source_tree::MTime;

const NANOS: i64 = 1_000_000_000;

#[derive(Debug)]
struct State {
    /// Nanoseconds since the epoch.
    now: AtomicI64,
    /// Nanoseconds added after each reading.
    step: i64,
}

/// The fake wall clock; clones share it.
#[derive(Debug, Clone)]
pub struct FakeWallClock(Arc<State>);

impl FakeWallClock {
    /// A clock at `secs` seconds since the epoch.
    #[must_use]
    pub fn at(secs: i64) -> Self {
        Self::stepping(secs, Duration::ZERO)
    }

    /// A clock at `secs` seconds since the epoch that moves `step` on after each reading.
    #[must_use]
    pub fn stepping(secs: i64, step: Duration) -> Self {
        Self(Arc::new(State {
            now: AtomicI64::new(secs.saturating_mul(NANOS)),
            step: i64::try_from(step.as_nanos()).unwrap_or(i64::MAX),
        }))
    }

    /// Moves the clock `secs` seconds on.
    pub fn advance(&self, secs: i64) {
        self.0
            .now
            .fetch_add(secs.saturating_mul(NANOS), Ordering::Relaxed);
    }
}

impl WallClock for FakeWallClock {
    fn now(&self) -> MTime {
        let at = self.0.now.fetch_add(self.0.step, Ordering::Relaxed);
        MTime {
            secs: at.div_euclid(NANOS),
            nanos: u32::try_from(at.rem_euclid(NANOS)).unwrap_or(0),
        }
    }

    fn deadline(&self, after: Duration) -> impl Future<Output = ()> + Send {
        let _ = after;
        core::future::ready(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_move_by_its_step_when_a_stepping_clock_is_read() {
        // given
        let clock = FakeWallClock::stepping(10, Duration::from_millis(600));
        // when
        let readings = [clock.now(), clock.now(), clock.now()];
        // then
        assert_eq!(
            readings,
            [
                MTime { secs: 10, nanos: 0 },
                MTime {
                    secs: 10,
                    nanos: 600_000_000
                },
                MTime {
                    secs: 11,
                    nanos: 200_000_000
                },
            ]
        );
    }

    #[test]
    fn should_stand_still_when_a_clock_is_read_without_a_step() {
        // given
        let clock = FakeWallClock::at(7);
        // when
        let first = clock.now();
        clock.advance(2);
        let second = clock.now();
        // then
        assert_eq!(first, MTime { secs: 7, nanos: 0 });
        assert_eq!(second, MTime { secs: 9, nanos: 0 });
    }
}
