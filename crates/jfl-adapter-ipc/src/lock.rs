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
//! A wait for a held lock with a deadline is a [`LockWait`]: `flock(2)` cannot be polled, so a
//! thread blocks in it and hands the lock over, and its owner waits on a [`Bell`] with the time it
//! has left (`poll(2)`, no sleep and no timed wait).
//!
//! Pinned by `tests/endpoint.rs` (the lock tests) and `lock::tests`.

use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use crate::bell::{self, Bell};
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
    /// own: only the spawn lock is waited for, and a holder that runs keeps it at most for its
    /// spawn wait, a suspended one for ever. So every caller bounds the wait: a daemon's shutdown
    /// waits for it on its blocking pool under a deadline (`jfl-composition`'s
    /// `run::SPAWN_LOCK_WAIT`); a proxy waits through a [`LockWait`].
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

/// A wait for the lock on a file another process holds: a thread blocks in `flock(2)` (which
/// cannot be polled) and hands the lock over a channel, ringing a [`Bell`]; the owner waits on the
/// bell with whatever time it has left ([`LockWait::wait`]), as often as it likes.
///
/// Dropping the wait abandons it: the thread, once it gets the lock, finds nobody to hand it to
/// and drops it, which releases it. Until then a thread stays blocked for as long as the holder
/// keeps the lock (for ever, for a suspended holder); it holds nothing but its open descriptor of
/// the lock file, close-on-exec like every other.
///
/// Pinned by `lock::tests`.
#[derive(Debug)]
pub struct LockWait {
    taken: Receiver<Result<Flock, LockError>>,
    bell: Bell,
    path: PathBuf,
}

impl LockWait {
    /// Starts waiting for the lock on `path` (created `0600` if absent).
    pub fn start(path: &Path) -> Result<Self, LockError> {
        let io = |source| LockError::Io {
            path: path.to_owned(),
            source,
        };
        let file = open(path)?;
        let (ringer, bell) = bell::bell().map_err(io)?;
        let (hand, taken) = mpsc::sync_channel(1);
        let owned = path.to_owned();
        std::thread::Builder::new()
            .name("jfl-lock-wait".to_owned())
            .spawn(move || {
                let locked = match file.lock() {
                    Ok(()) => Flock::held(file, &owned),
                    Err(source) => Err(LockError::Io {
                        path: owned,
                        source,
                    }),
                };
                // An abandoned wait's lock is dropped with the unsent message: released.
                let _handed = hand.send(locked);
                ringer.ring();
            })
            .map_err(io)?;
        Ok(Self {
            taken,
            bell,
            path: path.to_owned(),
        })
    }

    /// Waits at most `wait` (zero: only a lock taken already) for the lock: `Some` once it is
    /// held, `None` while another process still holds it. An interrupted wait may end early with
    /// `None`; the caller, which holds the clock, waits again with what it has left.
    pub fn wait(&self, wait: Duration) -> Result<Option<Flock>, LockError> {
        if let Some(taken) = self.take()? {
            return Ok(Some(taken));
        }
        self.bell.wait(None, wait).map_err(|source| LockError::Io {
            path: self.path.clone(),
            source,
        })?;
        self.take()
    }

    fn take(&self) -> Result<Option<Flock>, LockError> {
        match self.taken.try_recv() {
            Ok(taken) => taken.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(LockError::Io {
                path: self.path.clone(),
                source: io::Error::other("the wait for the lock ended without it"),
            }),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory under `/tmp`, removed when dropped (no env grant for `TMPDIR`).
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = Path::new("/tmp").join(format!("jfl-lock-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn should_hand_over_the_lock_at_once_when_its_holder_lets_go_during_the_wait() {
        // given: a holder (another open file description: flock conflicts within a process too)
        let scratch = Scratch::new("woken");
        let path = scratch.0.join("spawn.lock");
        let holder = Flock::try_acquire(&path).unwrap();
        let waiting = LockWait::start(&path).unwrap();
        let while_held = waiting.wait(Duration::ZERO).unwrap();

        // when: the holder lets go, and the owner waits far longer than the hand-over takes
        drop(holder);
        let mut taken = None;
        for _ in 0..3 {
            // An interrupted poll ends early; three tries are plenty.
            taken = waiting.wait(Duration::from_secs(60)).unwrap();
            if taken.is_some() {
                break;
            }
        }

        // then: nothing while held; the lock as soon as it was free (the bell rang: without it
        // the wait would end empty at its 60 s deadline), and it is this process's now
        assert!(while_held.is_none());
        let taken = taken.expect("the lock was handed over");
        assert_eq!(taken.id(), FileId::at(&path).unwrap().unwrap());
        assert!(matches!(
            Flock::try_acquire(&path),
            Err(LockError::Held { .. })
        ));
    }

    /// Whether a thread waits blocked in `flock(2)` on the file at `path`: `/proc/locks` lists a
    /// blocked waiter as `-> FLOCK` with the file's inode. `None` where it cannot be seen.
    #[cfg(target_os = "linux")]
    fn waiter_blocked(path: &Path) -> Option<bool> {
        let ino = fs::metadata(path).ok()?.ino();
        let suffix = format!(":{ino}");
        Some(fs::read_to_string("/proc/locks").is_ok_and(|locks| {
            locks.lines().any(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                fields.get(1) == Some(&"->")
                    && fields.get(2) == Some(&"FLOCK")
                    && fields.get(6).is_some_and(|id| id.ends_with(&suffix))
            })
        }))
    }

    /// Not observable here: the test then relies on the release alone.
    #[cfg(not(target_os = "linux"))]
    fn waiter_blocked(_path: &Path) -> Option<bool> {
        None
    }

    #[test]
    fn should_release_the_lock_it_takes_when_the_wait_was_abandoned() {
        // given: a wait for a held lock, its thread blocked in flock(2), then abandoned
        let scratch = Scratch::new("abandoned");
        let path = scratch.0.join("spawn.lock");
        let holder = Flock::try_acquire(&path).unwrap();
        let waiting = LockWait::start(&path).unwrap();
        let blocked = jfl_testkit::poll::wait_until(
            Duration::from_secs(20),
            "the waiting thread blocked in flock",
            || waiter_blocked(&path).unwrap_or(true),
        );
        drop(waiting);

        // when: the holder lets go; the abandoned thread is woken, takes the lock and drops it
        drop(holder);
        let mut probe = None;
        let released = jfl_testkit::poll::wait_until(
            Duration::from_secs(20),
            "the abandoned thread's release",
            || {
                probe = (!waiter_blocked(&path).unwrap_or(false))
                    .then(|| Flock::try_acquire(&path).ok())
                    .flatten();
                probe.is_some()
            },
        );

        // then: the thread was blocked before the holder let go, and a new holder gets the lock
        // once it is no longer waiting: the abandoned wait kept nothing
        blocked.unwrap();
        released.unwrap();
        assert!(probe.is_some());
    }
}
