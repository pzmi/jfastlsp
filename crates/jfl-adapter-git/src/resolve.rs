//! The innermost worktree of a path (plan §4.1, §4.2, ADR 0017).
//!
//! A request's directory is canonicalised, then each directory from it upwards is asked for a
//! `.git` entry with one `lstat`; the first that has one, of any type and wherever a link points,
//! is the candidate root (`jfl_application::source_tree::nested_root`, the rule the listing skips
//! nested roots by, so a file the parent view never lists never resolves to the parent). The walk
//! crosses filesystem boundaries, where Git's own discovery stops by default
//! (`GIT_DISCOVERY_ACROSS_FILESYSTEM`): a worktree on another mount below a checkout is found
//! here, and Git confirms it by its own path (`-C <candidate>`). The walk is bounded by the depth of
//! the canonical path.
//!
//! Before Git is asked, the candidate must look like a worktree Git could open (`precheck`): a
//! `.git` directory, or a link to one, holding a regular `HEAD`, or a `.git` file whose `gitdir:`
//! names a directory holding one. Anything else (a FIFO `.git`, a dangling link, a gitdir that is
//! gone, a FIFO `HEAD` that would stall Git until its deadline) is refused with
//! [`RepoProbeError::NotAWorktree`] without a spawn. The check is a stat or two and runs on every
//! request that has no confirmed worktree, so a repository repaired later is seen at once.
//!
//! Git then confirms the candidate (`rev-parse`: its top level must be the candidate, and it names
//! `$GIT_DIR` and the common directory). The answer is cached per root under the `.git` entry's
//! identity ([`GitKey`]):
//!
//! - a worktree, and a refusal that the repository's structure decides (reftable, a top level
//!   elsewhere, a path with no URI, malformed output), are kept until the key changes;
//! - a refusal that may pass (a deadline, Git exiting with an error or killed by a signal, which
//!   load, the OOM killer or a full descriptor table cause as readily as a broken repository) is
//!   retried with a backoff counted in requests ([`Backoff`]): the adapter holds no clock (fnd §2.4),
//!   and a key that every `git add` leaves alone would otherwise refuse the root for the daemon's
//!   lifetime;
//! - what the environment decides (git missing, an I/O error) is not cached at all.
//!
//! The walk itself is not cached. A `git init` or `git worktree add` inside a directory already
//! resolved makes that directory a new innermost root, and nothing announces it: the watcher
//! leaves `.git` paths out of its hints. A cached directory-to-root answer would keep binding
//! requests there to the parent, which skips the new worktree when it scans (plan §4.2). The walk
//! costs one `lstat` per directory level (the corpus numbers are in `doc/bench/README.md`), so it
//! is redone on every call. Pinned by
//! `tests/worktrees.rs::should_resolve_to_a_new_nested_repository_when_it_appears_after_a_resolve`.

use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use jfl_application::source_tree::nested_root;
use jfl_application::{GitMarker, GitStep, RepoProbeError, StatRecord, Worktree, WorktreeKind};
use jfl_domain::FileUri;
use jfl_process::EnvSource;

use crate::cli::Git;
use crate::snapshot::{read_small, record};
use crate::{GitConfig, map_io, parse};

/// The most roots the cache holds, confirmed and refused together; past it, the cache starts
/// again empty. Far past the number of worktrees one daemon serves.
pub const MAX_CACHED_ROOTS: usize = 4096;

/// The most `rev-parse` output a confirmation reads: three paths of at most `PATH_MAX`.
const CONFIRM_OUTPUT: usize = 3 * 4096 + 3;

