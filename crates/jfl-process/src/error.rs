//! Typed refusals and failures (fnd §4.3): what a caller can act on, kept apart.

use std::io;

use crate::run::Exit;

/// A command refused before anything starts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// [`crate::Command::from_argv`] was given no program.
    #[error("the argv is empty: there is no program to run")]
    EmptyArgv,
    /// The program is the empty string.
    #[error("the program is empty")]
    EmptyProgram,
    /// A NUL byte, which `execve` cannot carry, in the named part of the command.
    #[error("{what} holds a NUL byte")]
    Nul {
        /// Which part: the program, an argument, the working directory, a variable name or value.
        what: &'static str,
    },
    /// A variable name that is empty or holds `=`.
    #[error("invalid environment variable name {name:?}")]
    InvalidEnvName {
        /// The name, lossily decoded.
        name: String,
    },
    /// A program without a `/` and no `PATH` in the child's environment to look it up on.
    #[error("{program:?} has no `/` and the child's environment has no PATH to look it up on")]
    NoPathForLookup {
        /// The program, lossily decoded.
        program: String,
    },
    /// A program without a `/` and a `PATH` with an empty or relative entry, which would search
    /// the child's working directory.
    #[error("the child's PATH has the entry {entry:?}, which is not absolute")]
    RelativePathEntry {
        /// The entry, lossily decoded.
        entry: String,
    },
}

/// Why a child could not be run to an [`crate::Outcome`] or started.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The command was refused before anything started.
    #[error(transparent)]
    Refused(#[from] Refusal),
    /// The OS could not start the program (missing, not executable, a bad working directory).
    #[error("failed to start {program:?}: {source}")]
    FailedToStart {
        /// The program, lossily decoded.
        program: String,
        /// The OS error.
        #[source]
        source: io::Error,
    },
    /// Setting up the pipes of, or waiting on, a started child failed; its group was killed and
    /// it was reaped.
    #[error("waiting on the child failed: {0}")]
    Wait(#[source] io::Error),
}

/// Why an exchange with a [`crate::FramedChild`] failed. After any of these but
/// [`FramedError::PayloadTooLarge`] (refused before a byte is written) the child's group has been
/// killed and the child reaped: a stream cut mid-frame cannot be trusted again, so the caller
/// starts a new child. Pinned by `tests/framed.rs`.
#[derive(Debug, thiserror::Error)]
pub enum FramedError {
    /// The exchange did not finish before its deadline.
    #[error("the exchange with the child passed its deadline")]
    Deadline,
    /// The child declared a frame longer than the limit; nothing was allocated for it.
    #[error("the child declared a frame of {declared} bytes, over the limit of {max}")]
    FrameTooLarge {
        /// The declared length.
        declared: u64,
        /// The limit.
        max: u32,
    },
    /// The payload to send is longer than the limit.
    #[error("a payload of {len} bytes is over the frame limit of {max}")]
    PayloadTooLarge {
        /// The payload's length.
        len: usize,
        /// The limit.
        max: u32,
    },
    /// The child closed its end (it exited, or closed stdin or stdout); how it ended.
    #[error("the child ended the stream: {0:?}")]
    Exited(Exit),
    /// An earlier failure already killed this child.
    #[error("the child was killed by an earlier failure")]
    Dead,
    /// A read, write or wait failed.
    #[error("talking to the child failed: {0}")]
    Io(#[source] io::Error),
}
