//! Bounded reads and stats of one file under a root.
//!
//! Both follow symbolic links, at the file itself and at any directory on its path, as an editor
//! opening the path would: a `RelPath` through a linked directory can name a file outside the root.
//! The walk never lists such a path (plan §11), so only a path a client named can reach one, and
//! the daemon reads only what its own user can read. Both refuse anything but a regular file. A
//! read opens the file with `O_NONBLOCK` and checks the opened file's type before reading, so a
//! FIFO, which would block an ordinary open until a writer comes, is refused even when it replaces
//! the file between a stat and the open (a regular file ignores the flag).

use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use jfl_application::source_tree::{FileStat, MTime, ReadBytes, SourceTreeError};
use jfl_domain::RelPath;

/// Maps an I/O error to the port's error. A missing component or a component that is a file is
/// `NotFound`: either way nothing is at the path.
#[must_use]
pub fn map_io(error: &io::Error) -> SourceTreeError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => SourceTreeError::NotFound,
        io::ErrorKind::PermissionDenied => SourceTreeError::PermissionDenied,
        other => SourceTreeError::Io(other),
    }
}

/// The port's stat of a metadata record.
#[must_use]
pub fn file_stat(meta: &Metadata) -> FileStat {
    FileStat {
        size: meta.size(),
        mtime: MTime {
            secs: meta.mtime(),
            nanos: u32::try_from(meta.mtime_nsec()).unwrap_or(0),
        },
        inode: meta.ino(),
    }
}

fn regular(meta: &Metadata) -> Result<(), SourceTreeError> {
    if meta.is_file() {
        Ok(())
    } else {
        Err(SourceTreeError::NotAFile)
    }
}

/// Stats the regular file `root/path`.
///
/// Pinned by the `source_tree` contract suite (`stat_reflects_writes`) and `tests/files.rs`.
pub fn stat(root: &Path, path: &RelPath) -> Result<FileStat, SourceTreeError> {
    let meta = fs::metadata(root.join(path.as_str())).map_err(|e| map_io(&e))?;
    regular(&meta)?;
    Ok(file_stat(&meta))
}

/// Starts the kernel reading the first `limit` bytes of each regular file `root/path` into the
/// page cache (`posix_fadvise(WILLNEED)` over that range, which queues the reads and returns
/// before they finish), so a later [`read`] of at most `limit` bytes finds its pages there or on
/// their way rather than waiting on the disk alone. The bound is ours, not the kernel's: nothing
/// past what a read would take is asked for.
/// Opened as [`read`] opens (non-blocking, so a FIFO cannot block it) and closed at once;
/// anything that is not a regular file, or cannot be opened, is passed over. Elsewhere than
/// Linux it does nothing.
///
/// Pinned by the `source_tree` contract suite (`prefetch_is_only_a_hint`) and
/// `tests/files.rs::should_pass_over_a_fifo_without_blocking_when_it_is_prefetched`.
pub fn prefetch(root: &Path, paths: &[RelPath], limit: usize) {
    #[cfg(target_os = "linux")]
    for path in paths {
        let Ok(file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(root.join(path.as_str()))
        else {
            continue;
        };
        let length = core::num::NonZeroU64::new(u64::try_from(limit).unwrap_or(u64::MAX));
        if length.is_some() && file.metadata().is_ok_and(|meta| meta.is_file()) {
            let _ = rustix::fs::fadvise(&file, 0, length, rustix::fs::Advice::WillNeed);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (root, paths, limit);
}

/// Reads at most `limit` bytes of the regular file `root/path`. The buffer grows to the smaller
/// of the file's size and the limit, never to a size the caller did not allow.
///
/// Pinned by the `source_tree` contract suite (`read_bounded`, `read_refuses_what_is_not_a_file`)
/// and `tests/files.rs` (a FIFO).
pub fn read(root: &Path, path: &RelPath, limit: usize) -> Result<ReadBytes, SourceTreeError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(root.join(path.as_str()))
        .map_err(|e| map_io(&e))?;
    let meta = file.metadata().map_err(|e| map_io(&e))?;
    regular(&meta)?;
    let limit_bytes = u64::try_from(limit).unwrap_or(u64::MAX);
    let expected = usize::try_from(meta.len().min(limit_bytes)).unwrap_or(limit);
    let mut bytes = Vec::with_capacity(expected);
    file.take(limit_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| map_io(&e))?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok(ReadBytes { bytes, truncated })
}
