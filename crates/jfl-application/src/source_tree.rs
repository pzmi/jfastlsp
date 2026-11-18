//! The `SourceTree` port (plan §6.4): a worktree's `.java` files, listed with the exclusions of
//! plan §4.2, read with a bound, stat'ed, and watched for hints (plan §4.3).
//!
//! The exclusion rules are pure functions here, so the testkit fake and the filesystem adapter
//! apply the same rules and the `source_tree` contract suite (`jfl_testkit::contracts`) checks
//! that each does. What a listing could not see is reported ([`Listing::unlisted`]), never
//! dropped: a view built from an incomplete listing cannot answer `complete=true` (ADR 0014).

use core::future::Future;

use jfl_domain::{FileUri, RelPath};

/// The extension of the files a listing holds.
pub const JAVA_EXTENSION: &str = ".java";

/// Directory names that hold a build's output when they sit beside a build file (Gradle's
/// `build`, Maven's `target`, IntelliJ's `out`, Eclipse's `bin`). Only beside a build file: a
/// package may be called `build` (spring-framework's `org/springframework/build` is one).
pub const BUILD_OUTPUT_DIRS: [&str; 4] = ["build", "target", "out", "bin"];

/// The files that make a sibling [`BUILD_OUTPUT_DIRS`] directory a build's output.
pub const BUILD_FILES: [&str; 5] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "pom.xml",
];

/// Directories that only ever hold tool state, wherever they are: Gradle's project cache.
pub const TOOL_STATE_DIRS: [&str; 1] = [".gradle"];

/// The most entries [`Listing::unlisted`] keeps; [`Listing::unlisted_count`] counts them all.
pub const MAX_REPORTED_UNLISTED: usize = 1024;

/// What a listing returns for each file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    /// Paths only: the walk reads directories and never stats a file.
    Paths,
    /// Paths and each file's [`FileStat`]: one more `lstat` per file.
    PathsAndStat,
}

/// A modification time: seconds and nanoseconds since the Unix epoch, as the filesystem keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MTime {
    /// Whole seconds; negative before 1970.
    pub secs: i64,
    /// Nanoseconds within the second, below 1e9.
    pub nanos: u32,
}

/// What a stat tells freshness (plan §4.3): enough to see a write, a replacement or a truncation
/// without reading the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStat {
    /// Length in bytes.
    pub size: u64,
    /// Last modification.
    pub mtime: MTime,
    /// The inode number: an editor that saves by rename changes it.
    pub inode: u64,
}

/// One listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// The path under the root.
    pub path: RelPath,
    /// Its stat, with [`ListMode::PathsAndStat`].
    pub stat: Option<FileStat>,
}

/// Why a directory (or a symbolic link) was left out on purpose. Not a gap: these are the rules
/// of plan §4.2 and §11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exclusion {
    /// It holds a `.git` file: a linked worktree, which is its own view (plan §4.2).
    NestedWorktree,
    /// It holds a `.git` directory: a nested repository, its own view.
    NestedRepository,
    /// `.claude/worktrees/`, where agent worktrees live, skipped whatever it holds (plan §4.2).
    AgentWorktrees,
    /// A build's output ([`BUILD_OUTPUT_DIRS`] beside a [`BUILD_FILES`] file) or tool state
    /// ([`TOOL_STATE_DIRS`]).
    BuildOutput,
    /// A symbolic link to a directory or a `.java` file: never followed, so nothing outside the
    /// root is listed (plan §11) and nothing inside it twice.
    Symlink,
}

/// A path left out on purpose.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Skipped {
    /// The directory or link.
    pub path: RelPath,
    /// Which rule.
    pub why: Exclusion,
}

/// Why something under the root could not be listed: a gap in the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnlistedReason {
    /// A directory could not be read (permissions, or it vanished during the walk).
    Unreadable,
    /// A `.java` file whose name is not UTF-8: it has no [`RelPath`].
    NonUtf8Name,
    /// A path longer than [`jfl_domain::path::MAX_PATH_LEN`].
    PathTooLong,
    /// A file whose stat failed (it vanished between the directory read and the stat).
    StatFailed,
    /// The listing reached its file cap and stopped.
    TooManyFiles,
}

