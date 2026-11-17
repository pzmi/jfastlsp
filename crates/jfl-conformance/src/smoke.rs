//! The driver of `formal/smoke.qnt` (FORMAL-02): a test-only counter stands in for the real code,
//! so the harness is proven before a model of the product has a driver. Its negative control is a
//! counter that skips a value ([`SKIPPED`]): replayed over any trace that counts that far, it must
//! diverge at the step whose model state is [`SKIPPED`] (ver §6.2 item 4). Pinned by
//! tests/replay.rs::should_fail_at_the_skipped_value_when_the_negative_control_replays_a_trace.

use serde::Deserialize;

use crate::driver::{Driver, Step, StepError};
use crate::replay::{self, Context, Failure};
use crate::trace::RawStep;

/// The value the faulty counter of the negative control skips.
pub const SKIPPED: i64 = 3;

/// The counter under test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counter {
    count: i64,
    skips: Option<i64>,
}

impl Counter {
    /// A correct counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// The negative control's counter: it never takes the value `skipped`, counting past it.
    pub fn skipping(skipped: i64) -> Self {
        Self {
            count: 0,
            skips: Some(skipped),
        }
    }

    /// Starts at zero.
    pub fn reset(&mut self) {
        self.count = 0;
    }

    /// Counts one up; refuses to overflow.
    pub fn increment(&mut self) -> Result<(), String> {
        let next = self
            .count
            .checked_add(1)
            .ok_or_else(|| format!("{} + 1 overflows", self.count))?;
        self.count = if Some(next) == self.skips {
            next.checked_add(1)
                .ok_or_else(|| format!("{next} + 1 overflows"))?
        } else {
            next
        };
        Ok(())
    }

    /// The current value.
    pub fn count(&self) -> i64 {
        self.count
    }
}

/// The smoke model's variables, as the driver observes them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmokeState {
    /// The counter.
    pub count: i64,
}

/// Drives a [`Counter`] by `smoke.qnt`'s actions: `init` resets it, `step` increments it.
#[derive(Debug, Clone, Default)]
pub struct SmokeDriver {
    counter: Counter,
}

impl SmokeDriver {
    /// A driver over `counter`.
    pub fn new(counter: Counter) -> Self {
        Self { counter }
    }
}

impl Driver for SmokeDriver {
    type State = SmokeState;

    fn step(&mut self, step: &Step) -> Result<(), StepError> {
        match step.action() {
            "init" => {
                self.counter.reset();
                Ok(())
            }
            "step" => self.counter.increment().map_err(StepError::Refused),
            _ => Err(StepError::unknown(step)),
        }
    }

    fn observe(&self) -> SmokeState {
        SmokeState {
            count: self.counter.count(),
        }
    }
}

/// Replays `steps` through the correct counter.
pub fn replay(context: &Context<'_>, steps: Vec<RawStep>) -> Result<usize, Failure> {
    replay::replay(SmokeDriver::new(Counter::new()), context, steps)
}

/// The negative control: replays `steps` through a counter that skips [`SKIPPED`].
pub fn control(context: &Context<'_>, steps: Vec<RawStep>) -> Result<usize, Failure> {
    replay::replay(SmokeDriver::new(Counter::skipping(SKIPPED)), context, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_skip_only_the_named_value_when_the_counter_is_faulty() {
        // given
        let mut faulty = Counter::skipping(SKIPPED);

        // when
        let seen: Vec<i64> = (0..4)
            .map(|_| {
                let _ = faulty.increment();
                faulty.count()
            })
            .collect();

        // then
        assert_eq!(seen, [1, 2, 4, 5]);
    }

    #[test]
    fn should_refuse_when_the_counter_would_overflow() {
        // given
        let mut counter = Counter {
            count: i64::MAX,
            skips: None,
        };

        // when
        let refused = counter.increment();

        // then
        assert!(refused.is_err());
        assert_eq!(counter.count(), i64::MAX);
    }
}
