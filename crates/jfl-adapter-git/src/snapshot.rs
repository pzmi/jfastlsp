//! The stat set of plan §4.3, read without spawning anything (ADR 0024's hot path).
//!
//! For a main worktree `$GIT_DIR` is the common directory (`.git`); for a linked one it is
//! `worktrees/<name>/` under the common directory, outside the worktree's root, so no watcher of
//! the root sees it change. The set:
//!
//! 1. `$GIT_DIR/HEAD`: a checkout rewrites it, and so does a commit or reset while detached;
//! 2. while `HEAD` is symbolic, each loose ref it leads to (at most [`MAX_SYMREF_DEPTH`]), in the
//!    common directory or, for the per-worktree refs, in `$GIT_DIR`: a commit or `reset --soft`
//!    on a branch moves only that ref, never `HEAD` or the index;
//! 3. the common directory's `packed-refs`: a branch that is packed has no loose file, and
//!    `git pack-refs` moves it there;
//! 4. `$GIT_DIR/index`: staging, checkout, stash and a hard reset rewrite it.
//!
//! A repository whose refs are in the reftable format (`HEAD` names [`parse::REFTABLE_MARKER`]) is
//! refused ([`RepoProbeError::ReftableUnsupported`]): its stacks' `tables.list` would replace 2 and
//! 3, but no test pins that set yet (the pinned git predates the format's default).
//!
//! A file that does not exist is recorded as absent: its appearing is a change (the first
//! commit on an unborn branch, the first `git add`).
//!
//! Each file is stat'ed before it is read, so the content read is never older than the stat
//! recorded: a replacement racing the snapshot leaves the older stat, and the next snapshot
//! differs. Only a file that stat says is regular is opened, non-blocking, and checked again after
//! the open (it may have been swapped in between); reads are bounded. A FIFO or device planted as
//! `HEAD` is never opened and cannot block the request path.
//!
//! Pinned by the `repo_probe` contract suite's `snapshot_*` cases and `tests/stat_set.rs`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use jfl_application::source_tree::MTime;
use jfl_application::{
    GitFile, GitSnapshot, GitStat, HeadState, ObjectId, RepoProbeError, StatRecord, Worktree,
};
use jfl_domain::{FileUri, RelPath};

use crate::map_io;
use crate::parse::{self, REFTABLE_MARKER, RefHome};

/// How many symbolic refs a chain from `HEAD` may pass through: Git's own limit is 5.
pub const MAX_SYMREF_DEPTH: usize = 5;

/// The longest `HEAD` or loose ref read: a `ref: ` line with a long name, or an object id.
const MAX_REF_FILE: u64 = 4096;

/// The longest `packed-refs` read for [`head_commit`]: 64 MiB, about 600,000 refs; a larger one
/// leaves the commit unknown.
pub const MAX_PACKED_REFS: u64 = 64 << 20;

/// The commit `HEAD` names, read without spawning anything: a detached `HEAD`'s id, or the id the
/// branch it names holds (a loose ref, followed at most [`MAX_SYMREF_DEPTH`] times, else its line
/// in `packed-refs`). `None` when it names none: an unborn branch, or anything unreadable (a
/// `packed-refs` over [`MAX_PACKED_REFS`]). What an answer's view names as its `HEAD`; the stat set
/// ([`snapshot`]) is what freshness compares.
///
/// Pinned by: `tests/stat_set.rs::should_name_the_commit_of_head_when_it_is_detached_loose_or_packed`.
pub fn head_commit(worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
    let head = join(&worktree.git_dir, "HEAD")?;
    let mut next = read_ref_file(&head)?;
    for _ in 0..MAX_SYMREF_DEPTH {
        let name = match next {
            HeadState::Detached(id) => return Ok(Some(id)),
            HeadState::Unreadable => return Ok(None),
            HeadState::Symbolic(name) => name,
        };
        if name == REFTABLE_MARKER {
            return Err(RepoProbeError::ReftableUnsupported);
        }
        let home = match parse::ref_home(&name) {
            RefHome::GitDir => &worktree.git_dir,
            RefHome::CommonDir => &worktree.common_dir,
        };
        let loose = join(home, &name)?;
        next = match read_small(Path::new(loose.path()), MAX_REF_FILE)? {
            Some(bytes) => parse::head(&bytes),
            None => return packed(worktree, &name),
        };
    }
    Ok(None)
}

