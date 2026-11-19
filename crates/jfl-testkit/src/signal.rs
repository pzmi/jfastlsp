//! Signals to a process a test started (a daemon spawned detached, which no handle reaches once
//! its launcher dropped it): through the system's `kill` program, by pid, never by name.

use std::process::{Command, Stdio};

/// Sends `signal` (`TERM`, `KILL`, …) to `pid`; whether `kill` reported success (a process gone
/// already is a failure, not an error).
///
/// Pinned by: `signal::tests::should_report_failure_when_the_pid_names_no_process`.
#[must_use]
pub fn send(pid: u32, signal: &str) -> bool {
    Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_failure_when_the_pid_names_no_process() {
        // given: pids are below 2^22 on Linux and 99999 on macOS
        let none = 2_000_000_000;

        // when
        let sent = send(none, "TERM");

        // then
        assert!(!sent);
    }
}