/// A gap in a listing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unlisted {
    /// The nearest path that names it, when there is one (a non-UTF-8 name has none).
    pub path: Option<RelPath>,
    /// What went wrong.
    pub why: UnlistedReason,
}

/// A listing of a root's `.java` files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Every listed file, sorted by path, each once.
    pub files: Vec<ListedFile>,
    /// What the exclusion rules left out, sorted.
    pub skipped: Vec<Skipped>,
    /// The gaps, sorted, at most [`MAX_REPORTED_UNLISTED`].
    pub unlisted: Vec<Unlisted>,
    /// How many gaps there were in all.
    pub unlisted_count: u64,
}

impl Listing {
    /// Whether the listing saw everything its rules let it see.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unlisted_count == 0
    }

    /// Records a gap, keeping at most [`MAX_REPORTED_UNLISTED`] of them.
    pub fn record_unlisted(&mut self, gap: Unlisted) {
        self.unlisted_count = self.unlisted_count.saturating_add(1);
        if self.unlisted.len() < MAX_REPORTED_UNLISTED {
            self.unlisted.push(gap);
        }
    }

    /// Sorts every part, so equal trees give equal listings.
    pub fn sort(&mut self) {
        self.files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        self.skipped.sort_unstable();
        self.unlisted.sort_unstable();
    }
}

/// A bounded read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBytes {
    /// At most the limit's bytes, from the start of the file.
    pub bytes: Vec<u8>,
    /// The file held more than the limit.
    pub truncated: bool,
}

/// Why a watcher failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchErrorKind {
    /// The system's limit on watches was reached (Linux `max_user_watches`).
    LimitReached,
    /// Any other I/O error.
    Io,
    /// The watcher reported something else.
    Other,
}

/// Why every path under the root may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RescanReason {
    /// The hint queue filled up; its paths were dropped and counted.
    QueueFull,
    /// The operating system's own event queue overflowed (inotify `IN_Q_OVERFLOW`, FSEvents'
    /// must-scan flag).
    WatcherOverflow,
    /// A change at a path with no name under the root: the root itself, a path outside it, or a
    /// name that is not UTF-8.
    Unnamed,
    /// The watcher reported an error, or a directory could not be watched: part of the tree may
    /// now be unwatched, so later changes there may go unhinted.
    WatcherError(WatchErrorKind),
    /// The watch ended (its backend stopped): nothing is hinted any more.
    WatchEnded,
}

impl RescanReason {
    const ALL: [Self; 7] = [
        Self::QueueFull,
        Self::WatcherOverflow,
        Self::Unnamed,
        Self::WatcherError(WatchErrorKind::LimitReached),
        Self::WatcherError(WatchErrorKind::Io),
        Self::WatcherError(WatchErrorKind::Other),
        Self::WatchEnded,
    ];

    fn bit(self) -> u8 {
        match self {
            Self::QueueFull => 1,
            Self::WatcherOverflow => 1 << 1,
            Self::Unnamed => 1 << 2,
            Self::WatcherError(WatchErrorKind::LimitReached) => 1 << 3,
            Self::WatcherError(WatchErrorKind::Io) => 1 << 4,
            Self::WatcherError(WatchErrorKind::Other) => 1 << 5,
            Self::WatchEnded => 1 << 6,
        }
    }

    /// Whether this reason means the watch may have stopped seeing part of the tree for good,
    /// not just lost some events: one rescan does not make the view safe again.
    #[must_use]
    pub fn degrades_the_watch(self) -> bool {
        matches!(self, Self::WatcherError(_) | Self::WatchEnded)
    }
}

/// Every reason a rescan was asked for since the last one was taken: none is ever hidden behind
/// another (a lost watch behind a full queue, say).
///
/// Pinned by: `hints::tests::should_keep_every_reason_when_rescans_pile_up`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RescanReasons(u8);

