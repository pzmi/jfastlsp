//! The OS calls the modes share: the started leader and its group, the wait on pipes and the
//! leader's exit, and the platform's exit descriptor (`#[cfg]` items, never a `cfg!` branch).

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ExitStatus};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};

use crate::run::Exit;

/// The longest slice of a wait between two checks of the leader where the platform has no exit
/// descriptor (not Linux, or a Linux without `pidfd_open`): the delay with which a leader's exit
/// is seen while a descendant still holds its pipes, or after it closed them.
pub const EXIT_RECHECK: Duration = Duration::from_millis(5);

/// The longest single `poll(2)`: a deadline past it is waited for in several slices, which keeps
/// the `Timespec` conversion in range whatever the caller passes.
const MAX_POLL: Duration = Duration::from_secs(3600);

/// A started child, the leader of its own process group (`process_group(0)`). Dropping it before
/// it is reaped kills the group with `SIGKILL` and reaps the leader.
///
/// The group is killed only while the leader is unreaped: a zombie leader keeps its pid, so the
/// group id cannot have been reused by an unrelated group. Pinned by
/// `tests/run.rs::should_leave_no_grandchild_when_the_leader_exits_first`.
#[derive(Debug)]
pub(crate) struct Leader {
    child: Child,
    reaped: Option<ExitStatus>,
}

impl Leader {
    pub(crate) fn new(child: Child) -> Self {
        Self {
            child,
            reaped: None,
        }
    }

    pub(crate) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    /// Sends `SIGKILL` to the whole group and to the leader itself, unless the leader is already
    /// reaped. The leader is signalled by pid as well because it may have left its group
    /// (`setpgid` into another one), and the group kill alone would then miss it and leave
    /// [`Leader::kill_and_reap`] blocked in `wait`; unreaped, its pid cannot have been reused.
    /// A group or leader already gone (`ESRCH`) is the expected race, not an error. Pinned by
    /// `tests/run.rs::should_kill_the_leader_at_the_deadline_when_it_left_its_group` and
    /// `tests/framed.rs::should_kill_the_leader_when_it_left_its_group_and_misses_a_deadline`.
    pub(crate) fn kill_group(&self) {
        if self.reaped.is_none() {
            let pid = pid_of(&self.child);
            // ESRCH: already exited; EPERM cannot happen for our own children.
            let _ignored = rustix::process::kill_process_group(pid, Signal::KILL);
            let _ignored = rustix::process::kill_process(pid, Signal::KILL);
        }
    }

    /// Whether the leader has exited, without reaping it (`waitid` with `WNOWAIT`).
    pub(crate) fn has_exited(&self) -> io::Result<bool> {
        if self.reaped.is_some() {
            return Ok(true);
        }
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        loop {
            match rustix::process::waitid(WaitId::Pid(pid_of(&self.child)), options) {
                Ok(status) => return Ok(status.is_some()),
                Err(Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Kills what is left of the group, then reaps the leader (blocking until it is gone, which
    /// `SIGKILL` makes prompt).
    pub(crate) fn kill_and_reap(&mut self) -> io::Result<Exit> {
        self.kill_group();
        self.reap()
    }

    fn reap(&mut self) -> io::Result<Exit> {
        if let Some(status) = self.reaped {
            return Ok(Exit::from_status(status));
        }
        let status = self.child.wait()?;
        self.reaped = Some(status);
        Ok(Exit::from_status(status))
    }
}

impl Drop for Leader {
    fn drop(&mut self) {
        if self.reaped.is_none() {
            self.kill_group();
            let _ignored = self.child.wait();
        }
    }
}

fn pid_of(child: &Child) -> Pid {
    Pid::from_child(child)
}

impl Exit {
    pub(crate) fn from_status(status: ExitStatus) -> Self {
        match (status.code(), status.signal()) {
            (Some(code), _) => Self::Code(code),
            (None, Some(signal)) => Self::Signal(signal),
            (None, None) => Self::Other(status.into_raw()),
        }
    }
}

/// A descriptor that becomes readable when `child` exits: a pidfd on Linux, `None` when the
/// kernel refuses it (before 5.3).
#[cfg(target_os = "linux")]
pub(crate) fn exit_descriptor(child: &Child) -> Option<OwnedFd> {
    rustix::process::pidfd_open(pid_of(child), rustix::process::PidfdFlags::empty()).ok()
}

/// No safe exit descriptor here (`kevent` is `unsafe` in rustix): waits are cut into slices of
/// [`EXIT_RECHECK`] instead.
#[cfg(not(target_os = "linux"))]
pub(crate) fn exit_descriptor(_child: &Child) -> Option<OwnedFd> {
    None
}

/// How long one wait may last: all of `remaining` with an exit descriptor, else a slice of it.
pub(crate) fn slice(remaining: Duration, exit: Option<&OwnedFd>) -> Duration {
    match exit {
        Some(_) => remaining,
        None => remaining.min(EXIT_RECHECK),
    }
}

/// Waits up to `wait` until one of `fds` is ready for `events` (or hung up), and says which. An
/// interrupted wait reports nothing ready; the caller's loop recomputes its deadline.
pub(crate) fn wait_ready<const N: usize>(
    fds: [Option<BorrowedFd<'_>>; N],
    events: [PollFlags; N],
    wait: Duration,
) -> io::Result<[bool; N]> {
    let mut polls = Vec::with_capacity(N);
    let mut slots = Vec::with_capacity(N);
    for (slot, (fd, flags)) in fds.iter().zip(events).enumerate() {
        if let Some(fd) = fd {
            polls.push(PollFd::from_borrowed_fd(*fd, flags));
            slots.push(slot);
        }
    }
    let timeout = Timespec::try_from(wait.min(MAX_POLL))
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut ready = [false; N];
    match rustix::event::poll(&mut polls, Some(&timeout)) {
        Ok(_) => {}
        Err(Errno::INTR) => return Ok(ready),
        Err(error) => return Err(error.into()),
    }
    for (poll, slot) in polls.iter().zip(slots) {
        if !poll.revents().is_empty()
            && let Some(flag) = ready.get_mut(slot)
        {
            *flag = true;
        }
    }
    Ok(ready)
}

/// `fd`, borrowed, if present.
pub(crate) fn borrow<T: AsFd>(fd: Option<&T>) -> Option<BorrowedFd<'_>> {
    fd.map(AsFd::as_fd)
}
