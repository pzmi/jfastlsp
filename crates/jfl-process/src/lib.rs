//! The only child-process runner: argv only, cleared environment plus an allowlist, deadline, own
//! process group, bounded output. Role: support.
//!
//! Every child of the product starts here (plan §6.2, §6.5; fnd §2.4): the git, javac and Gradle
//! adapters and composition's daemon launcher name no `std::process`. Three modes share one
//! [`Command`] and its refusals:
//!
//! - [`Runner::run`]: a one-shot child with a deadline and captured, capped output
//!   ([`Outcome`]);
//! - [`Runner::start_framed`]: a long-lived child spoken to in length-prefixed frames over its
//!   stdin and stdout ([`FramedChild`], the JVM helpers);
//! - [`Runner::spawn_detached`]: a child left running with its output in a caller-opened log,
//!   free to call `setsid` ([`Detached`], the daemon launcher, ADR 0021 as amended).
//!
//! What every mode guarantees:
//!
//! - **argv only.** The program and its arguments reach `execve` as given; no shell, no string
//!   splitting, no interpolation. Pinned by
//!   `tests/run.rs::should_deliver_every_argument_intact_when_it_holds_spaces_and_quotes`.
//! - **A cleared environment plus an allowlist** (`ARCH-child-environment`): the child sees only
//!   the variables its [`Command`] sets or names for inheritance from the injected [`EnvSource`].
//!   Pinned by `tests/run.rs::should_pass_only_the_allowlist_when_the_parent_has_other_variables`
//!   and the `process_runner` contract suite.
//! - **Its own process group** (one-shot and framed modes), killed as a group with `SIGKILL` on
//!   the deadline, after the leader exits, and when a [`FramedChild`] is dropped; the leader is
//!   also killed by its pid, so it dies even if it left the group. Pinned by
//!   `tests/run.rs::should_kill_the_group_at_the_deadline_when_the_child_ignores_sigterm`,
//!   `tests/run.rs::should_leave_no_grandchild_when_the_deadline_kills_the_group` and
//!   `tests/run.rs::should_kill_the_leader_at_the_deadline_when_it_left_its_group`. A descendant
//!   that leaves the group (`setsid`, `setpgid`) is out of reach, as with any group kill.
//! - **Bounded output**: stdout and stderr are each kept up to a byte limit; the excess is read,
//!   counted and dropped, never buffered ([`Captured`]).
//!
//! No thread is spawned and nothing sleeps: one `poll(2)` waits on both pipes and, on Linux, a
//! pidfd for the leader's exit, bounded by the deadline. Elsewhere (macOS has no safe exit
//! descriptor; `kevent` is `unsafe` in rustix) the same wait is cut into slices of at most
//! [`EXIT_RECHECK`], after each of which the leader is checked.

mod command;
mod detached;
mod env;
mod error;
mod framed;
mod run;
mod sys;

pub use command::{Command, DEFAULT_OUTPUT_LIMIT, OutputLimits};
pub use detached::Detached;
pub use env::{EnvSource, ProcessEnv};
pub use error::{FramedError, Refusal, RunError};
pub use framed::{DEFAULT_MAX_FRAME, FramedChild, FramedOptions, StderrSink};
pub use run::{Captured, Ending, Exit, Outcome, Runner};
pub use sys::EXIT_RECHECK;
