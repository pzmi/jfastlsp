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
//! - [`summary_store`] (IDX-08): the `SummaryStore` port, the storage tier of DEC-03 and the large
//!   tier's [`Declarations`] and name postings.
//! - [`views`] (IDX-09): the view registry: views per worktree root, generations, freshness,
//!   completeness and `sync`, the transitions of `formal/views.qnt`.
//! - [`view_service`] (IDX-09): the async shell that drives the registry over the ports.
//! - [`clock`] (IDX-09): the `WallClock` port, the time before each stat and the cookie deadline.
//! - [`diagnostics`] (LSP-04): the diagnostics coordinator: which compile results are published,
//!   keyed by document, open session and input epoch, the transitions of `formal/diagnostics.qnt`.
//! - [`queries`] (IDX-10): definition, hover, document and workspace symbols over a pinned
//!   generation, each answer with its completeness (plan §2.1, §2.2, §2.7).

pub mod blocking;
pub mod clock;
pub mod diagnostics;
pub mod hints;
pub mod queries;
pub mod repo_probe;
pub mod source_tree;
pub mod summary_store;
pub mod syntax_indexer;
pub mod view_service;
pub mod views;

pub use blocking::{BlockingPool, Interrupted};
pub use clock::WallClock;
pub use hints::{DEFAULT_HINT_CAPACITY, HintQueue, HintSink, hint_queue};
pub use queries::{
    ClockDeadline, Deadline, DocumentSymbol, DocumentSymbols, FileText, Hover, HoverDeclaration,
    Pinned, QueryCache, QueryConfig, QueryError, ReferenceCounts, ReferencesQuery, Sources,
    SymbolFilter, SymbolQuery, Symbols, WorkspaceSymbol,
};
pub use repo_probe::{
    ChangedFiles, GitFile, GitSnapshot, GitStat, GitStep, HeadState, ObjectId, RepoId, RepoProbe,
    RepoProbeError, StatRecord, TrackedFiles, Worktree, WorktreeKind,
};
pub use source_tree::{
    Exclusion, FileStat, FsKind, GitEntry, GitMarker, Hint, HintSubject, Hints, ListMode,
    ListedFile, Listing, MTime, ReadBytes, Rescan, RescanReason, RescanReasons, Skipped,
    SourceTree, SourceTreeError, Unlisted, UnlistedReason, WatchErrorKind,
};
pub use summary_store::{
    CountingClock, Declarations, DeclarationsError, LruClock, NamePostings, StorageTier,
    StoreStats, SummaryStore, SummaryStoreError,
};
pub use syntax_indexer::{
    Around, GrammarId, Indexed, SiteForm, SiteSyntax, SyntaxDiagnostic, SyntaxIndexError,
    SyntaxIndexer, SyntaxProblem,
};
pub use view_service::{
    Answer, GitObserved, ServiceConfig, ServiceError, ServiceSources, Summary, ViewService,
};
pub use views::{
    Build, Cookie, Entry, Event, FileObserved, FileSeen, GitSeen, Observed, Pin, Published,
    Request, RequestId, RequestPhase, Stamp, Started, Stored, SyncPhase, SyncState, ViewError,
    ViewRegistry, ViewState,
};
