//! A wake-up for a thread that waits on the daemon's connection and on its own work queue at once
//! (`jfl lsp`'s link, LSP-04): pushed diagnostics arrive on the connection whenever the daemon
//! has them, while the client's changes and requests arrive on a channel, and one `poll(2)` waits
//! for either. A channel cannot be polled, so whoever queues work also rings the bell.
//!
//! The bell is an anonymous Unix socket pair (std's `UnixStream::pair`): no path, so nothing else
//! can reach it, and no new grant (sockets stay in this crate, plan §6.5). Both ends are
//! non-blocking: a ring never waits (a full buffer means it rang already), and a wait reads every
//! ring it finds, so one wait answers any number of rings.
//!
//! Pinned by `bell::tests`.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use crate::wait::{self, IN};

/// The ringing end, shared by everyone who queues work.
#[derive(Debug, Clone)]
pub struct Ringer(Arc<UnixStream>);

/// The waiting end, held by the one thread that waits.
#[derive(Debug)]
pub struct Bell {
    rx: UnixStream,
}

/// What a wait found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Woken {
    /// The bell rang (once or more) since the last wait.
    pub rang: bool,
    /// The other descriptor is readable (or hung up).
    pub readable: bool,
}

/// A new bell and its ringer.
pub fn bell() -> io::Result<(Ringer, Bell)> {
    let (tx, rx) = UnixStream::pair()?;
    tx.set_nonblocking(true)?;
    rx.set_nonblocking(true)?;
    Ok((Ringer(Arc::new(tx)), Bell { rx }))
}

impl Ringer {
    /// Rings: the waiting thread wakes, now or at its next wait. Never blocks; a bell that cannot
    /// take another byte has rung already, and one whose waiter is gone has no one to wake.
    pub fn ring(&self) {
        let _rung = (&*self.0).write(&[1]);
    }
}

impl Bell {
    /// Waits at most `wait` until the bell rings or `also` becomes readable, and takes every ring
    /// waiting. An interrupted wait reports nothing.
    pub fn wait(&self, also: Option<BorrowedFd<'_>>, wait: Duration) -> io::Result<Woken> {
        let [rang, readable] =
            wait::ready([Some((self.rx.as_fd(), IN)), also.map(|fd| (fd, IN))], wait)?;
        if rang {
            self.drain()?;
        }
        Ok(Woken { rang, readable })
    }

    fn drain(&self) -> io::Result<()> {
        let mut buf = [0_u8; 64];
        loop {
            match (&self.rx).read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_wake_once_for_many_rings_and_then_wait_when_rung_before_the_wait() {
        // given
        let (ringer, bell) = bell().unwrap();
        for _ in 0..1000 {
            ringer.clone().ring();
        }

        // when
        let first = bell.wait(None, Duration::from_secs(5)).unwrap();
        let second = bell.wait(None, Duration::ZERO).unwrap();

        // then
        assert_eq!(
            first,
            Woken {
                rang: true,
                readable: false
            }
        );
        assert_eq!(second, Woken::default());
    }

    #[test]
    fn should_report_the_other_descriptor_when_it_becomes_readable() {
        // given
        let (_ringer, bell) = bell().unwrap();
        let (mut a, b) = UnixStream::pair().unwrap();
        a.write_all(b"x").unwrap();

        // when
        let woken = bell.wait(Some(b.as_fd()), Duration::from_secs(5)).unwrap();

        // then
        assert_eq!(
            woken,
            Woken {
                rang: false,
                readable: true
            }
        );
    }
}
