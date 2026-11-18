//! The `RepoProbe` port (plan §4.1, §4.3, §4.6, §6.4): which worktree a path belongs to, the
//! repository it shares, the Git files whose stat proves `HEAD` and the index did not move, and
//! the files changed since a tree.
//!
//! Two paths, two costs (ADR 0024): [`RepoProbe::resolve`] and [`RepoProbe::snapshot`] are the
//! per-request hot path and read the filesystem only (a confirmation of a newly seen root aside);
//! [`RepoProbe::head_tree`], [`RepoProbe::changed_since`] and [`RepoProbe::tracked_java`] are the
//! reconcile path and run the `git` CLI.
//!
//! What any implementation must do is the `repo_probe` contract suite
//! (`jfl_testkit::contracts::repo_probe`), run against the testkit's scripted repository and
//! against `jfl-adapter-git`.

use core::future::Future;
use core::time::Duration;

use jfl_domain::{FileUri, RelPath};

use crate::source_tree::MTime;

/// A Git object id: 40 (SHA-1) or 64 (SHA-256) lower-case hexadecimal digits, as Git prints it.
///
/// Pinned by: `repo_probe::tests::should_accept_an_object_id_when_it_is_full_lower_case_hex`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(String);

impl ObjectId {
    /// Parses an object id, refusing anything but 40 or 64 lower-case hexadecimal digits: an id
    /// can then never be read as an option or a revision expression by the `git` it is passed to.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        ((text.len() == 40 || text.len() == 64) && text.bytes().all(hex))
            .then(|| Self(String::from(text)))
    }

    /// The id as Git prints it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A repository's identity (plan §4.6): its common directory, canonicalised. Every worktree of
/// one repository has the same id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoId(pub FileUri);

/// Which kind of worktree a root is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorktreeKind {
    /// The repository's main worktree: `$GIT_DIR` is the common directory (a `.git` directory, or
    /// a submodule's `.git` file pointing at its own repository).
    Main,
    /// A linked worktree (`git worktree add`): its `$GIT_DIR` is `worktrees/<name>/` under the
    /// common directory, outside its root.
    Linked,
}

/// One worktree: the root a view is keyed by (plan §4.1) and where its Git state lives. Every
/// path is canonical (symbolic links resolved).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Worktree {
    /// The innermost directory holding a `.git` file or directory.
    pub root: FileUri,
    /// `$GIT_DIR`: where this worktree's `HEAD` and index live.
    pub git_dir: FileUri,
    /// The common directory: objects, branches, `packed-refs`, shared by every worktree.
    pub common_dir: FileUri,
    /// Main or linked.
    pub kind: WorktreeKind,
}

impl Worktree {
    /// The repository this worktree belongs to.
    #[must_use]
    pub fn repo(&self) -> RepoId {
        RepoId(self.common_dir.clone())
    }
}

/// Which Git file a [`GitStat`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitFile {
    /// `$GIT_DIR/HEAD`: moves on checkout, and on a commit or reset while detached.
    Head,
    /// A loose ref `HEAD` points at, directly or through another symbolic ref: moves on a commit
    /// and a reset on that branch, which leave `HEAD` itself untouched.
    Ref,
    /// The common directory's `packed-refs`: where a branch lives once packed.
    PackedRefs,
    /// `$GIT_DIR/index`: moves on staging, checkout, stash and a hard reset.
    Index,
}

/// What a stat of a Git file tells freshness: enough to see the file replaced (Git writes every
/// one of these files to a lock file and renames it over the old one) or rewritten in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatRecord {
    /// The device it lives on.
    pub dev: u64,
    /// The inode: a rename over the file changes it, though a freed inode may be reused.
    pub inode: u64,
    /// Length in bytes.
    pub size: u64,
    /// Last modification, with the filesystem's precision (nanoseconds where it keeps them).
    pub mtime: MTime,
    /// Last status change: a rename or a `touch` that resets the mtime still moves it.
    pub ctime: MTime,
}