/// What identifies a `.git` entry for the cache: when it changes, the root is confirmed again.
///
/// - **A directory**, or a link to one, is keyed by the directory's device and inode plus the stat
///   of its `config` (where `core.worktree` and `core.bare` would move the top level). Never by its
///   own times: every `git add`, commit or refreshing `status` writes a lock file into it and moves
///   them, and the request after each would pay a spawn. Pinned by
///   `tests/worktrees.rs::should_spawn_no_confirmation_when_the_user_runs_git_after_a_resolve`.
/// - **A file** (a linked worktree's or submodule's `gitdir:` pointer) is keyed by its whole
///   `lstat`: Git rewrites it only when the worktree is moved or repaired.
/// - **A link** is keyed by its own `lstat` too, so pointing it elsewhere re-confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GitKey {
    marker: GitMarker,
    /// The `.git` entry's own `lstat` (for a directory, its device and inode only).
    own: (u64, u64, Option<StatRecord>),
    /// What the entry leads to, followed: its device and inode, and its `config` when it is a
    /// directory.
    target: Option<(u64, u64, Option<StatRecord>)>,
}

impl GitKey {
    fn of(git: &Path, meta: &Metadata, marker: GitMarker) -> Self {
        let own_stat = (marker != GitMarker::Dir).then(|| record(meta));
        let target = match marker {
            GitMarker::Dir | GitMarker::Symlink => fs::metadata(git).ok().map(|followed| {
                let config = followed
                    .is_dir()
                    .then(|| fs::metadata(git.join("config")).ok().map(|m| record(&m)))
                    .flatten();
                (followed.dev(), followed.ino(), config)
            }),
            GitMarker::File | GitMarker::Other => None,
        };
        Self {
            marker,
            own: (meta.dev(), meta.ino(), own_stat),
            target,
        }
    }
}

/// The longest backoff, in requests: past a run of transient failures Git is asked again at least
/// once every this many requests.
pub const MAX_BACKOFF: u32 = 1024;

/// When to ask Git again after a transient refusal, counted in requests (the adapter holds no
/// clock). With Git failing every time, the requests that ask are the 1st, 2nd, 4th, 8th, … after
/// the first failure: the gap between two asks doubles after each failure, up to [`MAX_BACKOFF`]
/// requests; the requests in between are refused with the last error, without a spawn.
///
/// Pinned by: `resolve::tests::should_ask_again_on_a_doubling_schedule_when_failures_repeat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// The refusal to answer with while waiting.
    pub error: RepoProbeError,
    /// Requests still to refuse before asking again.
    pub remaining: u32,
    /// The gap to the next ask after the next failure.
    pub wait: u32,
}

impl Backoff {
    /// The state after a first transient failure.
    #[must_use]
    pub fn first(error: RepoProbeError) -> Self {
        Self {
            error,
            remaining: 0,
            wait: 1,
        }
    }

    /// The state after one more failure, asked when the wait ran out.
    #[must_use]
    pub fn failed_again(self, error: RepoProbeError) -> Self {
        Self {
            error,
            remaining: self.wait.saturating_sub(1),
            wait: self.wait.saturating_mul(2).min(MAX_BACKOFF),
        }
    }

    /// One request: `Some(refusal)` while the wait lasts (counting this request), `None` when Git
    /// is to be asked.
    pub fn take(&mut self) -> Option<RepoProbeError> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        Some(self.error)
    }
}

/// What the cache holds for a root.
#[derive(Debug, Clone)]
enum Entry {
    /// A worktree, or a refusal its structure decides: kept until the key changes.
    Decided(Result<Worktree, RepoProbeError>),
    /// A refusal that may pass.
    Waiting(Backoff),
}

/// Confirmed and refused roots, by root.
#[derive(Debug, Default)]
pub struct Roots {
    cache: Mutex<HashMap<PathBuf, (GitKey, Entry)>>,
}

impl Roots {
    /// The innermost worktree holding `path`, or `None`.
    pub fn resolve<E: EnvSource>(
        &self,
        git: &Git<E>,
        config: &GitConfig,
        path: &FileUri,
    ) -> Result<Option<Worktree>, RepoProbeError> {
        let start = existing_dir(Path::new(path.path()))?;
        let Some((root, key)) = candidate(&start, config.ceiling.as_deref())? else {
            return Ok(None);
        };
        let previous = {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            match cache.get_mut(&root).filter(|(cached, _)| *cached == key) {
                Some((_, Entry::Decided(decision))) => return decision.clone().map(Some),
                Some((_, Entry::Waiting(backoff))) => match backoff.take() {
                    Some(error) => return Err(error),
                    None => Some(*backoff),
                },
                None => None,
            }
        };
        precheck(&root.join(".git"), key.marker)?;
        let decision = confirm(git, config, &root);
        let entry = match decision {
            Ok(_) => Entry::Decided(decision.clone()),
            Err(error) if structural(error) => Entry::Decided(decision.clone()),
            Err(error) if transient(error) => Entry::Waiting(match previous {
                Some(backoff) => backoff.failed_again(error),
                None => Backoff::first(error),
            }),
            Err(_) => return decision.map(Some),
        };
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= MAX_CACHED_ROOTS {
            cache.clear();
        }
        cache.insert(root, (key, entry));
        decision.map(Some)
    }
}

