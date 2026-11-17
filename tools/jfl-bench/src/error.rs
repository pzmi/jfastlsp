//! The harness's one error type: a message that says what failed and where.
//!
//! A harness has no caller that branches on the kind of failure, only a person reading the last
//! line, so a message (with its context prefixed as it propagates) is the whole type; a typed
//! enum per module was rejected as ceremony nobody would match on.

use std::fmt;

/// A failure, as one human-readable line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchError(String);

impl BenchError {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BenchError {}

/// The harness's result type.
pub type Result<T> = std::result::Result<T, BenchError>;

/// Prefix a failure with what was being attempted.
pub trait Context<T> {
    /// `what: <the original failure>`.
    fn context(self, what: impl fmt::Display) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, what: impl fmt::Display) -> Result<T> {
        self.map_err(|e| BenchError(format!("{what}: {e}")))
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, what: impl fmt::Display) -> Result<T> {
        self.ok_or_else(|| BenchError(what.to_string()))
    }
}
