//! The state directory: the access control (plan §5.3).
//!
//! It is created with `DirBuilderExt::mode(0o700)` (then set to exactly `0700`, since the umask
//! filters the creation mode). An existing one is used only if it is a real directory (not a
//! symlink, which `symlink_metadata` does not follow) owned by the expected uid with mode exactly
//! `0700`; anything else is refused, naming the path and what is wrong. No other user can then
//! traverse it, which is what makes the window between a socket's bind and its `chmod` harmless.
//!
//! Pinned by `tests/endpoint.rs` (the state directory tests).

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The state directory's mode.
pub const DIR_MODE: u32 = 0o700;
/// The mode of the lock files and the socket.
pub const FILE_MODE: u32 = 0o600;
/// The owner's lock (plan §5.2): never removed.
pub const LOCK_FILE: &str = "daemon.lock";
/// The daemon's socket.
pub const SOCKET_FILE: &str = "sock";
/// The proxies' spawn lock: never removed.
pub const SPAWN_LOCK_FILE: &str = "spawn.lock";
/// The prefix of a readiness handshake's socket, `n-<token>`, the token 12 lowercase hex digits
/// (a fixed width, held to `sun_path`'s limit like `sock`), bound by the proxy holding the spawn
/// lock: one path per spawn, so a notice from a daemon of an earlier, abandoned spawn reaches no
/// later spawner (its path is gone: the send fails and is ignored).
pub const NOTIFY_PREFIX: &str = "n-";
/// The bytes of a notify socket's name: the longest name the state directory holds.
pub const NOTIFY_NAME_LEN: usize = 14;
/// The tokens that fit the name: 48 bits.
pub const NOTIFY_TOKEN_MASK: u64 = 0xffff_ffff_ffff;
/// The spawned daemon's stdout and stderr.
pub const LOG_FILE: &str = "daemon.log";

/// Why a state directory is refused. Each names the path.
#[derive(Debug, thiserror::Error)]
pub enum StateDirError {
    /// It could not be created or inspected.
    #[error("state directory {}: {source}", path.display())]
    Io {
        /// The directory.
        path: PathBuf,
        /// What failed.
        source: io::Error,
    },
    /// Something other than a directory is there (a symlink, a file).
    #[error("state directory {} is not a directory (a symlink or another file): refused", path.display())]
    NotADirectory {
        /// The path.
        path: PathBuf,
    },
    /// Another user owns it.
    #[error("state directory {} is owned by uid {owner}, not by this user (uid {uid}): refused", path.display())]
    Owner {
        /// The directory.
        path: PathBuf,
        /// Its owner.
        owner: u32,
        /// The uid it must have.
        uid: u32,
    },
    /// Its mode is not exactly `0700`.
    #[error("state directory {} has mode {mode:04o}, not 0700: refused (chmod 700 it, or remove it)", path.display())]
    Mode {
        /// The directory.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
}

/// A state directory that passed the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDir {
    path: PathBuf,
}

impl StateDir {
    /// Opens `path` as the state directory of this uid, creating it (and its parents) when absent.
    pub fn open(path: &Path) -> Result<Self, StateDirError> {
        Self::open_for(path, crate::current_uid())
    }

    /// Opens `path` as the state directory of `uid` (a test passes another uid to see the owner
    /// refusal without a second account).
    pub fn open_for(path: &Path, uid: u32) -> Result<Self, StateDirError> {
        let io = |source| StateDirError::Io {
            path: path.to_owned(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(io)?;
        }
        match DirBuilder::new().mode(DIR_MODE).create(path) {
            Ok(()) => fs::set_permissions(path, Permissions::from_mode(DIR_MODE)).map_err(io)?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io(e)),
        }
        let meta = fs::symlink_metadata(path).map_err(io)?;
        if !meta.file_type().is_dir() {
            return Err(StateDirError::NotADirectory {
                path: path.to_owned(),
            });
        }
        if meta.uid() != uid {
            return Err(StateDirError::Owner {
                path: path.to_owned(),
                owner: meta.uid(),
                uid,
            });
        }
        let mode = meta.mode() & 0o7777;
        if mode != DIR_MODE {
            return Err(StateDirError::Mode {
                path: path.to_owned(),
                mode,
            });
        }
        Ok(Self {
            path: path.to_owned(),
        })
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `daemon.lock`.
    #[must_use]
    pub fn lock_path(&self) -> PathBuf {
        self.path.join(LOCK_FILE)
    }

    /// `sock`.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.path.join(SOCKET_FILE)
    }

    /// `spawn.lock`.
    #[must_use]
    pub fn spawn_lock_path(&self) -> PathBuf {
        self.path.join(SPAWN_LOCK_FILE)
    }

    /// `n-<token>`: one spawn's notify socket, the token's low 48 bits in 12 hex digits.
    #[must_use]
    pub fn notify_path(&self, token: u64) -> PathBuf {
        self.path
            .join(format!("{NOTIFY_PREFIX}{:012x}", token & NOTIFY_TOKEN_MASK))
    }

    /// `daemon.log`.
    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        self.path.join(LOG_FILE)
    }

    /// Removes every notify socket in the directory (`n-` and 12 lowercase hex digits) and says
    /// how many. Only the spawn lock's holder binds one, and it removes its own before it lets the
    /// lock go, so one found by the next holder was left by a spawner that died while it waited:
    /// call this only under the spawn lock, before binding a new one. Pinned by
    /// `jfl-composition`'s
    /// `tests/daemon.rs::should_remove_a_notify_socket_left_by_a_dead_spawner_when_it_spawns`.
    pub fn remove_notify_sockets(&self) -> io::Result<usize> {
        let mut removed = 0_usize;
        for entry in fs::read_dir(&self.path)? {
            let name = entry?.file_name();
            let Some(token) = name
                .to_str()
                .and_then(|name| name.strip_prefix(NOTIFY_PREFIX))
            else {
                continue;
            };
            let shaped = token.len() == NOTIFY_NAME_LEN.saturating_sub(NOTIFY_PREFIX.len())
                && token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !shaped {
                continue;
            }
            match fs::remove_file(self.path.join(&name)) {
                Ok(()) => removed = removed.saturating_add(1),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(removed)
    }
}
