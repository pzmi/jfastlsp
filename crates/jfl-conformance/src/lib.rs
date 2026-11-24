//! The Quint ITF replay harness and the conformance drivers (ver §6.2, plan §7, ADR 0007 as
//! amended). Role: test only.
//!
//! A driver ([`driver::Driver`]) applies each step of a Quint trace (its action and
//! nondeterministic picks) to the real code and observes the real state; [`replay`] compares that
//! observation with the model's state after every step, the initial state included, and reports a
//! divergence with model, seed, trace, step, action, expected, actual and a line diff. A trace
//! that cannot be read as an ITF trace with Quint's `--mbt` metadata is a [`HarnessError`], never
//! a pass ([`trace`]). The committed regression traces are listed in
//! `formal/traces/regression/manifest.toml` ([`manifest`]); the drivers are registered by model
//! in [`registry`], each with its negative control; the `jfl-conformance` binary ([`cli`]) replays
//! the committed traces and the seeded traces xtask's `conformance-replay` stage generates.
//!
//! `quint-connect` 0.1 was weighed and not taken (ADR 0007, amended for FORMAL-02): its `Step` cannot
//! be built outside the crate, so committed traces cannot go through its `Driver`; its `quint run`
//! uses Quint's default `rust` backend, which downloads an evaluator at run time; its seed and
//! verbosity are read at compile time; and its divergence report names no step. The `Driver`
//! here keeps its shape (a step dispatched by action name, a state compared after every step).

use std::fmt;

pub mod cli;
pub mod daemon;
pub mod diagnostics;
pub mod driver;
pub mod manifest;
pub mod registry;
pub mod replay;
pub mod smoke;
pub mod trace;
pub mod views;

/// The harness itself failed: a trace that is not an ITF trace with Quint's `--mbt` metadata, a
/// state the driver cannot decode, an action the driver has no case for, a missing file or an
/// empty run. Never a pass (FORMAL-02's fourth `Accept:`); the binary exits 3 and prints
/// `HARNESS_ERROR`. Pinned by tests/replay.rs::should_report_a_harness_error_when_the_itf_file_is_malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError(pub String);

impl HarnessError {
    /// A harness error saying `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HARNESS_ERROR: {}", self.0)
    }
}

impl std::error::Error for HarnessError {}
