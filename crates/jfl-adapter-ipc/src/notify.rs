//! The readiness handshake (sccache's notify socket, plan §5.2, ADR 0021): the proxy that holds
//! the spawn lock binds a datagram socket at `<state>/n-<token>` (`0600`), starts the daemon
//! with that path, and waits for one [`Notice`] with a bounded budget. The daemon sends `ready`
//! once it listens, or why it failed (the lock held, the bind refused). A path, not an inherited
//! descriptor, so the daemon inherits nothing from its launcher.
//!
//! Each spawn has its own path, `n-` and a fixed-width token ([`crate::state::NOTIFY_NAME_LEN`]
//! bytes in all, checked with the socket's length when the state directory's socket path is
//! made): a daemon of an earlier spawn whose wait expired may still report, and on a shared path
//! its `ready` would reach the next spawner, which would then take a daemon still starting for one
//! that listens (found by the daemon driver's `spawnTimeout`). A file left by a proxy that died is
//! removed before a bind.
//!
//! The wait is one `poll(2)` on the datagram socket and, where the platform has one, the child's
//! exit descriptor (a pidfd), bounded by what the caller has left: no sleep, no socket timeout.
//!
//! Pinned by `tests/endpoint.rs::should_deliver_the_notice_when_the_daemon_reports_ready`.

use std::fs::{self, Permissions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::socket::SocketPath;
use crate::state::FILE_MODE;
use crate::wait::{self, IN};

/// The largest notice: a failure's reason is cut to fit.
pub const MAX_NOTICE: usize = 512;

const READY: &str = "ready ";
const FAILED: &str = "failed ";

/// What the spawned daemon reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// It listens.
    Ready {
        /// Its pid.
        pid: u32,
    },
    /// It exits 1, for this reason.
    Failed {
        /// Why.
        reason: String,
    },
}

impl Notice {
    fn encode(&self) -> Vec<u8> {
        let mut text = match self {
            Self::Ready { pid } => format!("{READY}{pid}"),
            Self::Failed { reason } => format!("{FAILED}{reason}"),
        };
        if text.len() > MAX_NOTICE {
            let mut cut = MAX_NOTICE;
            while !text.is_char_boundary(cut) {
                cut = cut.saturating_sub(1);
            }
            text.truncate(cut);
        }
        text.into_bytes()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        if let Some(pid) = text.strip_prefix(READY) {
            return pid.parse().ok().map(|pid| Self::Ready { pid });
        }
        text.strip_prefix(FAILED).map(|reason| Self::Failed {
            reason: reason.to_owned(),
        })
    }
}

/// The proxy's end: bound at the path until dropped, which removes the file.
#[derive(Debug)]
pub struct NotifyListener {
    socket: UnixDatagram,
    path: PathBuf,
}

impl NotifyListener {
    /// Removes a file left at `path` and binds it, `0600`. A path too long for `sun_path` is
    /// refused with the limit named.
    pub fn bind(path: &Path) -> io::Result<Self> {
        SocketPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let socket = UnixDatagram::bind(path)?;
        socket.set_nonblocking(true)?;
        fs::set_permissions(path, Permissions::from_mode(FILE_MODE))?;
        Ok(Self {
            socket,
            path: path.to_owned(),
        })
    }

    /// Waits at most `wait` (zero: only what is there already) for the notice or, when `exit` is
    /// given, for the child's exit descriptor to become readable. One wait: the caller, which
    /// holds the clock, loops on [`Waited::Nothing`] until its deadline. A datagram that is not a
    /// notice is skipped.
    pub fn recv(&self, wait: Duration, exit: Option<BorrowedFd<'_>>) -> io::Result<Waited> {
        if let Some(notice) = self.try_recv()? {
            return Ok(Waited::Notice(notice));
        }
        let [_, exited] = wait::ready(
            [Some((self.socket.as_fd(), IN)), exit.map(|fd| (fd, IN))],
            wait,
        )?;
        if let Some(notice) = self.try_recv()? {
            return Ok(Waited::Notice(notice));
        }
        Ok(if exited {
            Waited::Exited
        } else {
            Waited::Nothing
        })
    }

    fn try_recv(&self) -> io::Result<Option<Notice>> {
        let mut buf = [0_u8; MAX_NOTICE];
        loop {
            match self.socket.recv(&mut buf) {
                Ok(n) => {
                    if let Some(notice) = buf.get(..n).and_then(Notice::decode) {
                        return Ok(Some(notice));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }
}

/// What one notify wait saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited {
    /// The daemon's notice.
    Notice(Notice),
    /// The child's exit descriptor became readable: it died without a notice.
    Exited,
    /// Nothing within the wait.
    Nothing,
}

impl Drop for NotifyListener {
    fn drop(&mut self) {
        // The proxy's own file; a failure leaves it for the next bind to remove.
        let _ignored = fs::remove_file(&self.path);
    }
}

/// The daemon's end: sends `notice` to the proxy waiting at `path`.
pub fn send(path: &Path, notice: &Notice) -> io::Result<()> {
    let socket = UnixDatagram::unbound()?;
    socket.send_to(&notice.encode(), path).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_back_a_notice_when_it_is_encoded() {
        // given
        let notices = [
            Notice::Ready { pid: 42 },
            Notice::Failed {
                reason: "daemon.lock is held".to_owned(),
            },
        ];

        // when
        let decoded: Vec<_> = notices
            .iter()
            .map(|n| Notice::decode(&n.encode()))
            .collect();

        // then
        assert_eq!(decoded, notices.map(Some));
        assert_eq!(Notice::decode(b"ready x"), None);
        assert_eq!(Notice::decode(b"hello"), None);
    }

    #[test]
    fn should_cut_a_long_reason_when_it_is_encoded() {
        // given
        let notice = Notice::Failed {
            reason: "é".repeat(MAX_NOTICE),
        };

        // when
        let bytes = notice.encode();

        // then
        assert!(bytes.len() <= MAX_NOTICE);
        assert!(matches!(
            Notice::decode(&bytes),
            Some(Notice::Failed { .. })
        ));
    }
}