impl RescanReasons {
    /// The set holding only `reason`.
    #[must_use]
    pub fn of(reason: RescanReason) -> Self {
        Self(reason.bit())
    }

    /// Adds `reason`.
    pub fn insert(&mut self, reason: RescanReason) {
        self.0 |= reason.bit();
    }

    /// Whether `reason` is in the set.
    #[must_use]
    pub fn contains(self, reason: RescanReason) -> bool {
        self.0 & reason.bit() != 0
    }

    /// The reasons, in a fixed order.
    pub fn iter(self) -> impl Iterator<Item = RescanReason> {
        RescanReason::ALL
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }

    /// Whether any reason [degrades the watch](RescanReason::degrades_the_watch).
    #[must_use]
    pub fn degrade_the_watch(self) -> bool {
        self.iter().any(RescanReason::degrades_the_watch)
    }
}

/// Everything may have changed: reconcile the whole view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rescan {
    /// Every reason since the last rescan was taken.
    pub reasons: RescanReasons,
    /// The path hints this rescan replaced.
    pub dropped: u64,
}

/// A watcher's hint (plan §4.3): a reason to reconcile early, never a fact; the reconcile decides
/// what changed. Hints are not a complete record of changes: a change no hint names and no rescan
/// covers (the gaps `jfl_adapter_fs::watch` lists) is seen only by a request's own stat of that
/// file or by the next full reconcile, so a view must not claim `complete=true` from hints alone
/// (IDX-09's obligations, in its card).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hint {
    /// Something at this path changed: a file written, created, removed or renamed, or a
    /// directory created, removed or renamed. For a directory, anything under it may have changed
    /// too (a rename reports only the directory). A new directory's hint is handed out only once
    /// the directory is watched, so a scan of it on the hint misses nothing written later
    /// (watch, then scan).
    Changed(RelPath),
    /// Something may have changed anywhere under the root.
    Rescan(Rescan),
}

/// What a change names, as far as the watcher knows: the extension rule of [`hint_excluded`]
/// applies only to files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintSubject {
    /// A regular file (written, created, removed).
    File,
    /// A directory.
    Directory,
    /// Unknown: a rename, or an event that does not say.
    Unknown,
}

/// Why a `SourceTree` call failed. The values carry no part of a path (fnd §5 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceTreeError {
    /// The root does not exist.
    #[error("the root does not exist")]
    RootNotFound,
    /// The root is not a directory.
    #[error("the root is not a directory")]
    RootNotADirectory,
    /// The file does not exist.
    #[error("the file does not exist")]
    NotFound,
    /// The path names something other than a regular file.
    #[error("the path is not a regular file")]
    NotAFile,
    /// The operating system refused access.
    #[error("permission denied")]
    PermissionDenied,
    /// Any other I/O error.
    #[error("I/O error: {0}")]
    Io(std::io::ErrorKind),
    /// The watcher could not be started.
    #[error("the watcher could not start")]
    WatchFailed(WatchErrorKind),
    /// The blocking job did not finish (the pool shut down).
    #[error("interrupted")]
    Interrupted,
}

impl From<crate::blocking::Interrupted> for SourceTreeError {
    fn from(_: crate::blocking::Interrupted) -> Self {
        Self::Interrupted
    }
}

/// A stream of [`Hint`]s from one watch; dropping it stops the watch.
pub trait Hints: Send {
    /// The next hint, if one is waiting. May block briefly to watch a new directory before its
    /// hint is handed out: call it off the async runtime.
    fn try_next(&mut self) -> Option<Hint>;

    /// The next hint, waiting for one; `None` once the watch has ended and its last hint (a
    /// [`RescanReason::WatchEnded`] rescan) was taken. Cancel-safe: a hint is taken only when the
    /// future resolves.
    fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send;
}

