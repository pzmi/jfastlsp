//! The harness's one error type: a message that says what failed and where.
//!
//! As in `jfl-bench`, nobody branches on the kind of failure, only a person reads the last line,
//! so a message with its context prefixed is the whole type. A message names files and ids, never
//! the text of a task, a transcript or a source file: errors reach a terminal, and a terminal log
//! gets pasted.

use std::fmt;

/// A failure, as one human-readable line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError(String);

impl EvalError {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EvalError {}

/// The harness's result type.
pub type Result<T> = std::result::Result<T, EvalError>;

/// Prefix a failure with what was being attempted.
pub trait Context<T> {
    /// `what: <the original failure>`.
    fn context(self, what: impl fmt::Display) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, what: impl fmt::Display) -> Result<T> {
        self.map_err(|e| EvalError(format!("{what}: {e}")))
    }
}
