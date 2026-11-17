//! Replaying one trace through a driver (ver §6.2 item 3): each step's action is applied to the
//! real code and the observed state is compared with the model's, after every step, the initial
//! state included. The first difference is a [`Divergence`] naming model, seed, trace, step,
//! action, expected (the model's state), actual (the real code's) and a line diff; a step the real
//! code refused is a divergence too. Anything the harness cannot do is a [`HarnessError`].
//!
//! Pinned by tests/replay.rs::should_fail_at_the_skipped_value_when_the_negative_control_replays_a_trace.

use std::fmt;

use similar::TextDiff;

use crate::HarnessError;
use crate::driver::{Driver, Step, StepError};
use crate::trace::RawStep;

/// What a replay is of, for its report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context<'a> {
    /// The model's name in the MODELS registry.
    pub model: &'a str,
    /// The seed Quint generated the trace with.
    pub seed: &'a str,
    /// The trace file.
    pub trace: &'a str,
}

/// The model and the real code disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The model.
    pub model: String,
    /// The seed of the trace.
    pub seed: String,
    /// The trace file.
    pub trace: String,
    /// The step (0 is the initial state).
    pub step: usize,
    /// The action of that step.
    pub action: String,
    /// The model's state after the step, as the driver's observation.
    pub expected: String,
    /// The real code's observed state after the step.
    pub actual: String,
    /// When the real code refused the step, why.
    pub refused: Option<String>,
}

impl Divergence {
    /// The unified line diff from expected (the model) to actual (the implementation).
    pub fn diff(&self) -> String {
        let (expected, actual) = (format!("{}\n", self.expected), format!("{}\n", self.actual));
        TextDiff::from_lines(&expected, &actual)
            .unified_diff()
            .context_radius(3)
            .header("model", "implementation")
            .to_string()
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "DIVERGENCE: model {}, seed {}, trace {}, step {}, action {}",
            self.model, self.seed, self.trace, self.step, self.action
        )?;
        if let Some(why) = &self.refused {
            writeln!(f, "the implementation refused the step: {why}")?;
        }
        writeln!(f, "expected (model):\n{}", self.expected)?;
        writeln!(f, "actual (implementation):\n{}", self.actual)?;
        write!(f, "diff:\n{}", self.diff())
    }
}

/// Why a replay did not conform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The real code diverged from the model.
    Diverged(Box<Divergence>),
    /// The harness could not replay the trace.
    Harness(HarnessError),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diverged(divergence) => divergence.fmt(f),
            Self::Harness(error) => error.fmt(f),
        }
    }
}

impl From<HarnessError> for Failure {
    fn from(error: HarnessError) -> Self {
        Self::Harness(error)
    }
}

/// Replays `steps` through `driver`: the number of steps that conformed, or the first failure.
/// An empty trace is a harness error (it would conform vacuously).
pub fn replay<D: Driver>(
    mut driver: D,
    context: &Context<'_>,
    steps: Vec<RawStep>,
) -> Result<usize, Failure> {
    if steps.is_empty() {
        return Err(HarnessError::new(format!("{}: the trace has no state", context.trace)).into());
    }
    let mut conformed = 0_usize;
    for raw in steps {
        let step = Step::new(raw.index, raw.action.clone(), raw.picks);
        let refused = match driver.step(&step) {
            Ok(()) => None,
            Err(StepError::Refused(why)) => Some(why),
            Err(StepError::Harness(why)) => {
                return Err(HarnessError::new(format!("{}: {why}", context.trace)).into());
            }
        };
        let expected: D::State = itf::de::decode_value(raw.state).map_err(|e| {
            HarnessError::new(format!(
                "{}: step {}: the model's state does not decode as the driver's observation: {e}",
                context.trace, raw.index
            ))
        })?;
        let actual = driver.observe();
        if refused.is_some() || expected != actual {
            return Err(Failure::Diverged(Box::new(Divergence {
                model: context.model.to_owned(),
                seed: context.seed.to_owned(),
                trace: context.trace.to_owned(),
                step: raw.index,
                action: raw.action,
                expected: format!("{expected:#?}"),
                actual: format!("{actual:#?}"),
                refused,
            })));
        }
        conformed = conformed.saturating_add(1);
    }
    Ok(conformed)
}