impl StatRecord {
    /// Whether an equal stat taken later may be trusted to mean "unchanged" (the racy-mtime rule
    /// of plan §4.3 and `views.qnt`): only when both times are at least `granularity` older than
    /// `observed_at`, the wall-clock time read *before* this stat was taken. A file written within
    /// the granularity of the stat's own time can be written again with the same mtime, size and
    /// (a freed inode being reused) the same inode, so an equal stat proves nothing about it.
    ///
    /// `granularity` is the coarsest timestamp step the filesystem may use: the kernel's coarse
    /// clock tick on Linux (up to 10 ms), a second on HFS+, two seconds on FAT. On a network
    /// filesystem (NFS, SMB) the times come from the server's clock, so `granularity` must also
    /// cover the skew between the server's clock and the one `observed_at` was read from; with
    /// the skew unknown, treat such a filesystem as never settled.
    ///
    /// Pinned by: `repo_probe::tests::should_call_a_stat_racy_when_it_is_within_the_granularity`.
    #[must_use]
    pub fn settled_before(&self, observed_at: MTime, granularity: Duration) -> bool {
        older_by(self.mtime, observed_at, granularity)
            && older_by(self.ctime, observed_at, granularity)
    }
}

/// Whether `time` is at least `by` before `reference`.
fn older_by(time: MTime, reference: MTime, by: Duration) -> bool {
    let secs = i64::try_from(by.as_secs()).unwrap_or(i64::MAX);
    let shifted_secs = time.secs.saturating_add(secs);
    let nanos = time.nanos.saturating_add(by.subsec_nanos());
    let (shifted_secs, nanos) = if nanos >= 1_000_000_000 {
        (
            shifted_secs.saturating_add(1),
            nanos.saturating_sub(1_000_000_000),
        )
    } else {
        (shifted_secs, nanos)
    };
    MTime {
        secs: shifted_secs,
        nanos,
    } <= reference
}

/// One file of a [`GitSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitStat {
    /// Which file.
    pub file: GitFile,
    /// Where it is.
    pub path: FileUri,
    /// Its stat, or `None` when it does not exist (an unborn branch, refs not yet packed, no
    /// index yet): its appearing later is a change.
    pub stat: Option<StatRecord>,
}

/// What `HEAD` names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HeadState {
    /// A branch or other ref (`ref: refs/heads/main`).
    Symbolic(String),
    /// A commit, detached.
    Detached(ObjectId),
    /// Neither: missing, not a regular file, too long, or not in either form. Git would refuse
    /// the worktree too; a view treats its freshness as unknown.
    Unreadable,
}

/// The stat set of plan §4.3 for one worktree, taken without spawning anything: equal snapshots
/// (each [`StatRecord::settled_before`] the earlier one's observation) mean `HEAD`, the branch it
/// names and the index have not moved.
///
/// Every file is stat'ed before it is read, so a replacement racing the snapshot leaves the older
/// stat behind and the next snapshot sees a change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitSnapshot {
    /// What `HEAD` named when it was read.
    pub head: HeadState,
    /// The files stat'ed, in a fixed order: `HEAD`, then the refs `HEAD` leads to and
    /// `packed-refs`, then the index.
    pub files: Vec<GitStat>,
}

impl GitSnapshot {
    /// Whether every file present is [settled](StatRecord::settled_before) at `observed_at`.
    #[must_use]
    pub fn settled_before(&self, observed_at: MTime, granularity: Duration) -> bool {
        self.files.iter().all(|file| {
            file.stat
                .is_none_or(|stat| stat.settled_before(observed_at, granularity))
        })
    }
}

/// The files changed since a tree (plan §4.3): what a reconcile re-summarises.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedFiles {
    /// `HEAD`'s tree when the changes were read; `None` on an unborn branch. The next reconcile
    /// passes it back as its base.
    pub head_tree: Option<ObjectId>,
    /// The base could not be compared with `HEAD` (no base, or `HEAD` unborn while a base was
    /// given): every tracked file may have changed, so the caller rescans the worktree.
    pub everything: bool,
    /// Paths that differ between the base tree and `HEAD`'s tree (`git diff --name-only`),
    /// sorted, each once.
    pub committed: Vec<RelPath>,
    /// Tracked paths whose index or working copy differs from `HEAD` (staged, unstaged,
    /// deleted, unmerged; `git status`), sorted, each once.
    pub working: Vec<RelPath>,
    /// Untracked paths Git does not ignore, every file listed (not their directory), sorted.
    pub untracked: Vec<RelPath>,
    /// Paths Git reported that have no [`RelPath`] (not UTF-8, too long): counted, never dropped
    /// silently. A view with any cannot claim it saw every change (ADR 0014).
    pub unrepresentable: u64,
}