/// A refusal the repository's structure decides, kept until its `.git` changes.
fn structural(error: RepoProbeError) -> bool {
    matches!(
        error,
        RepoProbeError::NotAWorktree
            | RepoProbeError::ReftableUnsupported
            | RepoProbeError::Unrepresentable
            | RepoProbeError::Malformed(_)
    )
}

/// A refusal that may pass: Git past its deadline, exiting with an error, or killed by a signal.
/// Pinned by `tests/worktrees.rs::should_resolve_again_when_a_confirmation_failed_once`.
fn transient(error: RepoProbeError) -> bool {
    matches!(
        error,
        RepoProbeError::Deadline(_) | RepoProbeError::GitFailed { .. }
    )
}

/// The longest `.git` file read: a `gitdir:` line with a path of at most `PATH_MAX`.
const MAX_GITFILE: u64 = 4096 + 16;

/// Whether the candidate's `.git` looks like one Git could open, decided by a stat or two and
/// without a spawn (the module doc says why). A link is followed, as Git follows it.
fn precheck(git: &Path, marker: GitMarker) -> Result<(), RepoProbeError> {
    let refuse = Err(RepoProbeError::NotAWorktree);
    let regular = |path: &Path| fs::metadata(path).is_ok_and(|m| m.is_file());
    let followed = match marker {
        GitMarker::Other => return refuse,
        GitMarker::Dir | GitMarker::File | GitMarker::Symlink => match fs::metadata(git) {
            Ok(meta) => meta,
            Err(_) => return refuse,
        },
    };
    if followed.is_dir() {
        return if regular(&git.join("HEAD")) {
            Ok(())
        } else {
            refuse
        };
    }
    if !followed.is_file() || followed.len() > MAX_GITFILE {
        return refuse;
    }
    let Ok(Some(bytes)) = read_small(git, MAX_GITFILE) else {
        return refuse;
    };
    let Some(gitdir) = parse::gitfile(&bytes) else {
        return refuse;
    };
    let gitdir = git
        .parent()
        .map_or_else(|| gitdir.to_path_buf(), |dir| dir.join(gitdir));
    if regular(&gitdir.join("HEAD")) {
        Ok(())
    } else {
        refuse
    }
}

/// The canonical form of the deepest existing directory that holds `path` (itself, when it is a
/// directory): a request may name a file not yet written, or one under a symbolic link.
fn existing_dir(path: &Path) -> Result<PathBuf, RepoProbeError> {
    canonical_parts(path).map(|(dir, _)| dir)
}

/// `path` as the probe binds it: the deepest existing directory that holds it (itself, when it
/// is a directory) canonicalised, then the rest of `path` below that directory as written. The
/// root [`resolve`] finds for `path` is this path's innermost worktree, so a caller that binds a
/// file to that root by this path always finds it under the root (FND-17: the daemon binds a
/// request spelled through a link with it).
///
/// Pinned by: `tests/worktrees.rs::should_resolve_the_directory_and_keep_the_rest_when_a_path_goes_through_a_link`.
pub fn canonical_path(path: &Path) -> Result<PathBuf, RepoProbeError> {
    let (mut dir, rest) = canonical_parts(path)?;
    if !rest.as_os_str().is_empty() {
        dir.push(rest);
    }
    Ok(dir)
}

