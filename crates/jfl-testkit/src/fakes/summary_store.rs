//! [`MemorySummaryStore`]: a `SummaryStore` in memory, with the cap, the least-recently-used
//! eviction and the misses of the on-disk one, and [`FakeClock`], the clock tests drive.
//!
//! It is no looser than `jfl-adapter-store` (ver §5): it measures an entry by the summary's
//! [`FileSummary::heap_bytes`] where the adapter counts encoded bytes, evicts on the same ticks,
//! refuses a summary larger than the cap, and can be told to lose an entry the way a torn file is
//! lost ([`MemorySummaryStore::damage`]): the next read is a miss and the entry is discarded.
//! Contents outlive a store over the same [`Backing`], as files outlive a daemon
//! ([`MemorySummaryStore::open`]); recency does not, as the adapter's does not.
//!
//! Pinned by the `summary_store` contract suite (`contracts::summary_store::tests`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use jfl_application::{
    Declarations, LruClock, NamePostings, StoreStats, SummaryStore, SummaryStoreError,
};
use jfl_domain::{FileSummary, SyntaxKey};

/// An [`LruClock`] that reads what the test set.
#[derive(Debug, Default)]
pub struct FakeClock(AtomicU64);

impl FakeClock {
    /// A clock at `tick`.
    #[must_use]
    pub const fn at(tick: u64) -> Self {
        Self(AtomicU64::new(tick))
    }

    /// Moves the clock to `tick`.
    pub fn set(&self, tick: u64) {
        self.0.store(tick, Ordering::Relaxed);
    }
}

impl LruClock for FakeClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
struct Stored {
    summary: Arc<FileSummary>,
    damaged: bool,
}

/// What a [`MemorySummaryStore`] stores, shared by every store opened over it: the fake's disk.
#[derive(Debug, Clone, Default)]
pub struct Backing(Arc<Mutex<BTreeMap<SyntaxKey, Stored>>>);

impl Backing {
    /// An empty backing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<SyntaxKey, Stored>) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Forgets everything: a deleted state directory.
    pub fn wipe(&self) {
        self.with(BTreeMap::clear);
    }
}

#[derive(Debug, Default)]
struct Recency {
    ticks: BTreeMap<SyntaxKey, (u8, u64)>,
    order: BTreeSet<((u8, u64), SyntaxKey)>,
    stats: StoreStats,
}

impl Recency {
    fn set(&mut self, key: SyntaxKey, rank: (u8, u64)) {
        if let Some(old) = self.ticks.insert(key, rank) {
            self.order.remove(&(old, key));
        }
        self.order.insert((rank, key));
    }

    fn forget(&mut self, key: SyntaxKey) {
        if let Some(old) = self.ticks.remove(&key) {
            self.order.remove(&(old, key));
        }
    }
}

/// The in-memory [`SummaryStore`].
#[derive(Clone)]
pub struct MemorySummaryStore {
    backing: Backing,
    cap: u64,
    clock: Arc<dyn LruClock>,
    recency: Arc<Mutex<Recency>>,
}

impl core::fmt::Debug for MemorySummaryStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemorySummaryStore")
            .field("cap", &self.cap)
            .finish_non_exhaustive()
    }
}

fn size_of_summary(summary: &FileSummary) -> u64 {
    u64::try_from(summary.heap_bytes()).unwrap_or(u64::MAX)
}

impl MemorySummaryStore {
    /// A store over `backing` with a cap of `cap` bytes (by [`FileSummary::heap_bytes`]), ticking
    /// uses on `clock`. Entries already in the backing rank before any use, in key order.
    #[must_use]
    pub fn open(backing: Backing, cap: u64, clock: Arc<dyn LruClock>) -> Self {
        let mut recency = Recency::default();
        recency.stats.cap = cap;
        backing.with(|entries| {
            for (rank, (key, stored)) in (0_u64..).zip(entries.iter()) {
                recency.set(*key, (0, rank));
                recency.stats.entries = recency.stats.entries.saturating_add(1);
                recency.stats.bytes = recency
                    .stats
                    .bytes
                    .saturating_add(size_of_summary(&stored.summary));
            }
        });
        let store = Self {
            backing,
            cap,
            clock,
            recency: Arc::new(Mutex::new(recency)),
        };
        store.evict(None);
        store
    }

    /// Makes `key`'s entry unreadable, as a torn or truncated file is: its next read is a miss.
    pub fn damage(&self, key: &SyntaxKey) {
        self.backing.with(|entries| {
            if let Some(stored) = entries.get_mut(key) {
                stored.damaged = true;
            }
        });
    }

