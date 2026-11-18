//! The `SummaryStore` port (plan §5.5, §6.4): file summaries kept by [`SyntaxKey`] across every
//! worktree and commit holding the same bytes, and the name index of the large tier.
//!
//! **The tier (DEC-03, owner, 2026-11-18; plan §5.5).** Every view reads its summaries through
//! the store and writes the ones it had to compute, so a reopen re-hashes and re-parses only what
//! changed. What a view then *holds* depends on its size ([`StorageTier::for_view`]):
//!
//! - [`StorageTier::WholeInMemory`] (small and medium repositories): the whole summaries, read
//!   with [`SummaryStore::get`], rebuilt in memory at every open from the flat on-disk cache.
//! - [`StorageTier::ReferencesOnDisk`] (large repositories): each file's [`Declarations`] (its
//!   summary without reference sites, plus the distinct names those sites use), read with
//!   [`SummaryStore::get_declarations`], and a name → files index built from them
//!   ([`SummaryStore::postings`]). A `refs` query reads the whole summaries of only the files its
//!   name's postings list. Reference sites are most of a summary's bytes (IDX-02, IDX-07), and
//!   held whole they would pass the 250 MB syntax-tier budget on elasticsearch (plan §1.7, §5.5).
//!
//! **A cache, never a source of truth** (plan §5.10, fnd §6): a key the store does not hold, an
//! entry it cannot read whole, and an entry of another format version are all a miss
//! (`Ok(None)`), and the caller re-summarises the file. A store never answers with a summary other
//! than the one put under that key. Deleting the state directory costs time, never correctness.
//!
//! **Bounded** (plan §5.9): the store keeps at most its cap of bytes and evicts the entries used
//! least recently, by the ticks of an injected [`LruClock`] (no clock is read here or in the
//! adapter, fnd §2.4). [`StoreStats`] reports the entries, the bytes and every eviction, miss and
//! discarded entry, for `status`.
//!
//! The `summary_store` contract suite (`jfl_testkit::contracts::summary_store`, ver §5) checks
//! every implementation against these promises.

use core::future::Future;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use jfl_domain::{FileSummary, StrId, SyntaxKey};

/// From this many files a view keeps reference sites on disk ([`StorageTier::for_view`]).
///
/// Plan §5.5 puts the line at "about 10–15k files or 50 MB of Java": medium repositories
/// (spring-framework, 9.3k files) rebuild in memory, large ones (elasticsearch, 33.9k files) do
/// not. `just bench-store` measures both sides of it (`doc/bench/README.md`).
pub const REFERENCES_ON_DISK_FROM_FILES: u64 = 12_000;

/// From this many bytes of Java a view keeps reference sites on disk, whatever its file count.
pub const REFERENCES_ON_DISK_FROM_BYTES: u64 = 50_000_000;

/// What a view holds in memory (DEC-03; plan §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageTier {
    /// Whole summaries, reference sites included.
    WholeInMemory,
    /// [`Declarations`] and a name → files postings index; reference sites stay in the store.
    ReferencesOnDisk,
}

impl StorageTier {
    /// The tier of a view of `files` Java files holding `bytes` bytes of Java.
    ///
    /// Pinned by: `summary_store::tests::should_keep_references_on_disk_when_a_view_is_large`.
    #[must_use]
    pub const fn for_view(files: u64, bytes: u64) -> Self {
        if files >= REFERENCES_ON_DISK_FROM_FILES || bytes >= REFERENCES_ON_DISK_FROM_BYTES {
            Self::ReferencesOnDisk
        } else {
            Self::WholeInMemory
        }
    }
}

/// A file's summary without its reference sites, and the distinct names those sites use: what
/// the large tier holds in memory per file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declarations {
    summary: FileSummary,
    names: Box<[StrId]>,
}

/// Why [`Declarations::from_parts`] refused its parts (read back from a store, so untrusted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeclarationsError {
    /// The summary still holds reference sites.
    #[error("declarations hold {count} reference sites")]
    HasReferences {
        /// How many.
        count: usize,
    },
    /// A name id with no string.
    #[error("name id {id} is out of range")]
    UnknownName {
        /// The id.
        id: u32,
    },
    /// The names are not strictly increasing by their text.
    #[error("name {index} is not after the one before it")]
    Unsorted {
        /// Its index in the list.
        index: usize,
    },
}

impl Declarations {
    /// `summary` without its reference sites, and the names they use, sorted by text, each once.
    ///
    /// Pinned by: `summary_store::tests::should_list_each_reference_name_once_when_declarations_are_taken`.
    #[must_use]
    pub fn of(summary: &FileSummary) -> Self {
        let names = Self::names_of(summary);
        let summary = summary.without_references();
        Self { summary, names }
    }

