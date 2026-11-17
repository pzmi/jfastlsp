//! What the integration tests share: the test child, a generous deadline, and a check that a
//! process is gone.

use std::time::Duration;

use jfl_process::{Command, Ending, Exit, Outcome, Runner};

/// The test child (`src/bin/jfl-process-test-child.rs`).
pub(crate) const CHILD: &str = env!("CARGO_BIN_EXE_jfl-process-test-child");

/// A deadline no healthy run comes near, for the runs that are not about the deadline.
pub(crate) const GENEROUS: Duration = Duration::from_secs(20);

/// Runs `command`, which must start.
pub(crate) fn run(command: &Command, deadline: Duration) -> Outcome {
    Runner::new().run(command, deadline).unwrap()
}

/// The `ps` state of `pid` (`S`, `Z`, ...), or `None` when there is no such process; run through
/// the runner itself. Any other `ps` failure fails the test rather than reading as "gone".
pub(crate) fn ps_state(pid: &str) -> Option<String> {
    let ps = Command::new("ps")
        .env("PATH", "/bin:/usr/bin")
        .args(["-o", "stat=", "-p", pid]);
    let outcome = run(&ps, GENEROUS);
    match outcome.ending {
        Ending::Exited(Exit::Code(0)) => Some(
            String::from_utf8_lossy(outcome.stdout.bytes())
                .trim()
                .to_owned(),
        ),
        Ending::Exited(Exit::Code(1)) if outcome.stdout.bytes().trim_ascii().is_empty() => None,
        other => panic!("ps failed: {other:?}, {:?}", outcome.stderr),
    }
}

/// Whether `pid` is gone, or a zombie its new parent has yet to reap.
pub(crate) fn is_gone(pid: &str) -> bool {
    ps_state(pid).is_none_or(|state| state.starts_with('Z'))
}