    fn with_recency<T>(&self, f: impl FnOnce(&mut Recency) -> T) -> T {
        f(&mut self.recency.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn read(&self, key: &SyntaxKey) -> Option<Arc<FileSummary>> {
        let tick = self.clock.now();
        let found = self.backing.with(|entries| match entries.get(key) {
            Some(stored) if stored.damaged => {
                entries.remove(key);
                Err(())
            }
            Some(stored) => Ok(Some(Arc::clone(&stored.summary))),
            None => Ok(None),
        });
        self.with_recency(|r| match &found {
            Ok(Some(_)) => {
                r.set(*key, (1, tick));
                r.stats.hits = r.stats.hits.saturating_add(1);
            }
            Ok(None) => {
                r.stats.misses = r.stats.misses.saturating_add(1);
            }
            Err(()) => {
                r.forget(*key);
                r.stats.misses = r.stats.misses.saturating_add(1);
                r.stats.discarded = r.stats.discarded.saturating_add(1);
            }
        });
        if found.is_err() {
            self.recount();
        }
        found.ok().flatten()
    }

    fn recount(&self) {
        let (entries, bytes) = self.backing.with(|entries| {
            (
                u64::try_from(entries.len()).unwrap_or(u64::MAX),
                entries
                    .values()
                    .map(|s| size_of_summary(&s.summary))
                    .fold(0_u64, u64::saturating_add),
            )
        });
        self.with_recency(|r| {
            r.stats.entries = entries;
            r.stats.bytes = bytes;
        });
    }

    fn write(&self, key: SyntaxKey, summary: Arc<FileSummary>) {
        let tick = self.clock.now();
        let size = size_of_summary(&summary);
        if size > self.cap {
            self.with_recency(|r| r.stats.refused = r.stats.refused.saturating_add(1));
            return;
        }
        self.backing.with(|entries| {
            entries.entry(key).or_insert(Stored {
                summary,
                damaged: false,
            });
        });
        self.with_recency(|r| r.set(key, (1, tick)));
        self.recount();
        self.evict(Some(key));
    }

    fn evict(&self, keep: Option<SyntaxKey>) {
        loop {
            let over = self.with_recency(|r| r.stats.bytes > self.cap);
            if !over {
                return;
            }
            let victim = self.with_recency(|r| {
                r.order
                    .iter()
                    .map(|(_, key)| *key)
                    .find(|key| Some(*key) != keep)
            });
            let Some(victim) = victim else { return };
            self.backing.with(|entries| entries.remove(&victim));
            self.with_recency(|r| {
                r.forget(victim);
                r.stats.evicted = r.stats.evicted.saturating_add(1);
            });
            self.recount();
        }
    }
}

/// The fake's [`NamePostings`]: a sorted map.
#[derive(Debug, Clone, Default)]
pub struct MapPostings(BTreeMap<String, Vec<u32>>);

impl MapPostings {
    /// The postings of `files`.
    pub fn build<'a, I>(files: I) -> Self
    where
        I: IntoIterator<Item = (u32, &'a Declarations)>,
    {
        let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for (ordinal, declarations) in files {
            for name in declarations.names() {
                map.entry(name.to_owned()).or_default().push(ordinal);
            }
        }
        for files in map.values_mut() {
            files.sort_unstable();
            files.dedup();
        }
        Self(map)
    }
}

impl NamePostings for MapPostings {
    fn files(&self, name: &str) -> &[u32] {
        self.0.get(name).map_or(&[], Vec::as_slice)
    }

    fn names(&self) -> usize {
        self.0.len()
    }

    fn heap_bytes(&self) -> usize {
        self.0
            .iter()
            .map(|(name, files)| name.len().saturating_add(files.len().saturating_mul(4)))
            .fold(0, usize::saturating_add)
    }
}

impl SummaryStore for MemorySummaryStore {
    type Postings = MapPostings;

    fn get(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<FileSummary>, SummaryStoreError>> + Send {
        core::future::ready(Ok(self.read(&key).map(|s| (*s).clone())))
    }

    fn get_declarations(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<Declarations>, SummaryStoreError>> + Send {
        core::future::ready(Ok(self.read(&key).map(|s| Declarations::of(&s))))
    }

    fn put(
        &self,
        key: SyntaxKey,
        summary: Arc<FileSummary>,
    ) -> impl Future<Output = Result<(), SummaryStoreError>> + Send {
        self.write(key, summary);
        core::future::ready(Ok(()))
    }

    fn stats(&self) -> StoreStats {
        self.with_recency(|r| r.stats)
    }

    fn postings<'a, I>(&self, files: I) -> MapPostings
    where
        I: IntoIterator<Item = (u32, &'a Declarations)>,
    {
        MapPostings::build(files)
    }
}
