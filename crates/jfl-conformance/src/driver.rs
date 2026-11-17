//! The driver: the real code behind a model (ver §6.2 item 3). Shaped like `quint-connect`'s
//! `Driver` (a step dispatched by its action's name, its picks decoded with serde, a state
//! compared after every step), without its macros: a driver matches on [`Step::action`] and
//! answers [`StepError::unknown`] for an action it has no case for.

use std::fmt::Debug;

use itf::Value;
use itf::value::Record;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// One step handed to a driver: the action the model took and its nondeterministic picks.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    index: usize,
    action: String,
    picks: Record,
}

impl Step {
    /// A step at `index` (0 is the initial state) taking `action` with `picks`.
    pub fn new(index: usize, action: impl Into<String>, picks: Record) -> Self {
        Self {
            index,
            action: action.into(),
            picks,
        }
    }

    /// The position in the trace; 0 is the initial state.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The action's name, as the model declares it (`init`, `step`, or a named action).
    pub fn action(&self) -> &str {
        &self.action
    }

    /// The nondeterministic pick `name`, decoded: `None` when the action made no such pick or
    /// Quint recorded it as `None` (an action that did not use it), the value of a `Some`.
    pub fn pick<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, StepError> {
        let Some(value) = self.picks.get(name) else {
            return Ok(None);
        };
        let value = match value {
            Value::Record(variant) => match (variant.get("tag"), variant.get("value")) {
                (Some(Value::String(tag)), _) if tag == "None" => return Ok(None),
                (Some(Value::String(tag)), Some(inner)) if tag == "Some" => inner.clone(),
                _ => value.clone(),
            },
            other => other.clone(),
        };
        itf::de::decode_value(value).map(Some).map_err(|e| {
            StepError::Harness(format!(
                "step {}: the pick `{name}` of `{}` does not decode: {e}",
                self.index, self.action
            ))
        })
    }
}

/// Why a driver could not apply a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// The harness cannot apply the step (an unknown action, a pick that does not decode): a
    /// harness error, never a pass.
    Harness(String),
    /// The real code refused a step the model took: a divergence.
    Refused(String),
}

impl StepError {
    /// The error for an action the driver has no case for.
    pub fn unknown(step: &Step) -> Self {
        Self::Harness(format!(
            "step {}: the driver has no case for the action `{}`",
            step.index, step.action
        ))
    }
}

/// The real code behind one model.
pub trait Driver {
    /// The canonical observation compared with the model's state after every step: decoded from
    /// the model's variables (ITF through serde) and built from the real code by
    /// [`Driver::observe`]. It is `Serialize` so the replay can check that it names exactly the
    /// trace's variables: an observation narrower than the model would compare too little.
    type State: DeserializeOwned + Serialize + PartialEq + Debug;

    /// Applies the step's action, with its picks, to the real code (never a re-implementation of
    /// the transition, ver §6.2 item 3).
    fn step(&mut self, step: &Step) -> Result<(), StepError>;

    /// The real code's state, observed.
    fn observe(&self) -> Self::State;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks(json: &str) -> Record {
        match serde_json::from_str::<Value>(json) {
            Ok(Value::Record(record)) => record,
            _ => Record::new(),
        }
    }

    #[test]
    fn should_decode_each_pick_when_quint_records_it_as_an_option_or_a_value() {
        // given
        let step = Step::new(
            1,
            "write",
            picks(
                r##"{"file": {"tag": "Some", "value": "A.java"}, "worker": {"tag": "None", "value": {"#tup": []}}, "n": {"#bigint": "7"}, "bad": {"tag": "Some", "value": "x"}}"##,
            ),
        );

        // when
        let file = step.pick::<String>("file");
        let worker = step.pick::<i64>("worker");
        let n = step.pick::<i64>("n");
        let absent = step.pick::<i64>("absent");
        let bad = step.pick::<i64>("bad");

        // then
        assert_eq!(file, Ok(Some("A.java".to_owned())));
        assert_eq!(worker, Ok(None));
        assert_eq!(n, Ok(Some(7)));
        assert_eq!(absent, Ok(None));
        assert!(matches!(bad, Err(StepError::Harness(why)) if why.contains("`bad` of `write`")));
    }
}
