//! Use cases and queries, the view registry and generations, the diagnostics coordinator and the
//! port traits. Role: application.
//!
//! - [`source_tree`] (IDX-06): the `SourceTree` port, its value types and the exclusion rules of
//!   plan §4.2 that every implementation applies.
//! - [`hints`] (IDX-06): the bounded, coalescing queue a watch delivers its hints through.
//! - [`blocking`] (IDX-06): the `BlockingPool` port, where adapters run blocking jobs (fnd §4.4).

pub mod blocking;
pub mod hints;
pub mod source_tree;

pub use blocking::{BlockingPool, Interrupted};
pub use hints::{DEFAULT_HINT_CAPACITY, HintQueue, HintSink, hint_queue};
pub use source_tree::{
    Exclusion, FileStat, GitEntry, Hint, HintSubject, Hints, ListMode, ListedFile, Listing, MTime,
    ReadBytes, Rescan, RescanReason, RescanReasons, Skipped, SourceTree, SourceTreeError, Unlisted,
    UnlistedReason, WatchErrorKind,
};
