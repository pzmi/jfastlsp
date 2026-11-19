//! The `WallClock` port: the time the view service reads before every stat (git's racy-mtime rule,
//! plan §4.3) and the deadline it waits for a cookie with (ADR 0037).
//!
//! This crate holds no clock grant (fnd §2.4): composition binds the port to the system clock and
//! its runtime's timer, and tests to `jfl_testkit::fakes::clock::FakeWallClock`, whose time moves
//! only when a test moves it.

use core::future::Future;
use core::time::Duration;

use crate::source_tree::MTime;

/// The wall clock and a timer.
pub trait WallClock: Send + Sync {
    /// The time now, on the clock the filesystem stamps files with: read *before* a stat, so a
    /// file written within the timestamp granularity of the stat is called racy.
    fn now(&self) -> MTime;

    /// Resolves once `after` has passed. Cancel-safe: dropping it cancels the timer.
    fn deadline(&self, after: Duration) -> impl Future<Output = ()> + Send;
}

/// How long passed from `from` to `to` (zero when `to` is earlier).
///
/// Pinned by: `clock::tests::should_measure_the_time_between_two_readings_when_to_is_later`.
#[must_use]
pub fn elapsed(from: MTime, to: MTime) -> Duration {
    let secs = to.secs.saturating_sub(from.secs);
    let Ok(secs) = u64::try_from(secs) else {
        return Duration::ZERO;
    };
    let whole = Duration::from_secs(secs);
    let (to_nanos, from_nanos) = (
        Duration::from_nanos(u64::from(to.nanos)),
        u64::from(from.nanos),
    );
    whole
        .saturating_add(to_nanos)
        .saturating_sub(Duration::from_nanos(from_nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_measure_the_time_between_two_readings_when_to_is_later() {
        // given
        let from = MTime {
            secs: 10,
            nanos: 900_000_000,
        };
        let to = MTime {
            secs: 11,
            nanos: 100_000_000,
        };

        // when
        let forward = elapsed(from, to);
        let backward = elapsed(to, from);

        // then
        assert_eq!(forward, Duration::from_millis(200));
        assert_eq!(backward, Duration::ZERO);
    }
}
