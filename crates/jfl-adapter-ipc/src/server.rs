//! The daemon's side of the socket (Tokio; plan §5.3).
//!
//! [`Listener::bind`] binds the path, then sets it `0600` (std's and Tokio's `bind` take no mode,
//! and the process-wide umask is racy in a threaded daemon; the window is harmless because the
//! `0700` state directory admits no other user), and records the bound file's identity so the
//! daemon can tell later whether the path still names its socket (a socket file removed from
//! under it: the model's `sweep`). Each accepted peer's uid (`SO_PEERCRED` on Linux,
//! `getpeereid` on macOS, through Tokio's `peer_cred`) must equal the daemon's: the backstop if
//! the directory check is ever wrong.
//!
//! A connection is split: the [`ConnReader`] feeds the bytes it reads to `jfl-protocol`'s bounded
//! decoder (the declared length checked before any allocation) and yields one [`Incoming`] per
//! frame; the [`ConnWriter`] frames the bodies the caller serialized.
//!
//! Every function that registers a socket with Tokio must run inside a runtime with IO enabled.
//! Pinned by `tests/endpoint.rs`.

use std::fs::{self, Permissions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::PermissionsExt;

use jfl_protocol::frame::{FrameDecoder, encode_frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::lock::FileId;
use crate::message::{End, Incoming, parse};
use crate::socket::SocketPath;
use crate::state::FILE_MODE;

/// One read from a connection.
pub const READ_CHUNK: usize = 64 * 1024;

/// The daemon's listening socket.
#[derive(Debug)]
pub struct Listener {
    inner: tokio::net::UnixListener,
    id: FileId,
    uid: u32,
}

/// What an accept gave.
#[derive(Debug)]
pub enum Accepted {
    /// A peer of the daemon's uid (the reader boxed: it holds the decoder's header buffer).
    Peer(Box<ConnReader>, ConnWriter),
    /// A peer of another uid, closed at once.
    Refused {
        /// The peer's uid.
        uid: u32,
    },
}

impl Listener {
    /// Binds `path` (which must not exist: the owner removes a stale file first), sets it `0600`
    /// and listens; peers must have `uid`.
    pub fn bind(path: &SocketPath, uid: u32) -> io::Result<Self> {
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        fs::set_permissions(path, Permissions::from_mode(FILE_MODE))?;
        let id = FileId::at(path.path())?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "the socket file vanished right after its bind",
            )
        })?;
        listener.set_nonblocking(true)?;
        let inner = tokio::net::UnixListener::from_std(listener)?;
        Ok(Self { inner, id, uid })
    }

    /// The bound socket file's identity.
    #[must_use]
    pub const fn id(&self) -> FileId {
        self.id
    }

    /// The next connection from the kernel's backlog. A peer of another uid is closed and
    /// reported, never served.
    pub async fn accept(&self) -> io::Result<Accepted> {
        let (stream, _) = self.inner.accept().await?;
        let uid = stream.peer_cred()?.uid();
        if uid != self.uid {
            return Ok(Accepted::Refused { uid });
        }
        let (read, write) = stream.into_split();
        Ok(Accepted::Peer(
            Box::new(ConnReader::new(read, jfl_protocol::DEFAULT_MAX_FRAME_LEN)),
            ConnWriter { half: write },
        ))
    }
}

impl AsFd for Listener {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.inner.as_fd()
    }
}

/// A connection's input, decoded with a limit.
#[derive(Debug)]
pub struct ConnReader {
    half: OwnedReadHalf,
    decoder: FrameDecoder,
    chunk: Box<[u8]>,
    start: usize,
    end: usize,
    ended: bool,
}

impl ConnReader {
    fn new(half: OwnedReadHalf, max_frame_len: usize) -> Self {
        Self {
            half,
            decoder: FrameDecoder::new(max_frame_len),
            chunk: vec![0; READ_CHUNK].into_boxed_slice(),
            start: 0,
            end: 0,
            ended: false,
        }
    }

    /// The next frame; after an [`Incoming::End`], always the same end. Cancel-safe between
    /// frames only: the bytes read are kept in the reader, so a dropped call loses nothing that
    /// was read.
    pub async fn next(&mut self) -> Incoming {
        if self.ended {
            return Incoming::End(End::Eof);
        }
        let incoming = self.read_frame().await;
        if matches!(incoming, Incoming::End(_)) {
            self.ended = true;
        }
        incoming
    }

    async fn read_frame(&mut self) -> Incoming {
        loop {
            while self.start < self.end {
                let pending = self.chunk.get(self.start..self.end).unwrap_or_default();
                let progress = match self.decoder.feed(pending) {
                    Ok(progress) => progress,
                    Err(err) => return Incoming::End(End::Framing(err)),
                };
                self.start = self
                    .start
                    .saturating_add(progress.consumed.max(1))
                    .min(self.end);
                if let Some(oversized) = progress.oversized {
                    return Incoming::Oversized(oversized);
                }
                if let Some(body) = progress.frame {
                    return parse(&body);
                }
            }
            match self.half.read(&mut self.chunk).await {
                Ok(0) => {
                    return Incoming::End(match self.decoder.finish() {
                        Ok(()) => End::Eof,
                        Err(err) => End::Framing(err),
                    });
                }
                Ok(read) => {
                    self.start = 0;
                    self.end = read.min(self.chunk.len());
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Incoming::End(End::Read(err.kind())),
            }
        }
    }
}

impl AsFd for ConnReader {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.half.as_ref().as_fd()
    }
}

/// A connection's output.
#[derive(Debug)]
pub struct ConnWriter {
    half: OwnedWriteHalf,
}

impl ConnWriter {
    /// Frames `body` (a message serialized by the caller) and writes it whole.
    pub async fn send(&mut self, body: &[u8]) -> io::Result<()> {
        let frame = encode_frame(body, jfl_protocol::DEFAULT_MAX_FRAME_LEN)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.half.write_all(&frame).await
    }
}
