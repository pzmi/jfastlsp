//! The proxy's side of the socket: std only, so a CLI call starts without a runtime (plan §5.3).
//!
//! Every wait has a deadline (IDX-12's obligation 8): the caller, which holds the clock, passes a
//! `remaining` function that says how long is left. The socket is non-blocking, and each wait for
//! it to become readable or writable is one `poll(2)` bounded by what is left (no socket timeout:
//! no production crate holds a sleep, plan §6.5); nothing left is [`RecvError::TimedOut`], which
//! the proxy treats as a reset. Frames come through `jfl-protocol`'s bounded decoder, as on the daemon
//! side.
//!
//! Pinned by `tests/endpoint.rs`.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use jfl_protocol::frame::{FrameDecoder, FrameError, encode_frame};

/// One read from the daemon.
pub const READ_CHUNK: usize = 64 * 1024;

use crate::wait::{self, IN, OUT};

/// Why a connect failed.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// No socket file at the path (`ENOENT`): no daemon.
    #[error("no daemon socket")]
    Absent,
    /// A file nobody listens on (`ECONNREFUSED`): a stale socket, or a daemon that stopped
    /// accepting.
    #[error("the daemon socket refused the connection")]
    Refused,
    /// Anything else.
    #[error(transparent)]
    Io(io::Error),
}

/// What a read gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// One frame's body.
    Frame(Vec<u8>),
    /// The daemon closed the connection (or reset it) between frames.
    Eof,
}

/// Why a read failed.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// The deadline passed.
    #[error("the deadline passed waiting for the daemon")]
    TimedOut,
    /// The framing is lost or a frame is over the limit.
    #[error(transparent)]
    Framing(FrameError),
    /// The read failed.
    #[error(transparent)]
    Io(io::Error),
}

/// A connection to the daemon.
#[derive(Debug)]
pub struct ClientConn {
    stream: UnixStream,
    decoder: FrameDecoder,
    chunk: Box<[u8]>,
    start: usize,
    end: usize,
}

impl ClientConn {
    /// Connects to the socket at `path`. A listening daemon queues the connection in its backlog
    /// at once, whether or not it accepts it yet.
    pub fn connect(path: &Path) -> Result<Self, ConnectError> {
        let stream = UnixStream::connect(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => ConnectError::Absent,
            io::ErrorKind::ConnectionRefused => ConnectError::Refused,
            _ => ConnectError::Io(e),
        })?;
        stream.set_nonblocking(true).map_err(ConnectError::Io)?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(jfl_protocol::DEFAULT_MAX_FRAME_LEN),
            chunk: vec![0; READ_CHUNK].into_boxed_slice(),
            start: 0,
            end: 0,
        })
    }

    /// Frames `body` and writes it whole, each write bounded by what `remaining` leaves.
    pub fn send(
        &mut self,
        body: &[u8],
        remaining: &mut dyn FnMut() -> Option<Duration>,
    ) -> Result<(), RecvError> {
        let frame =
            encode_frame(body, jfl_protocol::DEFAULT_MAX_FRAME_LEN).map_err(RecvError::Framing)?;
        let mut rest = frame.as_slice();
        while !rest.is_empty() {
            match self.stream.write(rest) {
                Ok(0) => return Err(RecvError::Io(io::ErrorKind::WriteZero.into())),
                Ok(n) => rest = rest.get(n..).unwrap_or_default(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let left = remaining().ok_or(RecvError::TimedOut)?;
                    wait::ready([Some((self.stream.as_fd(), OUT))], left).map_err(RecvError::Io)?;
                }
                Err(e) => return Err(RecvError::Io(e)),
            }
        }
        Ok(())
    }

    /// The next frame, or the end of the connection, each read bounded by what `remaining`
    /// leaves. A reset connection reads as [`Received::Eof`], as the model's EOF.
    pub fn recv(
        &mut self,
        remaining: &mut dyn FnMut() -> Option<Duration>,
    ) -> Result<Received, RecvError> {
        loop {
            while self.start < self.end {
                let pending = self.chunk.get(self.start..self.end).unwrap_or_default();
                let progress = self.decoder.feed(pending).map_err(RecvError::Framing)?;
                self.start = self
                    .start
                    .saturating_add(progress.consumed.max(1))
                    .min(self.end);
                if let Some(oversized) = progress.oversized {
                    return Err(RecvError::Framing(FrameError::TooLarge {
                        declared: oversized.declared,
                        limit: oversized.limit,
                    }));
                }
                if let Some(body) = progress.frame {
                    return Ok(Received::Frame(body));
                }
            }
            match self.stream.read(&mut self.chunk) {
                Ok(0) => {
                    return match self.decoder.finish() {
                        Ok(()) => Ok(Received::Eof),
                        Err(err) => Err(RecvError::Framing(err)),
                    };
                }
                Ok(read) => {
                    self.start = 0;
                    self.end = read.min(self.chunk.len());
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let left = remaining().ok_or(RecvError::TimedOut)?;
                    wait::ready([Some((self.stream.as_fd(), IN))], left).map_err(RecvError::Io)?;
                }
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return Ok(Received::Eof),
                Err(e) => return Err(RecvError::Io(e)),
            }
        }
    }
}

impl AsFd for ClientConn {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

/// Whether a daemon listens at `path`: a connect that succeeds, closed at once (the proxy's
/// second check under the spawn lock). The daemon reads the closed connection's EOF before any
/// handshake and drops it.
#[must_use]
pub fn listening(path: &Path) -> bool {
    UnixStream::connect(path).is_ok()
}
