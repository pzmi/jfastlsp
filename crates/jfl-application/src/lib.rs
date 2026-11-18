//! Use cases and queries, the view registry and generations, the diagnostics coordinator and the
//! port traits. Role: application.
//!
//! - [`source_tree`] (IDX-06): the `SourceTree` port, its value types and the exclusion rules of
//!   plan §4.2 that every implementation applies.
//! - [`hints`] (IDX-06): the bounded, coalescing queue a watch delivers its hints through.
//! - [`blocking`] (IDX-06): the `BlockingPool` port, where adapters run blocking jobs (fnd §4.4).
//! - [`repo_probe`] (IDX-05): the `RepoProbe` port: worktree roots, repository identity, the Git
//!   stat set of plan §4.3 and the files changed since a tree.
//! - [`syntax_indexer`] (IDX-07): the `SyntaxIndexer` port and the source checks every
//!   implementation applies first.

pub mod blocking;
pub mod hints;
pub mod repo_probe;
pub mod source_tree;
pub mod syntax_indexer;

pub use blocking::{BlockingPool, Interrupted};
pub use hints::{DEFAULT_HINT_CAPACITY, HintQueue, HintSink, hint_queue};
pub use repo_probe::{
    ChangedFiles, GitFile, GitSnapshot, GitStat, GitStep, HeadState, ObjectId, RepoId, RepoProbe,
    RepoProbeError, StatRecord, TrackedFiles, Worktree, WorktreeKind,
};
pub use source_tree::{
    Exclusion, FileStat, GitEntry, GitMarker, Hint, HintSubject, Hints, ListMode, ListedFile,
    Listing, MTime, ReadBytes, Rescan, RescanReason, RescanReasons, Skipped, SourceTree,
    SourceTreeError, Unlisted, UnlistedReason, WatchErrorKind,
};
pub use syntax_indexer::{
    GrammarId, Indexed, SyntaxDiagnostic, SyntaxIndexError, SyntaxIndexer, SyntaxProblem,
};
