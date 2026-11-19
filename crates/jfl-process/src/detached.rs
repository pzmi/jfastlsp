//! The detached mode: the daemon launcher's (ADR 0021, amended 2026-11-17). The child stays in
//! the launcher's process group, with stdin from `/dev/null` and stdout and stderr in a log the
//! caller opened, and calls `setsid` itself to leave the launcher's session and group (so a
//! terminal's Ctrl-C or hang-up does not reach it). The launcher does not give it a group of its
//! own: `process_group(0)` would make it a group leader, and `setsid` fails with `EPERM` for a
//! group leader. Pinned by
//! `tests/detached.rs::should_let_the_child_call_setsid_when_detached` and
//! `tests/run.rs::should_refuse_setsid_to_a_group_leader_when_run`.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::process::{Child, Stdio};

use crate::command::Checked;
use crate::error::RunError;
use crate::run::{Exit, Group, failed_to_start, std_command};
use crate::sys;

/// A child left running. Unlike [`crate::FramedChild`], dropping this handle never kills it and
/// never blocks: the drop reaps it only if it has already exited. A child still running then is
/// reaped when it exits only by a later wait of the launcher's process, or by init once the
/// launcher exits, so a launcher that outlives its daemon keeps the handle and calls
/// [`Detached::try_exit`] (a daemon that failed its readiness handshake, say).
///
/// Pinned by `tests/detached.rs::should_write_to_the_log_when_detached` and
/// `tests/detached.rs::should_reap_an_exited_child_when_the_handle_is_dropped`.
#[derive(Debug)]
pub struct Detached {
    child: Child,
    exit: Option<OwnedFd>,
}

impl Detached {
    pub(crate) fn spawn(checked: &Checked, log: OwnedFd) -> Result<Self, RunError> {
        let stdout = log
            .try_clone()
            .map_err(|source| failed_to_start(checked, source))?;
        let child = std_command(checked, Group::Inherit)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|source| failed_to_start(checked, source))?;
        let exit = sys::exit_descriptor(&child);
        Ok(Self { child, exit })
    }

    /// A descriptor that becomes readable when the child exits (a pidfd on Linux), for a caller
    /// that waits on it with `poll(2)` beside its own descriptors; `None` where the platform has
    /// none (macOS), when the caller checks [`Detached::try_exit`] between bounded waits.
    /// Pinned by `tests/detached.rs::should_signal_the_exit_descriptor_when_a_detached_child_exits`.
    pub fn exit_fd(&self) -> Option<BorrowedFd<'_>> {
        self.exit.as_ref().map(AsFd::as_fd)
    }

    /// The child's pid.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Reaps the child if it has exited, without waiting.
    pub fn try_exit(&mut self) -> io::Result<Option<Exit>> {
        Ok(self.child.try_wait()?.map(Exit::from_status))
    }
}

impl Drop for Detached {
    fn drop(&mut self) {
        // Non-blocking: a running child is left to run.
        let _ignored = self.child.try_wait();
    }
}