    /// The ids of the distinct names `summary`'s reference sites use, sorted by their text (the
    /// smallest id of each text): the name list of [`Declarations::of`], without copying the
    /// summary.
    #[must_use]
    pub fn names_of(summary: &FileSummary) -> Box<[StrId]> {
        let mut names: Vec<StrId> = summary.references().iter().map(|r| r.name).collect();
        names.sort_by(|a, b| summary.str(*a).cmp(summary.str(*b)).then(a.cmp(b)));
        names.dedup_by(|a, b| summary.str(*a) == summary.str(*b));
        names.into_boxed_slice()
    }

    /// Checks parts read back from a store: no reference sites, and names that exist and are
    /// strictly increasing by their text.
    ///
    /// Pinned by: `summary_store::tests::should_refuse_declarations_when_their_parts_disagree`.
    pub fn from_parts(
        summary: FileSummary,
        names: Box<[StrId]>,
    ) -> Result<Self, DeclarationsError> {
        let count = summary.references().len();
        if count != 0 {
            return Err(DeclarationsError::HasReferences { count });
        }
        let strings = summary.parts().strings.len();
        let mut previous: Option<&str> = None;
        for (index, id) in names.iter().enumerate() {
            if usize::try_from(id.0).map_or(true, |at| at >= strings) {
                return Err(DeclarationsError::UnknownName { id: id.0 });
            }
            let name = summary.str(*id);
            if previous.is_some_and(|p| p >= name) {
                return Err(DeclarationsError::Unsorted { index });
            }
            previous = Some(name);
        }
        Ok(Self { summary, names })
    }

    /// The summary, with no reference sites.
    #[must_use]
    pub const fn summary(&self) -> &FileSummary {
        &self.summary
    }

    /// The ids of the names the reference sites use, sorted by text.
    #[must_use]
    pub const fn name_ids(&self) -> &[StrId] {
        &self.names
    }

    /// The names the reference sites use, sorted, each once.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(|id| self.summary.str(*id))
    }

    /// The bytes held on the heap: the summary's and the name list's.
    #[must_use]
    pub const fn heap_bytes(&self) -> usize {
        self.summary
            .heap_bytes()
            .saturating_add(self.names.len().saturating_mul(size_of::<StrId>()))
    }
}

/// The large tier's name → files index: for a simple name, the files (by the ordinal the caller
/// gave each) whose reference sites use it.
pub trait NamePostings: Send + Sync + 'static {
    /// The files whose reference sites use `name`, ascending, each once; empty for a name none
    /// uses.
    fn files(&self, name: &str) -> &[u32];

    /// How many distinct names the index holds.
    fn names(&self) -> usize;

    /// The bytes the index holds on the heap.
    fn heap_bytes(&self) -> usize;
}

/// A store failure that is not a miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SummaryStoreError {
    /// A write failed (the disk is full, the directory went away): the summary was not stored,
    /// and the caller keeps the one it has.
    #[error("the summary store could not write: {0}")]
    Write(std::io::ErrorKind),
    /// The pool shut down, or the job panicked.
    #[error("the summary store job was interrupted")]
    Interrupted,
}

/// What a store holds and what it did, for `status` (plan §5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StoreStats {
    /// Entries held.
    pub entries: u64,
    /// Their bytes, as the cap counts them.
    pub bytes: u64,
    /// The cap.
    pub cap: u64,
    /// Reads answered.
    pub hits: u64,
    /// Reads that were a miss, discarded entries included.
    pub misses: u64,
    /// Entries found unreadable (torn, truncated, corrupt, another format version) and removed.
    pub discarded: u64,
    /// Entries evicted by the cap.
    pub evicted: u64,
    /// Summaries not stored because they alone are larger than the cap, or than the largest
    /// entry the implementation reads back.
    pub refused: u64,
}

/// The ticks the cap orders entries by: larger is more recent. A counter is enough within one
/// process; tests drive a fake one.
pub trait LruClock: Send + Sync + 'static {
    /// The tick of a use happening now; never smaller than an earlier one.
    fn now(&self) -> u64;
}

/// An [`LruClock`] that counts uses, starting from a given tick.
#[derive(Debug, Default)]
pub struct CountingClock(AtomicU64);

impl CountingClock {
    /// A counter whose first tick is `first`.
    #[must_use]
    pub const fn starting_at(first: u64) -> Self {
        Self(AtomicU64::new(first))
    }
}

