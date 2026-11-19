//! One bounded wait on descriptors: `poll(2)` with the time the caller has left, as `jfl-process`
//! waits (no thread, no sleep, no socket timeout: plan §6.5 grants no production crate a sleep).
//! The caller holds the clock and loops; an interrupted wait reports nothing ready.

use std::io;
use std::os::fd::BorrowedFd;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::io::Errno;

/// The longest single `poll(2)`: a longer wait is several, which keeps the `Timespec` in range.
const MAX_POLL: Duration = Duration::from_secs(3600);

/// Readable (or hung up).
pub(crate) const IN: PollFlags = PollFlags::IN;
/// Writable (or hung up).
pub(crate) const OUT: PollFlags = PollFlags::OUT;

/// Waits up to `wait` until one of `fds` is ready for its events (or hung up); says which.
pub(crate) fn ready<const N: usize>(
    fds: [Option<(BorrowedFd<'_>, PollFlags)>; N],
    wait: Duration,
) -> io::Result<[bool; N]> {
    let mut polls = Vec::with_capacity(N);
    let mut slots = Vec::with_capacity(N);
    for (slot, entry) in fds.iter().enumerate() {
        if let Some((fd, flags)) = entry {
            polls.push(PollFd::from_borrowed_fd(*fd, *flags));
            slots.push(slot);
        }
    }
    let timeout = Timespec::try_from(wait.min(MAX_POLL))
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut out = [false; N];
    match rustix::event::poll(&mut polls, Some(&timeout)) {
        Ok(_) => {}
        Err(Errno::INTR) => return Ok(out),
        Err(error) => return Err(error.into()),
    }
    for (poll, slot) in polls.iter().zip(slots) {
        if !poll.revents().is_empty()
            && let Some(flag) = out.get_mut(slot)
        {
            *flag = true;
        }
    }
    Ok(out)
}