/// The id `packed-refs` holds for `name` (`<id> <name>` lines; `#` and `^` lines skipped).
fn packed(worktree: &Worktree, name: &str) -> Result<Option<ObjectId>, RepoProbeError> {
    let path = join(&worktree.common_dir, "packed-refs")?;
    let Some(bytes) = read_small(Path::new(path.path()), MAX_PACKED_REFS)? else {
        return Ok(None);
    };
    Ok(bytes.split(|b| *b == b'\n').find_map(|line| {
        let line = core::str::from_utf8(line).ok()?;
        let (id, named) = line.split_once(' ')?;
        (named.trim_end_matches('\r') == name)
            .then(|| ObjectId::parse(id))
            .flatten()
    }))
}

/// The stat set of `worktree`.
pub fn snapshot(worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
    let mut files = Vec::with_capacity(5);
    let head_path = join(&worktree.git_dir, "HEAD")?;
    let (head_stat, regular) = stat(&head_path)?;
    let head = if regular {
        read_ref_file(&head_path)?
    } else {
        HeadState::Unreadable
    };
    files.push(GitStat {
        file: GitFile::Head,
        path: head_path,
        stat: head_stat,
    });
    if let HeadState::Symbolic(name) = &head {
        if name == REFTABLE_MARKER {
            return Err(RepoProbeError::ReftableUnsupported);
        }
        refs(worktree, name, &mut files)?;
    }
    let index = join(&worktree.git_dir, "index")?;
    files.push(GitStat {
        file: GitFile::Index,
        stat: stat(&index)?.0,
        path: index,
    });
    Ok(GitSnapshot { head, files })
}

/// Items 2 and 3 of the set, for a symbolic `HEAD` naming `name`.
fn refs(worktree: &Worktree, name: &str, files: &mut Vec<GitStat>) -> Result<(), RepoProbeError> {
    let mut name = String::from(name);
    for _ in 0..MAX_SYMREF_DEPTH {
        let home = match parse::ref_home(&name) {
            RefHome::GitDir => &worktree.git_dir,
            RefHome::CommonDir => &worktree.common_dir,
        };
        let path = join(home, &name)?;
        let (ref_stat, regular) = stat(&path)?;
        let next = if regular {
            read_ref_file(&path)?
        } else {
            HeadState::Unreadable
        };
        files.push(GitStat {
            file: GitFile::Ref,
            path,
            stat: ref_stat,
        });
        match next {
            HeadState::Symbolic(target) => name = target,
            HeadState::Detached(_) | HeadState::Unreadable => break,
        }
    }
    let packed = join(&worktree.common_dir, "packed-refs")?;
    files.push(GitStat {
        file: GitFile::PackedRefs,
        stat: stat(&packed)?.0,
        path: packed,
    });
    Ok(())
}

/// `dir/name`, where `name` is a checked relative path.
fn join(dir: &FileUri, name: &str) -> Result<FileUri, RepoProbeError> {
    let name = RelPath::new(name).map_err(|_| RepoProbeError::Unrepresentable)?;
    dir.join(&name).map_err(|_| RepoProbeError::Unrepresentable)
}

/// The stat of `path`, following a symbolic link (`None` when nothing is there), and whether it
/// is a regular file.
fn stat(path: &FileUri) -> Result<(Option<StatRecord>, bool), RepoProbeError> {
    match fs::metadata(path.path()) {
        Ok(meta) => Ok((Some(record(&meta)), meta.is_file())),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok((None, false))
        }
        Err(error) => Err(map_io(&error)),
    }
}

/// The parts of a stat freshness compares.
pub fn record(meta: &fs::Metadata) -> StatRecord {
    let time = |secs: i64, nanos: i64| MTime {
        secs,
        nanos: u32::try_from(nanos).unwrap_or(0),
    };
    StatRecord {
        dev: meta.dev(),
        inode: meta.ino(),
        size: meta.size(),
        mtime: time(meta.mtime(), meta.mtime_nsec()),
        ctime: time(meta.ctime(), meta.ctime_nsec()),
    }
}

/// `HEAD` or a loose ref that stat found regular, read with a bound: [`HeadState::Unreadable`]
/// when it vanished, was swapped for something else since, or is longer than [`MAX_REF_FILE`].
fn read_ref_file(path: &FileUri) -> Result<HeadState, RepoProbeError> {
    Ok(read_small(Path::new(path.path()), MAX_REF_FILE)?
        .map_or(HeadState::Unreadable, |bytes| parse::head(&bytes)))
}

/// At most `max` bytes of a file that stat found regular, opened non-blocking and checked again
/// after the open: `None` when it vanished, is no longer a regular file, or is longer than `max`.
pub(crate) fn read_small(path: &Path, max: u64) -> Result<Option<Vec<u8>>, RepoProbeError> {
    let file = match open_nonblocking(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(&error)),
    };
    let meta = file.metadata().map_err(|error| map_io(&error))?;
    if !meta.is_file() || meta.len() > max {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(64);
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| map_io(&error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// Opens for reading without blocking on a FIFO or a device.
fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}
