//! The owner's lock and the spawn lock: an `flock` on a file in the state directory, through std's
//! `File::try_lock` (ADR 0021; no pid file is trusted, and no lock crate, plan §16.3).
//!
//! The kernel drops an `flock` when the last descriptor of its open file description closes, so
//! a dead process never holds one, and the file is never removed (removing it would let a second
//! daemon lock a new inode at the path while the first holds the old one: the model's
//! `lockBreakingStale`). For the same reason the owner checks, after locking and on every idle
//! tick, that the path still names the inode it locked ([`Flock::still_named`]): a lock file or
//! state directory deleted from outside makes it shut down (IDX-12's obligation 1).
//!
//! Pinned by `tests/endpoint.rs` (the lock tests).

use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::state::FILE_MODE;

/// A file's identity: its device and inode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId {
    /// The device.
    pub dev: u64,
    /// The inode.
    pub ino: u64,
}

impl FileId {
    /// The identity in `meta`.
    #[must_use]
    pub fn of(meta: &fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }

    /// The identity of the file `path` names now (not following a symlink), or `None` when
    /// nothing is there.
    pub fn at(path: &Path) -> io::Result<Option<Self>> {
        match fs::symlink_metadata(path) {
            Ok(meta) => Ok(Some(Self::of(&meta))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Why a lock was not taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds it.
    #[error("{} is held by another process", path.display())]
    Held {
        /// The lock file.
        path: PathBuf,
    },
    /// The file could not be opened or locked.
    #[error("{}: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// What failed.
        source: io::Error,
    },
}

/// A held `flock`. Dropping it closes the descriptor, which releases the lock; the file stays.
#[derive(Debug)]
pub struct Flock {
    file: File,
    path: PathBuf,
    id: FileId,
}

impl Flock {
    /// Takes the lock on `path` (created `0600` if absent) without waiting.
    pub fn try_acquire(path: &Path) -> Result<Self, LockError> {
        let file = open(path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::Held {
                    path: path.to_owned(),
                });
            }
            Err(TryLockError::Error(source)) => {
                return Err(LockError::Io {
                    path: path.to_owned(),
                    source,
                });
            }
        }
        Self::held(file, path)
    }

    /// Takes the lock on `path`, waiting while another process holds it, with no deadline of its
    /// own: only the spawn lock is waited for. A proxy waits for it unbounded (a holder that runs
    /// keeps it at most for its spawn wait; a suspended one, for ever); a daemon's shutdown waits
    /// for it on its blocking pool under a deadline (`jfl-composition`'s `run::SPAWN_LOCK_WAIT`).
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let file = open(path)?;
        file.lock().map_err(|source| LockError::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::held(file, path)
    }

    fn held(file: File, path: &Path) -> Result<Self, LockError> {
        let id = file
            .metadata()
            .map(|meta| FileId::of(&meta))
            .map_err(|source| LockError::Io {
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            file,
            path: path.to_owned(),
            id,
        })
    }

    /// The locked file's identity, from its descriptor.
    #[must_use]
    pub const fn id(&self) -> FileId {
        self.id
    }

    /// The lock file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the path still names the locked file: `false` once the file or its directory was
    /// removed or replaced from outside, when a second daemon could lock a new file there.
    pub fn still_named(&self) -> io::Result<bool> {
        Ok(FileId::at(&self.path)? == Some(self.id))
    }
}

impl AsFd for Flock {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

fn open(path: &Path) -> Result<File, LockError> {
    let io = |source| LockError::Io {
        path: path.to_owned(),
        source,
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(FILE_MODE)
        .open(path)
        .map_err(io)?;
    // A file left by an older build, or created under another umask, is set to 0600 too.
    file.set_permissions(Permissions::from_mode(FILE_MODE))
        .map_err(io)?;
    Ok(file)
}