impl ChangedFiles {
    /// Every path named in any part, sorted, each once.
    #[must_use]
    pub fn paths(&self) -> Vec<&RelPath> {
        let mut all: Vec<&RelPath> = self
            .committed
            .iter()
            .chain(&self.working)
            .chain(&self.untracked)
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    }
}

/// The `.java` files the index tracks (`git ls-files`), wherever they are: under a `.gitignore`d
/// or build-output directory too, which a listing of the tree skips (IDX-06's union, IDX-09).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedFiles {
    /// Sorted, each once.
    pub files: Vec<RelPath>,
    /// Tracked `.java` paths with no [`RelPath`], counted.
    pub unrepresentable: u64,
}

/// Which step of the reconcile path failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitStep {
    /// Confirming a root (`rev-parse --show-toplevel --git-dir --git-common-dir`).
    Confirm,
    /// Listing the repository's filter drivers (`config --get-regexp`), to turn them off.
    Config,
    /// Reading `HEAD`'s tree (`rev-parse HEAD^{tree}`).
    HeadTree,
    /// Comparing two trees (`diff --name-only`).
    Diff,
    /// The working state (`status`).
    Status,
    /// The tracked files (`ls-files`).
    LsFiles,
}

/// Why a `RepoProbe` call failed. The values carry no part of a path and no Git output (fnd §5
/// rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RepoProbeError {
    /// The path, or the root it names, does not exist.
    #[error("the path does not exist")]
    NotFound,
    /// A path with no [`FileUri`] (a canonical root or Git directory that is not UTF-8, or too
    /// long).
    #[error("the path has no file URI")]
    Unrepresentable,
    /// A `.git` was found, but Git does not take the directory for a worktree whose top level it
    /// is (a broken `.git` file, `core.worktree` pointing elsewhere, a repository owned by
    /// another user).
    #[error("the directory is not a worktree Git accepts")]
    NotAWorktree,
    /// The repository keeps its refs in the reftable format (`extensions.refStorage=reftable`),
    /// whose stat set no test pins yet: refused until one does (IDX-09, IDX-15).
    #[error("reftable repositories are not supported yet")]
    ReftableUnsupported,
    /// The operating system refused access.
    #[error("permission denied")]
    PermissionDenied,
    /// Any other I/O error.
    #[error("I/O error: {0}")]
    Io(std::io::ErrorKind),
    /// `git` could not be started (not installed, not on the allowed `PATH`).
    #[error("git could not be started")]
    GitMissing,
    /// `git` exited unsuccessfully: an unknown base tree, a corrupt repository.
    #[error("git failed at {step:?} with exit code {code:?}")]
    GitFailed {
        /// Which step.
        step: GitStep,
        /// Its exit code, or `None` when a signal ended it.
        code: Option<i32>,
    },
    /// `git` did not finish before its deadline and was killed.
    #[error("git passed its deadline at {0:?}")]
    Deadline(GitStep),
    /// `git` wrote more than the step's output limit: a partial answer is refused, never used.
    #[error("git's output at {0:?} passed its limit")]
    OutputTooLarge(GitStep),
    /// `git`'s output was not in the form asked for.
    #[error("git's output at {0:?} is malformed")]
    Malformed(GitStep),
    /// The blocking job did not finish (the pool shut down).
    #[error("interrupted")]
    Interrupted,
}

impl From<crate::blocking::Interrupted> for RepoProbeError {
    fn from(_: crate::blocking::Interrupted) -> Self {
        Self::Interrupted
    }
}

/// A worktree's Git state (plan §6.4). Paths are absolute [`FileUri`]s in, canonical
/// [`Worktree`] roots and root-relative [`RelPath`]s out.
///
/// What any implementation must do is the `repo_probe` contract suite
/// (`jfl_testkit::contracts::repo_probe`). Every call only reads: each is cancel-safe, and none
/// writes the index or takes a lock a user's own `git` would wait on.
///
/// **Order.** A reconcile takes its [`RepoProbe::snapshot`] *before* [`RepoProbe::changed_since`].
/// `changed_since` reads `HEAD`'s tree and then the working state; a commit landing between the
/// two leaves its files in neither part. Taken in this order, the snapshot predates the move, so the
/// next request's snapshot differs and its reconcile, diffing from the returned
/// [`ChangedFiles::head_tree`], finds them.
pub trait RepoProbe: Send + Sync {
    /// The innermost worktree holding `path` (a file or a directory, existing or not): the
    /// nearest enclosing directory with a `.git` file or directory, never a parent of a nested
    /// worktree or repository (plan §4.1, §4.2); `None` outside every repository.
    fn resolve(
        &self,
        path: &FileUri,
    ) -> impl Future<Output = Result<Option<Worktree>, RepoProbeError>> + Send;