/// A worktree's source files (plan §6.4). Every call takes the worktree root; paths below it are
/// [`RelPath`]s, which cannot leave it.
///
/// What any implementation must do is the `source_tree` contract suite
/// (`jfl_testkit::contracts::source_tree`).
pub trait SourceTree: Send + Sync {
    /// The live watch [`SourceTree::watch`] returns.
    type Watch: Hints;

    /// Lists the `.java` regular files under `root`, applying `.gitignore` and [`exclusion`].
    /// Symbolic links are never followed. A tracked file that matches a `.gitignore` pattern is
    /// not listed, though Git still tracks it: a caller that must know every tracked file unions
    /// the listing with the repository's. Cancel-safe (it only reads).
    fn list(
        &self,
        root: &FileUri,
        mode: ListMode,
    ) -> impl Future<Output = Result<Listing, SourceTreeError>> + Send;

    /// Reads at most `limit` bytes of the regular file `root/path`. Cancel-safe.
    fn read(
        &self,
        root: &FileUri,
        path: &RelPath,
        limit: usize,
    ) -> impl Future<Output = Result<ReadBytes, SourceTreeError>> + Send;

    /// Stats the regular file `root/path`. Cancel-safe.
    fn stat(
        &self,
        root: &FileUri,
        path: &RelPath,
    ) -> impl Future<Output = Result<FileStat, SourceTreeError>> + Send;

    /// Starts watching `root`: what the watch covers is established when the future resolves, so
    /// a caller watches first and lists second. Later changes are hinted, except those
    /// [`hint_excluded`] leaves out; a new directory's hint is handed out only once it is watched;
    /// whatever the watch cannot cover is a [`Rescan`]. Cancel-safe: a dropped future's watch is
    /// dropped with it.
    fn watch(
        &self,
        root: &FileUri,
    ) -> impl Future<Output = Result<Self::Watch, SourceTreeError>> + Send;
}

/// What sits at `<dir>/.git`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitEntry {
    /// A file: `dir` is a linked worktree.
    File,
    /// A directory: `dir` is a repository.
    Dir,
}

/// What `lstat` found at `<dir>/.git`, symbolic links not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitMarker {
    /// A directory.
    Dir,
    /// A regular file.
    File,
    /// A symbolic link, dangling or not.
    Symlink,
    /// Anything else: a FIFO, a socket, a device.
    Other,
}

impl GitMarker {
    /// The marker for an entry of this type (from `std::fs::FileType`, which this crate may not
    /// name).
    #[must_use]
    pub fn of(is_dir: bool, is_file: bool, is_symlink: bool) -> Self {
        match (is_dir, is_file, is_symlink) {
            (true, _, _) => Self::Dir,
            (_, true, _) => Self::File,
            (_, _, true) => Self::Symlink,
            _ => Self::Other,
        }
    }
}

/// Whether a directory holding `found` at `.git` is a nested root, and of which kind: **any**
/// entry counts, whatever its type and wherever a link points (plan §4.2). The one rule the
/// listing (`jfl-adapter-fs`, which skips such a directory) and root resolution
/// (`jfl-adapter-git`, which binds a request below it to it, never to the parent) share, so a file
/// a view never lists is never answered from that view. A directory is a repository; anything else
/// a linked worktree (or a broken one, which resolution refuses).
///
/// Pinned by: `source_tree::tests::should_call_any_git_entry_a_nested_root_when_one_is_found`.
#[must_use]
pub fn nested_root(found: Option<GitMarker>) -> Option<GitEntry> {
    found.map(|marker| match marker {
        GitMarker::Dir => GitEntry::Dir,
        GitMarker::File | GitMarker::Symlink | GitMarker::Other => GitEntry::File,
    })
}

