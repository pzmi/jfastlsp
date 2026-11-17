//! Reading a Quint ITF trace into steps (ver §6.2): each state of the trace is one step, the
//! initial state included, with the action Quint's `--mbt` mode recorded (`mbt::actionTaken`),
//! its nondeterministic picks (`mbt::nondetPicks`) and the model's state (every other variable).
//!
//! Fails closed: anything that is not such a trace is a [`HarnessError`], never an empty pass:
//! a file over [`MAX_TRACE_BYTES`], text that is not JSON or not an ITF trace, a trace with no
//! state, a state that is not a record, a state without the `--mbt` variables or with an empty
//! action, picks that are not a record, or a state whose `#meta.index` is not its position.
//! Pinned by tests/replay.rs::should_report_a_harness_error_when_the_itf_file_is_malformed.

use std::io::Read as _;
use std::path::Path;

use itf::Trace;
use itf::Value;
use itf::value::Record;

use crate::HarnessError;

/// The largest trace file read: Quint's traces of the models' budgets are kilobytes.
pub const MAX_TRACE_BYTES: u64 = 64 * 1024 * 1024;

/// The variable holding the action that produced a state (`quint run --mbt`).
pub const ACTION: &str = "mbt::actionTaken";

/// The variable holding that action's nondeterministic picks (`quint run --mbt`).
pub const PICKS: &str = "mbt::nondetPicks";

/// One state of a trace: the action that produced it, its picks and the model's state.
#[derive(Debug, Clone, PartialEq)]
pub struct RawStep {
    /// The position in the trace; 0 is the initial state.
    pub index: usize,
    /// The action Quint took (`init` for the initial state).
    pub action: String,
    /// The action's nondeterministic picks, by name.
    pub picks: Record,
    /// The model's state: every variable but the `--mbt` ones.
    pub state: Value,
}

/// The steps of the ITF trace `text`, named `name` in errors.
pub fn parse(name: &str, text: &str) -> Result<Vec<RawStep>, HarnessError> {
    let bad = |why: String| HarnessError::new(format!("{name}: {why}"));
    let trace: Trace<Value> =
        serde_json::from_str(text).map_err(|e| bad(format!("not an ITF trace: {e}")))?;
    if trace.states.is_empty() {
        return Err(bad("the trace has no state".to_owned()));
    }
    let mut steps = Vec::with_capacity(trace.states.len());
    for (index, state) in trace.states.into_iter().enumerate() {
        if let Some(said) = state.meta.index
            && usize::try_from(said).ok() != Some(index)
        {
            return Err(bad(format!(
                "state {index} says it is state {said} (#meta.index)"
            )));
        }
        let Value::Record(mut record) = state.value else {
            return Err(bad(format!("state {index} is not a record")));
        };
        let action = match record.remove(ACTION) {
            Some(Value::String(action)) if !action.is_empty() => action,
            Some(other) => {
                return Err(bad(format!(
                    "state {index}: `{ACTION}` is {other:?}, not an action's name (an anonymous action?)"
                )));
            }
            None => {
                return Err(bad(format!(
                    "state {index} has no `{ACTION}`: generate traces with `quint run --mbt`"
                )));
            }
        };
        let picks = match record.remove(PICKS) {
            Some(Value::Record(picks)) => picks,
            Some(other) => {
                return Err(bad(format!(
                    "state {index}: `{PICKS}` is {other:?}, not a record"
                )));
            }
            None => {
                return Err(bad(format!(
                    "state {index} has no `{PICKS}`: generate traces with `quint run --mbt`"
                )));
            }
        };
        steps.push(RawStep {
            index,
            action,
            picks,
            state: Value::Record(record),
        });
    }
    Ok(steps)
}

/// The steps of the ITF trace file `path`, named `name` in errors, read up to
/// [`MAX_TRACE_BYTES`].
pub fn read(path: &Path, name: &str) -> Result<Vec<RawStep>, HarnessError> {
    let file = std::fs::File::open(path)
        .map_err(|e| HarnessError::new(format!("{name}: cannot open: {e}")))?;
    let mut text = String::new();
    file.take(MAX_TRACE_BYTES.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(|e| HarnessError::new(format!("{name}: cannot read as UTF-8: {e}")))?;
    if u64::try_from(text.len()).map_or(true, |n| n > MAX_TRACE_BYTES) {
        return Err(HarnessError::new(format!(
            "{name}: larger than {MAX_TRACE_BYTES} bytes"
        )));
    }
    parse(name, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = r##"{"#meta":{"format":"ITF"},"vars":["count","mbt::actionTaken","mbt::nondetPicks"],"states":[{"#meta":{"index":0},"count":{"#bigint":"0"},"mbt::actionTaken":"init","mbt::nondetPicks":{}},{"#meta":{"index":1},"count":{"#bigint":"1"},"mbt::actionTaken":"step","mbt::nondetPicks":{}}]}"##;

    #[test]
    fn should_read_each_state_as_a_step_when_the_trace_is_well_formed() {
        // given
        let text = TWO;

        // when
        let steps = parse("two", text);

        // then
        let steps = steps.unwrap_or_default();
        let actions: Vec<(usize, &str)> =
            steps.iter().map(|s| (s.index, s.action.as_str())).collect();
        assert_eq!(actions, [(0, "init"), (1, "step")]);
        assert!(matches!(
            steps.get(1).map(|s| &s.state),
            Some(Value::Record(r)) if r.contains_key("count") && !r.contains_key(ACTION)
        ));
    }
}
