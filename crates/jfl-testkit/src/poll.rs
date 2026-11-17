//! The only sanctioned wait on a real-OS condition (ver §3 rule 4, fnd §2.6): a daemon's socket
//! appeared, a child exited, a watcher event arrived. Bounded by a deadline, carrying a reason
//! that the timeout shows, and never a correctness oracle: a test asserts on the condition's
//! documented signal after the wait, not on the fact that the wait returned.
//!
//! This module holds the workspace's only `std::thread::sleep` and, with the fixture builders,
//! `jfl-testkit`'s only `Instant::now` (the crate's `sleep` and `clock` grants, fnd §2.4; bound
//! to this module by review). Everywhere else clippy refuses a sleep: pinned by
//! `tools/jfl-architecture/tests/gate_negative_cases.rs::should_fail_clippy_when_a_test_sleeps_outside_poll`.

use core::fmt;
use std::time::{Duration, Instant};

/// How long one wait between checks of the condition lasts at most. Short enough that a met
/// condition is seen promptly, long enough not to spin.
const INTERVAL: Duration = Duration::from_millis(10);

/// The condition did not hold before the deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollTimeout {
    reason: String,
    deadline: Duration,
    checks: u64,
}

impl PollTimeout {
    /// What the caller was waiting for, as passed to [`wait_until`].
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for PollTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "timed out after {:?} waiting for {} ({} checks)",
            self.deadline, self.reason, self.checks
        )
    }
}

impl std::error::Error for PollTimeout {}

/// Waits until `condition` returns `true`, checking it at once and then every few milliseconds,
/// for at most `deadline` from the call (a duration, so a caller needs no clock of its own).
///
/// Returns [`PollTimeout`], whose message names `reason`, when the deadline passes first; the
/// condition is checked one last time at the deadline. Pinned by
/// `poll::tests::should_name_the_reason_when_the_deadline_passes` and
/// `poll::tests::should_return_ok_when_the_condition_holds_later`.
///
/// ```
/// use std::time::Duration;
/// use jfl_testkit::poll::wait_until;
///
/// let mut checks = 0;
/// let waited = wait_until(Duration::from_secs(5), "the third check", || {
///     checks += 1;
///     checks >= 3
/// });
/// assert!(waited.is_ok());
/// ```
pub fn wait_until(
    deadline: Duration,
    reason: &str,
    mut condition: impl FnMut() -> bool,
) -> Result<(), PollTimeout> {
    let start = Instant::now();
    let mut checks: u64 = 0;
    loop {
        checks = checks.saturating_add(1);
        if condition() {
            return Ok(());
        }
        let remaining = deadline.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(PollTimeout {
                reason: reason.to_owned(),
                deadline,
                checks,
            });
        }
        std::thread::sleep(remaining.min(INTERVAL));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_name_the_reason_when_the_deadline_passes() {
        // given
        let deadline = Duration::from_millis(30);

        // when
        let waited = wait_until(deadline, "the socket to appear", || false);

        // then
        let timeout = waited.unwrap_err();
        assert_eq!(timeout.reason(), "the socket to appear");
        let message = timeout.to_string();
        assert!(
            message.contains("waiting for the socket to appear"),
            "{message}"
        );
        assert!(message.contains("30ms"), "{message}");
    }

    #[test]
    fn should_return_ok_when_the_condition_holds_later() {
        // given
        let mut checks = 0_u32;

        // when
        let waited = wait_until(Duration::from_secs(10), "the fifth check", || {
            checks = checks.saturating_add(1);
            checks >= 5
        });

        // then
        assert_eq!(waited, Ok(()));
        assert_eq!(checks, 5);
    }

    #[test]
    fn should_check_once_when_the_deadline_is_zero() {
        // given
        let mut checks = 0_u32;

        // when
        let waited = wait_until(Duration::ZERO, "an immediate answer", || {
            checks = checks.saturating_add(1);
            false
        });

        // then
        assert!(waited.is_err());
        assert_eq!(checks, 1);
    }
}