/// The rule for one directory below the root (never the root itself): `None` to walk into it.
///
/// `parent` is its parent's name (`None` when the parent is the root, whose name is not part of
/// the tree); `git` says what `<dir>/.git` is, and `parent_has_build_file` whether a
/// [`BUILD_FILES`] file sits beside it. Both are asked only when the cheaper name rules have not
/// decided, so an adapter stats as little as it can.
///
/// The name `.git` itself is reported as `None` by this function: the caller never walks into a
/// `.git` directory and never lists a `.git` file, and neither is a skipped directory worth
/// reporting (see [`is_git_metadata`]).
///
/// Pinned by: `source_tree::tests::should_skip_a_directory_when_a_rule_of_plan_4_2_applies` and
/// the `source_tree` contract suite.
pub fn exclusion(
    parent: Option<&str>,
    name: &str,
    git: impl FnOnce() -> Option<GitEntry>,
    parent_has_build_file: impl FnOnce() -> bool,
) -> Option<Exclusion> {
    if TOOL_STATE_DIRS.contains(&name) {
        return Some(Exclusion::BuildOutput);
    }
    if parent == Some(".claude") && name == "worktrees" {
        return Some(Exclusion::AgentWorktrees);
    }
    match git() {
        Some(GitEntry::File) => return Some(Exclusion::NestedWorktree),
        Some(GitEntry::Dir) => return Some(Exclusion::NestedRepository),
        None => {}
    }
    if BUILD_OUTPUT_DIRS.contains(&name) && parent_has_build_file() {
        return Some(Exclusion::BuildOutput);
    }
    None
}

/// Whether `name` is Git's own metadata (`.git`), which a walk never enters or lists.
#[must_use]
pub fn is_git_metadata(name: &str) -> bool {
    name == ".git"
}

/// Whether a file of this name is listed.
#[must_use]
pub fn is_java_file_name(name: &str) -> bool {
    name.len() > JAVA_EXTENSION.len() && name.ends_with(JAVA_EXTENSION)
}

