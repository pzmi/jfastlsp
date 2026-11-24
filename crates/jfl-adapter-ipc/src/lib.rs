//! The Unix socket endpoint (server and client), the state-directory lock, peer checks and
//! framing. Role: adapter.
//!
//! The daemon's IPC of plan §5.2 and §5.3 (ADR 0016, ADR 0021), one module per resource:
//!
//! - [`state`]: the state directory, the access control: created `0700`, an existing one refused
//!   unless it is a real directory of this uid with mode exactly `0700`, the refusal naming it.
//! - [`lock`]: the single owner's lock and the proxies' spawn lock, `flock`s through std's
//!   `File::try_lock`, files `0600`, never removed; a pid file is never trusted (ADR 0021). The
//!   owner checks that the lock path still names its locked inode ([`lock::Flock::still_named`]).
//! - [`socket`]: the socket path and its length limit, refused with the limit named.
//! - [`server`]: the daemon's listener (Tokio): bound, then `0600`, each peer's uid checked
//!   against the daemon's; connections read through `jfl-protocol`'s bounded decoder and
//!   classified as `lsp_server::Message`s; results written by the caller on the typed path.
//! - [`client`]: the proxy's connection (std only, so a CLI call starts fast), every wait under a
//!   deadline the caller computes.
//! - [`bell`]: an anonymous socket pair that wakes a thread waiting on a connection and on its
//!   own work queue at once (`jfl lsp`'s link, for pushed diagnostics).
//! - [`notify`]: sccache's readiness handshake: the proxy binds a datagram socket in the state
//!   directory and the spawned daemon reports `ready` or why it failed.
//!
//! Every descriptor is close-on-exec: std and Tokio open every file and socket with `O_CLOEXEC`
//! or `SOCK_CLOEXEC`, and nothing here clears it, so no child inherits the lock, the listener or
//! a connection (IDX-12's obligation 2). Pinned by
//! `tests/endpoint.rs::should_open_every_descriptor_close_on_exec_when_the_daemon_side_is_up`.
//! The notify socket is a path, not an inherited descriptor, so there is no exception.

pub mod bell;
pub mod client;
pub mod lock;
pub mod message;
pub mod notify;
pub mod server;
pub mod socket;
pub mod state;
mod wait;

pub use bell::{Bell, Ringer, Woken};
pub use client::{ClientConn, ConnectError, Received, RecvError};
pub use lock::{FileId, Flock, LockError};
pub use message::{Call, End, Incoming, Malformed, Note};
pub use notify::{Notice, NotifyListener};
pub use server::{Accepted, ConnReader, ConnWriter, Listener};
pub use socket::{PathTooLong, SocketPath};
pub use state::{StateDir, StateDirError};

/// The uid this process runs as (its effective uid): the owner the state directory must have and
/// the uid every peer must have.
#[must_use]
pub fn current_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}