/// The canonical deepest existing directory holding `path`, and `path` below the directory it
/// canonicalised.
fn canonical_parts(path: &Path) -> Result<(PathBuf, PathBuf), RepoProbeError> {
    let mut dir = match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => path,
        _ => path.parent().unwrap_or(path),
    };
    loop {
        match fs::canonicalize(dir) {
            Ok(canonical) if canonical.is_dir() => {
                let rest = path
                    .strip_prefix(dir)
                    .unwrap_or(Path::new(""))
                    .to_path_buf();
                return Ok((canonical, rest));
            }
            Ok(_) => {}
            Err(error) if gone(&error) => {}
            Err(error) => return Err(map_io(&error)),
        }
        dir = dir.parent().ok_or(RepoProbeError::NotFound)?;
    }
}

/// Whether an error means "nothing there" rather than "cannot look".
fn gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// The nearest directory from `start` upwards that holds a `.git` entry of any type (links not
/// followed), stopping at `ceiling` (which is never itself a candidate).
fn candidate(
    start: &Path,
    ceiling: Option<&Path>,
) -> Result<Option<(PathBuf, GitKey)>, RepoProbeError> {
    for dir in start.ancestors() {
        if ceiling == Some(dir) {
            return Ok(None);
        }
        let git = dir.join(".git");
        let meta = match fs::symlink_metadata(&git) {
            Ok(meta) => meta,
            Err(error) if gone(&error) => continue,
            Err(error) => return Err(map_io(&error)),
        };
        let kind = meta.file_type();
        let marker = GitMarker::of(kind.is_dir(), kind.is_file(), kind.is_symlink());
        if nested_root(Some(marker)).is_some() {
            return Ok(Some((dir.to_path_buf(), GitKey::of(&git, &meta, marker))));
        }
    }
    Ok(None)
}

/// Asks Git for the candidate's top level, `$GIT_DIR` and common directory, and refuses a root
/// whose top level Git places elsewhere, or whose refs are in the reftable format.
fn confirm<E: EnvSource>(
    git: &Git<E>,
    config: &GitConfig,
    root: &Path,
) -> Result<Worktree, RepoProbeError> {
    let printed = git.run(
        GitStep::Confirm,
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
        CONFIRM_OUTPUT,
        config.confirm_deadline,
        &[],
    )?;
    let [top, git_dir, common] = parse::three_paths(&printed.stdout)
        .map_err(|_| RepoProbeError::Malformed(GitStep::Confirm))?;
    let canonical = |bytes: &[u8]| {
        let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
        fs::canonicalize(path).map_err(|error| map_io(&error))
    };
    if canonical(top)? != root {
        return Err(RepoProbeError::NotAWorktree);
    }
    let git_dir = canonical(git_dir)?;
    let common = canonical(common)?;
    if fs::metadata(common.join("reftable")).is_ok_and(|m| m.is_dir()) {
        return Err(RepoProbeError::ReftableUnsupported);
    }
    let kind = if git_dir == common {
        WorktreeKind::Main
    } else {
        WorktreeKind::Linked
    };
    Ok(Worktree {
        root: uri(root)?,
        git_dir: uri(&git_dir)?,
        common_dir: uri(&common)?,
        kind,
    })
}

/// The URI of a canonical path.
pub fn uri(path: &Path) -> Result<FileUri, RepoProbeError> {
    let text = path.to_str().ok_or(RepoProbeError::Unrepresentable)?;
    FileUri::from_path(text).map_err(|_| RepoProbeError::Unrepresentable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_ask_again_on_a_doubling_schedule_when_failures_repeat() {
        // given
        let error = RepoProbeError::Deadline(GitStep::Confirm);
        let mut backoff = Backoff::first(error);
        let mut asked = Vec::new();

        // when
        for request in 1..=2100_u32 {
            if backoff.take().is_none() {
                asked.push(request);
                backoff = backoff.failed_again(error);
            }
        }

        // then
        assert_eq!(asked, [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048]);
        let mut capped = Backoff::first(error);
        for _ in 0..40 {
            capped = capped.failed_again(error);
        }
        assert_eq!(
            (capped.wait, capped.remaining),
            (MAX_BACKOFF, MAX_BACKOFF - 1)
        );
    }
}
