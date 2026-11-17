//! Replaying one trace through a driver (ver §6.2 item 3): each step's action is applied to the
//! real code and the observed state is compared with the model's, after every step, the initial
//! state included. The first difference is a [`Divergence`] naming model, seed, trace, step,
//! action, expected (the model's state), actual (the real code's) and a line diff; a step the real
//! code refused is a divergence too. Anything the harness cannot do is a [`HarnessError`].
//!
//! Pinned by tests/replay.rs::should_fail_at_the_skipped_value_when_the_negative_control_replays_a_trace.
//!
//! The comparison is structural: a driver reaches the loop only as a [`Replayer`], which can be
//! built only from a [`Driver`] ([`Replayer::new`]), and the code that decodes the model's state
//! and compares it with [`Driver::observe`] is this module's private `Erased` implementation for
//! every driver. A driver supplies `step` and `observe` and nothing else, so no driver can replay
//! without the comparison or skip a step of it; and the observation must name exactly the trace's
//! variables (its serialised keys), so a narrow observation cannot make the comparison vacuous.
//! Pinned by `replay::tests::should_compare_the_initial_state_when_a_driver_observes_it_wrongly`
//! and `replay::tests::should_refuse_an_observation_when_it_omits_a_variable`.

use std::fmt;

use itf::Value;
use similar::TextDiff;

use crate::HarnessError;
use crate::driver::{Driver, Step, StepError};
use crate::trace::RawStep;

/// The comparison of one step: the model's state and the real code's, rendered for the report.
struct Compared {
    equal: bool,
    expected: String,
    actual: String,
}

/// A driver as the replay loop sees it. Private to this module, so the only implementation is
/// the blanket one below: the comparison cannot be overridden or bypassed by a driver.
trait Erased {
    fn apply(&mut self, step: &Step) -> Result<(), StepError>;
    fn compare(&self, model: Value) -> Result<Compared, String>;
}

impl<D: Driver> Erased for D {
    fn apply(&mut self, step: &Step) -> Result<(), StepError> {
        self.step(step)
    }

    fn compare(&self, model: Value) -> Result<Compared, String> {
        let actual = self.observe();
        let variables: Vec<String> = match &model {
            Value::Record(record) => record.iter().map(|(k, _)| k.clone()).collect(),
            other => return Err(format!("the model's state is {other:?}, not a record")),
        };
        let observed: Vec<String> = match serde_json::to_value(&actual) {
            Ok(serde_json::Value::Object(fields)) => fields.keys().cloned().collect(),
            Ok(other) => {
                return Err(format!(
                    "the observation serialises as {other}, not a record"
                ));
            }
            Err(e) => return Err(format!("the observation does not serialise: {e}")),
        };
        let (mut variables, mut observed) = (variables, observed);
        variables.sort();
        observed.sort();
        if variables != observed {
            return Err(format!(
                "the observation names {observed:?}, the trace's variables are {variables:?}: an observation must cover the whole model"
            ));
        }
        let expected: D::State = itf::de::decode_value(model).map_err(|e| e.to_string())?;
        Ok(Compared {
            equal: expected == actual,
            expected: format!("{expected:#?}"),
            actual: format!("{actual:#?}"),
        })
    }
}

/// A driver handed to [`replay`]: built only from a [`Driver`], so the replay loop, not the
/// driver, compares the model's state with the observation after every step.
pub struct Replayer(Box<dyn Erased>);

impl Replayer {
    /// `driver`, behind the harness's comparison.
    pub fn new<D: Driver + 'static>(driver: D) -> Self {
        Self(Box::new(driver))
    }
}

impl fmt::Debug for Replayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Replayer")
    }
}

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
pub fn replay(
    mut driver: Replayer,
    context: &Context<'_>,
    steps: Vec<RawStep>,
) -> Result<usize, Failure> {
    if steps.is_empty() {
        return Err(HarnessError::new(format!("{}: the trace has no state", context.trace)).into());
    }
    let mut conformed = 0_usize;
    for raw in steps {
        let step = Step::new(raw.index, raw.action.clone(), raw.picks);
        let refused = match driver.0.apply(&step) {
            Ok(()) => None,
            Err(StepError::Refused(why)) => Some(why),
            Err(StepError::Harness(why)) => {
                return Err(HarnessError::new(format!("{}: {why}", context.trace)).into());
            }
        };
        let compared = driver.0.compare(raw.state).map_err(|e| {
            HarnessError::new(format!(
                "{}: step {}: the model's state does not decode as the driver's observation: {e}",
                context.trace, raw.index
            ))
        })?;
        if refused.is_some() || !compared.equal {
            return Err(Failure::Diverged(Box::new(Divergence {
                model: context.model.to_owned(),
                seed: context.seed.to_owned(),
                trace: context.trace.to_owned(),
                step: raw.index,
                action: raw.action,
                expected: compared.expected,
                actual: compared.actual,
                refused,
            })));
        }
        conformed = conformed.saturating_add(1);
    }
    Ok(conformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace;
    use serde::{Deserialize, Serialize};

    /// A driver whose observation is off by one from the start.
    struct OffByOne(i64);

    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct Count {
        count: i64,
    }

    /// A driver that observes nothing: every state would compare equal.
    struct Blind;

    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct Nothing {}

    impl Driver for Blind {
        type State = Nothing;

        fn step(&mut self, _step: &Step) -> Result<(), StepError> {
            Ok(())
        }

        fn observe(&self) -> Nothing {
            Nothing {}
        }
    }

    const ONE: &str = r##"{"#meta":{"format":"ITF"},"vars":["count","mbt::actionTaken","mbt::nondetPicks"],"states":[{"#meta":{"index":0},"count":{"#bigint":"0"},"mbt::actionTaken":"init","mbt::nondetPicks":{}}]}"##;

    #[test]
    fn should_refuse_an_observation_when_it_omits_a_variable() -> Result<(), String> {
        // given
        let steps = trace::parse("one", ONE).map_err(|e| e.to_string())?;
        let context = Context {
            model: "one",
            seed: "0x1",
            trace: "one",
        };

        // when
        let replayed = replay(Replayer::new(Blind), &context, steps);

        // then
        let Err(Failure::Harness(error)) = replayed else {
            return Err(format!("the blind driver was not refused: {replayed:?}"));
        };
        assert!(
            error
                .0
                .contains(r#"the observation names [], the trace's variables are ["count"]"#),
            "{error}"
        );
        Ok(())
    }

    impl Driver for OffByOne {
        type State = Count;

        fn step(&mut self, step: &Step) -> Result<(), StepError> {
            match step.action() {
                "init" => {
                    self.0 = 1;
                    Ok(())
                }
                _ => Err(StepError::unknown(step)),
            }
        }

        fn observe(&self) -> Count {
            Count { count: self.0 }
        }
    }

    #[test]
    fn should_compare_the_initial_state_when_a_driver_observes_it_wrongly() -> Result<(), String> {
        // given
        let steps = trace::parse("one", ONE).unwrap_or_default();
        let context = Context {
            model: "one",
            seed: "0x1",
            trace: "one",
        };

        // when
        let replayed = replay(Replayer::new(OffByOne(0)), &context, steps);

        // then
        let Err(Failure::Diverged(divergence)) = replayed else {
            return Err(format!("the off-by-one driver conformed: {replayed:?}"));
        };
        assert_eq!(divergence.step, 0);
        assert!(divergence.expected.contains("count: 0"), "{divergence}");
        assert!(divergence.actual.contains("count: 1"), "{divergence}");
        Ok(())
    }
}