impl LruClock for CountingClock {
    fn now(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

/// Summaries by [`SyntaxKey`], bounded by a cap of bytes, evicting the least recently used.
pub trait SummaryStore: Send + Sync + 'static {
    /// The large tier's name index of this implementation.
    type Postings: NamePostings;

    /// The whole summary put under `key`, or `None`: never another summary (a miss is the answer
    /// to an unknown key and to an entry that cannot be read whole, which is removed). A use, for
    /// the cap.
    fn get(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<FileSummary>, SummaryStoreError>> + Send;

    /// [`Declarations::of`] the summary put under `key`, or `None`, as [`Self::get`]; without
    /// decoding the reference sites. A use, for the cap.
    fn get_declarations(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<Declarations>, SummaryStoreError>> + Send;

    /// Stores `summary` under `key` (the [`SyntaxKey`] of the bytes it summarises), then evicts
    /// least recently used entries until the cap holds. Putting a key already held only uses it:
    /// a key names one content. A summary larger than the whole cap, or than the largest entry the
    /// implementation reads back, is not stored ([`StoreStats::refused`]).
    fn put(
        &self,
        key: SyntaxKey,
        summary: Arc<FileSummary>,
    ) -> impl Future<Output = Result<(), SummaryStoreError>> + Send;

    /// What the store holds and did.
    fn stats(&self) -> StoreStats;

    /// The name → files index of `files`: each file's ordinal (chosen by the caller, the view's
    /// file index) and its declarations.
    fn postings<'a, I>(&self, files: I) -> Self::Postings
    where
        I: IntoIterator<Item = (u32, &'a Declarations)>;
}

#[cfg(test)]
mod tests {
    use jfl_domain::{
        ByteOffset, ByteRange, DeclKind, DeclarationInput, Modifiers, RefShape, SummaryBuilder,
    };

    use super::*;

    fn range(start: u32, end: u32) -> ByteRange {
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
    }

    fn summary() -> FileSummary {
        let mut builder = SummaryBuilder::new();
        builder.package("p").unwrap();
        builder
            .declaration(&DeclarationInput {
                kind: DeclKind::Class,
                modifiers: Modifiers::empty(),
                parent: None,
                name: "A",
                name_range: range(6, 7),
                range: range(0, 20),
                signature: "class A",
                params: &[],
                supertypes: &[],
                permits: &[],
                annotations: &[],
                javadoc: None,
            })
            .unwrap();
        for (name, at, end) in [
            ("zeta", 8, 9),
            ("A", 10, 11),
            ("alpha", 12, 13),
            ("zeta", 14, 15),
        ] {
            builder
                .reference(name, range(at, end), RefShape::Name, jfl_domain::Arity(0))
                .unwrap();
        }
        builder.finish()
    }

    #[test]
    fn should_keep_references_on_disk_when_a_view_is_large() {
        // given
        let views = [
            (9_300, 40_000_000),
            (REFERENCES_ON_DISK_FROM_FILES, 1),
            (1, REFERENCES_ON_DISK_FROM_BYTES),
            (33_850, 334_000_000),
        ];
        // when
        let tiers = views.map(|(files, bytes)| StorageTier::for_view(files, bytes));
        // then
        assert_eq!(
            tiers,
            [
                StorageTier::WholeInMemory,
                StorageTier::ReferencesOnDisk,
                StorageTier::ReferencesOnDisk,
                StorageTier::ReferencesOnDisk,
            ]
        );
    }

    #[test]
    fn should_list_each_reference_name_once_when_declarations_are_taken() {
        // given
        let whole = summary();
        // when
        let declarations = Declarations::of(&whole);
        // then
        assert_eq!(
            declarations.names().collect::<Vec<_>>(),
            ["A", "alpha", "zeta"]
        );
        assert!(declarations.summary().references().is_empty());
        assert_eq!(
            declarations.summary().declarations().collect::<Vec<_>>(),
            whole.declarations().collect::<Vec<_>>()
        );
        let again = Declarations::from_parts(
            declarations.summary().clone(),
            declarations.name_ids().into(),
        );
        assert_eq!(again, Ok(declarations));
    }

    #[test]
    fn should_refuse_declarations_when_their_parts_disagree() {
        // given
        let whole = summary();
        let declarations = Declarations::of(&whole);
        let ids = declarations.name_ids().to_vec();
        let mut reversed = ids.clone();
        reversed.reverse();
        // when
        let refused = [
            Declarations::from_parts(whole.clone(), ids.clone().into()),
            Declarations::from_parts(declarations.summary().clone(), reversed.into()),
            Declarations::from_parts(declarations.summary().clone(), vec![StrId(999)].into()),
            Declarations::from_parts(
                declarations.summary().clone(),
                [ids.clone(), ids].concat().into(),
            ),
        ];
        // then
        assert_eq!(
            refused.map(|r| r.err()),
            [
                Some(DeclarationsError::HasReferences { count: 4 }),
                Some(DeclarationsError::Unsorted { index: 1 }),
                Some(DeclarationsError::UnknownName { id: 999 }),
                Some(DeclarationsError::Unsorted { index: 3 }),
            ]
        );
    }

    #[test]
    fn should_count_up_when_a_counting_clock_ticks() {
        // given
        let clock = CountingClock::starting_at(5);
        // when
        let ticks = [clock.now(), clock.now(), clock.now()];
        // then
        assert_eq!(ticks, [5, 6, 7]);
    }
}