    /// The stat set of plan §4.3 for `worktree`, without spawning anything.
    fn snapshot(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = Result<GitSnapshot, RepoProbeError>> + Send;

    /// `HEAD`'s tree; `None` on an unborn branch.
    fn head_tree(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = Result<Option<ObjectId>, RepoProbeError>> + Send;

    /// The files changed since the tree `base` (`None`: no tree seen yet, so
    /// [`ChangedFiles::everything`]): the trees' difference plus the working state. A base Git
    /// does not know is an error, never an empty change set.
    fn changed_since(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> impl Future<Output = Result<ChangedFiles, RepoProbeError>> + Send;

    /// The `.java` files the index tracks.
    fn tracked_java(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = Result<TrackedFiles, RepoProbeError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64, nanos: u32) -> MTime {
        MTime { secs, nanos }
    }

    fn stat(mtime: MTime, ctime: MTime) -> StatRecord {
        StatRecord {
            dev: 1,
            inode: 2,
            size: 3,
            mtime,
            ctime,
        }
    }

    #[test]
    fn should_accept_an_object_id_when_it_is_full_lower_case_hex() {
        // given
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // when
        let refused = [
            "",
            "0123456789ABCDEF0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdef0123456",
            "-123456789abcdef0123456789abcdef01234567",
            "HEAD",
            "0123456789abcdef0123456789abcdef01234567\n",
        ]
        .map(ObjectId::parse);

        // then
        assert_eq!(ObjectId::parse(sha1).map(|id| id.0), Some(sha1.to_owned()));
        assert!(ObjectId::parse(sha256).is_some());
        assert!(refused.iter().all(Option::is_none), "{refused:?}");
    }

    #[test]
    fn should_call_a_stat_racy_when_it_is_within_the_granularity() {
        // given
        let granularity = Duration::from_millis(10);
        let observed = at(100, 5_000_000);

        // when
        let old = stat(at(99, 0), at(99, 0)).settled_before(observed, granularity);
        let just_old_enough =
            stat(at(99, 995_000_000), at(99, 0)).settled_before(observed, granularity);
        let racy_mtime = stat(at(99, 995_000_001), at(99, 0)).settled_before(observed, granularity);
        let racy_ctime = stat(at(99, 0), at(100, 0)).settled_before(observed, granularity);
        let future = stat(at(200, 0), at(200, 0)).settled_before(observed, granularity);

        // then
        assert!(old);
        assert!(just_old_enough);
        assert!(!racy_mtime);
        assert!(!racy_ctime);
        assert!(!future);
    }

    #[test]
    fn should_call_a_snapshot_settled_when_every_present_file_is() {
        // given
        let path = FileUri::from_path("/r/.git/HEAD").unwrap();
        let entry = |stat| GitStat {
            file: GitFile::Head,
            path: path.clone(),
            stat,
        };
        let observed = at(100, 0);
        let granularity = Duration::from_secs(1);
        let settled = GitSnapshot {
            head: HeadState::Unreadable,
            files: vec![entry(Some(stat(at(1, 0), at(1, 0)))), entry(None)],
        };
        let racy = GitSnapshot {
            head: HeadState::Unreadable,
            files: vec![
                entry(Some(stat(at(1, 0), at(1, 0)))),
                entry(Some(stat(at(99, 500), at(1, 0)))),
            ],
        };

        // when
        let verdicts = (
            settled.settled_before(observed, granularity),
            racy.settled_before(observed, granularity),
        );

        // then
        assert_eq!(verdicts, (true, false));
    }

    #[test]
    fn should_list_each_path_once_when_parts_overlap() {
        // given
        let rel = |p: &str| RelPath::new(p).unwrap();
        let changed = ChangedFiles {
            committed: vec![rel("b"), rel("c")],
            working: vec![rel("a"), rel("b")],
            untracked: vec![rel("d")],
            ..ChangedFiles::default()
        };

        // when
        let paths: Vec<&str> = changed.paths().into_iter().map(RelPath::as_str).collect();

        // then
        assert_eq!(paths, ["a", "b", "c", "d"]);
    }
}