/// Whether a change at `path` is left out of the hints: under a `.git` directory, under
/// `.claude/worktrees/`, or, for a [`HintSubject::File`] only, a file whose name has an extension
/// no listing or exclusion rule reads (a `.class` file a build writes, an editor's swap file).
/// `.java` files, `.gitignore` and [`BUILD_FILES`] are always hinted, and so is every directory
/// and every change of unknown kind, whatever its name: `java.base` and `org.eclipse.core` are
/// directories.
///
/// Pinned by: `source_tree::tests::should_leave_out_a_hint_when_its_path_is_never_listed` and the
/// `source_tree` contract suite's dotted-directory cases.
#[must_use]
pub fn hint_excluded(path: &RelPath, subject: HintSubject) -> bool {
    let mut previous = "";
    for component in path.components() {
        if is_git_metadata(component) || (previous == ".claude" && component == "worktrees") {
            return true;
        }
        previous = component;
    }
    if subject != HintSubject::File {
        return false;
    }
    let name = path.file_name();
    if is_java_file_name(name) || name == ".gitignore" || BUILD_FILES.contains(&name) {
        return false;
    }
    name.get(1..).is_some_and(|rest| rest.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn should_skip_a_directory_when_a_rule_of_plan_4_2_applies() {
        // given
        let no_git = || None;
        let no_build = || false;

        // when
        let cases = [
            exclusion(Some("x"), ".gradle", no_git, no_build),
            exclusion(Some(".claude"), "worktrees", no_git, no_build),
            exclusion(Some("x"), "agent", || Some(GitEntry::File), no_build),
            exclusion(Some("x"), "vendored", || Some(GitEntry::Dir), no_build),
            exclusion(None, "build", no_git, || true),
            exclusion(Some("x"), "target", no_git, || true),
        ];

        // then
        assert_eq!(
            cases,
            [
                Some(Exclusion::BuildOutput),
                Some(Exclusion::AgentWorktrees),
                Some(Exclusion::NestedWorktree),
                Some(Exclusion::NestedRepository),
                Some(Exclusion::BuildOutput),
                Some(Exclusion::BuildOutput),
            ]
        );
    }

    #[test]
    fn should_call_any_git_entry_a_nested_root_when_one_is_found() {
        // given
        let found = [
            None,
            Some(GitMarker::of(true, false, false)),
            Some(GitMarker::of(false, true, false)),
            Some(GitMarker::of(false, false, true)),
            Some(GitMarker::of(false, false, false)),
        ];

        // when
        let roots = found.map(nested_root);

        // then
        assert_eq!(
            roots,
            [
                None,
                Some(GitEntry::Dir),
                Some(GitEntry::File),
                Some(GitEntry::File),
                Some(GitEntry::File)
            ]
        );
    }

    #[test]
    fn should_walk_a_directory_when_a_build_name_has_no_build_file_beside_it() {
        // given
        let spring_package = ("springframework", "build");

        // when
        let verdict = exclusion(Some(spring_package.0), spring_package.1, || None, || false);
        let worktrees_elsewhere = exclusion(Some("docs"), "worktrees", || None, || false);

        // then
        assert_eq!(verdict, None);
        assert_eq!(worktrees_elsewhere, None);
    }

    #[test]
    fn should_not_ask_for_a_stat_when_a_name_rule_decides() {
        // given
        let mut asked = false;

        // when
        let verdict = exclusion(
            Some(".claude"),
            "worktrees",
            || {
                asked = true;
                None
            },
            || false,
        );

        // then
        assert_eq!(verdict, Some(Exclusion::AgentWorktrees));
        assert!(!asked);
    }

    #[test]
    fn should_leave_out_a_hint_when_its_path_is_never_listed() {
        // given
        let left_out = [
            (".git/index", HintSubject::File),
            ("sub/.git", HintSubject::Directory),
            (".claude/worktrees/a/src", HintSubject::Unknown),
            ("build/classes/A.class", HintSubject::File),
            ("src/.A.java.swp", HintSubject::File),
        ];
        let kept = [
            ("src/A.java", HintSubject::File),
            ("src", HintSubject::Directory),
            (".gitignore", HintSubject::File),
            ("lib/build.gradle.kts", HintSubject::File),
            ("Makefile", HintSubject::File),
            ("modules/java.base", HintSubject::Directory),
            ("org.eclipse.core", HintSubject::Unknown),
            ("build/classes/A.class", HintSubject::Unknown),
        ];

        // when
        let out: Vec<bool> = left_out
            .iter()
            .map(|(p, s)| hint_excluded(&rel(p), *s))
            .collect();
        let kept: Vec<bool> = kept
            .iter()
            .map(|(p, s)| hint_excluded(&rel(p), *s))
            .collect();

        // then
        assert!(out.iter().all(|x| *x), "{out:?}");
        assert!(kept.iter().all(|x| !*x), "{kept:?}");
    }

    #[test]
    fn should_say_a_watch_is_degraded_when_a_watcher_error_is_among_the_reasons() {
        // given
        let mut reasons = RescanReasons::of(RescanReason::QueueFull);
        let before = reasons.degrade_the_watch();

        // when
        reasons.insert(RescanReason::WatcherError(WatchErrorKind::LimitReached));

        // then
        assert!(!before);
        assert!(reasons.degrade_the_watch());
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            [
                RescanReason::QueueFull,
                RescanReason::WatcherError(WatchErrorKind::LimitReached)
            ]
        );
    }

    #[test]
    fn should_list_a_file_when_its_name_ends_in_java() {
        // given
        let names = ["A.java", ".java", "A.javax", "A.JAVA", "java"];

        // when
        let listed: Vec<bool> = names.iter().map(|n| is_java_file_name(n)).collect();

        // then
        assert_eq!(listed, [true, false, false, false, false]);
    }

    #[test]
    fn should_keep_every_gap_counted_when_more_than_the_cap_are_recorded() {
        // given
        let mut listing = Listing::default();

        // when
        for _ in 0..MAX_REPORTED_UNLISTED + 5 {
            listing.record_unlisted(Unlisted {
                path: None,
                why: UnlistedReason::Unreadable,
            });
        }

        // then
        assert_eq!(listing.unlisted.len(), MAX_REPORTED_UNLISTED);
        assert_eq!(listing.unlisted_count, 1029);
        assert!(!listing.is_complete());
    }
}
