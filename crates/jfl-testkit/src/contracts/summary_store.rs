//! The `summary_store` contract (ver §5, plan §5.5, §5.10): what is put is what is read back, by
//! key and never another's; declarations come without reference sites and with their names; a put
//! is idempotent; the cap evicts the least recently used entry on the injected clock and refuses a
//! summary larger than itself; a damaged entry is a miss, discarded, and stored again by the next
//! put; contents survive a reopen; a wiped store loses time, never correctness; and the name
//! postings list every file whose reference sites use a name. Checked the same way against
//! [`crate::fakes::summary_store::MemorySummaryStore`] (`summary_store::tests` here) and against
//! `jfl-adapter-store` in `crates/jfl-adapter-store/tests/contract.rs`.
//!
//! The summaries are the golden corpus's (`fixtures/java/summaries/orchard`), so every shape the
//! extractor produces goes through the store. The suite's own tests run it against deliberately
//! broken implementations and assert each fails the case it should.

use std::collections::BTreeSet;
use std::sync::Arc;

use jfl_application::{Declarations, LruClock, NamePostings, StoreStats, SummaryStore};
use jfl_domain::{Digest, FileSummary, SyntaxKey};

use crate::exec::block_on;
use crate::fakes::summary_store::FakeClock;
use crate::golden;

/// A case the implementation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

/// An implementation under test and its medium.
pub trait Harness {
    /// The store.
    type Store: SummaryStore;

    /// Opens a store over this harness's medium (the same medium every time, so a second open is
    /// a reopen), with a cap of `cap` bytes and uses ticked on `clock`.
    fn open(&mut self, cap: u64, clock: Arc<dyn LruClock>) -> Result<Self::Store, String>;

    /// Makes `key`'s stored entry unreadable the way the medium fails: a torn or truncated file
    /// for the disk.
    fn damage(&mut self, store: &Self::Store, key: &SyntaxKey) -> Result<(), String>;

    /// Deletes the medium's contents: the state directory removed.
    fn wipe(&mut self) -> Result<(), String>;
}

type Case<H> = fn(&mut H, &[FileSummary]) -> Result<(), String>;

/// The golden corpus's summaries (orchard), in corpus order.
pub fn corpus_summaries() -> Result<Vec<FileSummary>, String> {
    golden::corpus("")?
        .into_iter()
        .map(|file| {
            let path = golden::golden_path("orchard", &file.path);
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            golden::parse(&text, &file.bytes)
                .map(|indexed| indexed.summary)
                .map_err(|e| format!("{}: {e}", file.path))
        })
        .collect()
}

/// The key numbered `n`: keys sort by `n`.
#[must_use]
pub fn key(n: u32) -> SyntaxKey {
    let mut bytes = [0xA5_u8; 32];
    for (to, from) in bytes.iter_mut().zip(n.to_be_bytes()) {
        *to = from;
    }
    SyntaxKey::from_digest(Digest::from_bytes(bytes))
}

/// Runs every case; an empty list is a pass.
pub fn check<H: Harness>(harness: &mut H) -> Vec<Violation> {
    let summaries = match corpus_summaries() {
        Ok(s) if s.len() >= 4 => s,
        Ok(s) => {
            return vec![Violation {
                case: "corpus",
                detail: format!("{} summaries, want at least 4", s.len()),
            }];
        }
        Err(detail) => {
            return vec![Violation {
                case: "corpus",
                detail,
            }];
        }
    };
    let cases: [(&'static str, Case<H>); 10] = [
        ("misses_an_unknown_key", misses_an_unknown_key),
        ("reads_back_what_was_put", reads_back_what_was_put),
        (
            "takes_declarations_without_references",
            takes_declarations_without_references,
        ),
        ("puts_idempotently", puts_idempotently),
        ("evicts_least_recently_used", evicts_least_recently_used),
        (
            "refuses_a_summary_larger_than_the_cap",
            refuses_a_summary_larger_than_the_cap,
        ),
        ("misses_a_damaged_entry", misses_a_damaged_entry),
        ("survives_a_reopen", survives_a_reopen),
        ("loses_only_time_when_wiped", loses_only_time_when_wiped),
        ("indexes_reference_names", indexes_reference_names),
    ];
    cases
        .into_iter()
        .filter_map(|(case, run)| {
            harness
                .wipe()
                .and_then(|()| run(harness, &summaries))
                .err()
                .map(|detail| Violation { case, detail })
        })
        .collect()
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::at(1))
}

fn get<S: SummaryStore>(store: &S, key: SyntaxKey) -> Result<Option<FileSummary>, String> {
    block_on(store.get(key)).map_err(|e| e.to_string())
}

fn put<S: SummaryStore>(store: &S, key: SyntaxKey, summary: &FileSummary) -> Result<(), String> {
    block_on(store.put(key, Arc::new(summary.clone()))).map_err(|e| e.to_string())
}

fn first(summaries: &[FileSummary]) -> Result<&FileSummary, String> {
    summaries
        .iter()
        .find(|s| !s.references().is_empty())
        .ok_or_else(|| "no corpus summary has reference sites".to_owned())
}

fn misses_an_unknown_key<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    put(&store, key(1), first(summaries)?)?;
    let got = get(&store, key(2))?;
    let declarations = block_on(store.get_declarations(key(2))).map_err(|e| e.to_string())?;
    if got.is_none() && declarations.is_none() {
        Ok(())
    } else {
        Err("an unknown key answered a summary".to_owned())
    }
}

fn reads_back_what_was_put<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    for (n, summary) in (0_u32..).zip(summaries) {
        put(&store, key(n), summary)?;
    }
    for (n, summary) in (0_u32..).zip(summaries) {
        match get(&store, key(n))? {
            Some(got) if got == *summary => {}
            Some(_) => return Err(format!("key {n} answered another summary")),
            None => return Err(format!("key {n} missed")),
        }
    }
    let stats = store.stats();
    let count = u64::try_from(summaries.len()).map_err(|e| e.to_string())?;
    if stats.entries == count && stats.hits == count {
        Ok(())
    } else {
        Err(format!("{stats:?} after {count} puts and reads"))
    }
}

fn takes_declarations_without_references<H: Harness>(
    h: &mut H,
    summaries: &[FileSummary],
) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    for (n, summary) in (0_u32..).zip(summaries) {
        put(&store, key(n), summary)?;
    }
    for (n, summary) in (0_u32..).zip(summaries) {
        let got = block_on(store.get_declarations(key(n))).map_err(|e| e.to_string())?;
        let want = Declarations::of(summary);
        if got.as_ref() != Some(&want) {
            return Err(format!(
                "key {n}: names {:?}, want {:?}",
                got.map(|d| d.names().map(str::to_owned).collect::<Vec<_>>()),
                want.names().collect::<Vec<_>>()
            ));
        }
    }
    Ok(())
}

fn puts_idempotently<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    let summary = first(summaries)?;
    put(&store, key(1), summary)?;
    let once = store.stats();
    put(&store, key(1), summary)?;
    let twice = store.stats();
    let got = get(&store, key(1))?;
    if once.entries == 1
        && twice.entries == 1
        && once.bytes == twice.bytes
        && got.as_ref() == Some(summary)
    {
        Ok(())
    } else {
        Err(format!("after one put {once:?}, after two {twice:?}"))
    }
}

/// The bytes the implementation counts for `summary`, from an empty store.
fn size_of<H: Harness>(h: &mut H, summary: &FileSummary) -> Result<u64, String> {
    let store = h.open(u64::MAX, clock())?;
    put(&store, key(0), summary)?;
    let bytes = store.stats().bytes;
    h.wipe()?;
    if bytes == 0 {
        Err("a stored summary counts no bytes".to_owned())
    } else {
        Ok(bytes)
    }
}

fn held<S: SummaryStore>(store: &S, keys: &[u32]) -> Result<Vec<u32>, String> {
    let mut held = Vec::new();
    for &n in keys {
        if get(store, key(n))?.is_some() {
            held.push(n);
        }
    }
    Ok(held)
}

fn evicts_least_recently_used<H: Harness>(
    h: &mut H,
    summaries: &[FileSummary],
) -> Result<(), String> {
    let summary = first(summaries)?;
    let size = size_of(h, summary)?;
    // Room for three entries and a half: the fourth evicts exactly one.
    let cap = size
        .checked_mul(7)
        .and_then(|s| s.checked_div(2))
        .ok_or("size overflows")?;
    let clock = clock();
    let store = h.open(cap, Arc::clone(&clock) as Arc<dyn LruClock>)?;
    clock.set(10);
    put(&store, key(1), summary)?;
    clock.set(20);
    put(&store, key(2), summary)?;
    clock.set(30);
    put(&store, key(3), summary)?;
    clock.set(40);
    if get(&store, key(1))?.is_none() {
        return Err("key 1 missed before the cap was reached".to_owned());
    }
    clock.set(50);
    put(&store, key(4), summary)?;
    let evicted = store.stats().evicted;
    clock.set(60);
    let held = held(&store, &[1, 2, 3, 4])?;
    if held == [1, 3, 4] && evicted == 1 {
        Ok(())
    } else {
        Err(format!(
            "held {held:?} with {evicted} evicted; want key 2, the least recently used, evicted alone"
        ))
    }
}

fn refuses_a_summary_larger_than_the_cap<H: Harness>(
    h: &mut H,
    summaries: &[FileSummary],
) -> Result<(), String> {
    let small = summaries
        .iter()
        .min_by_key(|s| s.heap_bytes())
        .ok_or("no summary")?;
    let large = summaries
        .iter()
        .max_by_key(|s| s.heap_bytes())
        .ok_or("no summary")?;
    let small_size = size_of(h, small)?;
    let large_size = size_of(h, large)?;
    if large_size <= small_size {
        return Err("the corpus's largest summary is no larger than its smallest".to_owned());
    }
    let store = h.open(small_size, clock())?;
    put(&store, key(1), small)?;
    put(&store, key(2), large)?;
    let stats = store.stats();
    let held = held(&store, &[1, 2])?;
    if held == [1] && stats.refused == 1 && stats.evicted == 0 {
        Ok(())
    } else {
        Err(format!("held {held:?}, {stats:?}"))
    }
}

fn misses_a_damaged_entry<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    let summary = first(summaries)?;
    put(&store, key(1), summary)?;
    put(&store, key(2), summary)?;
    h.damage(&store, &key(1))?;
    let whole = get(&store, key(1))?;
    let declarations = block_on(store.get_declarations(key(1))).map_err(|e| e.to_string())?;
    let StoreStats {
        discarded, entries, ..
    } = store.stats();
    if whole.is_some() || declarations.is_some() {
        return Err("a damaged entry answered".to_owned());
    }
    if discarded != 1 || entries != 1 {
        return Err(format!(
            "{discarded} discarded and {entries} held after one damaged entry of two"
        ));
    }
    put(&store, key(1), summary)?;
    match (get(&store, key(1))?, get(&store, key(2))?) {
        (Some(a), Some(b)) if a == *summary && b == *summary => Ok(()),
        _ => Err("a put after a damaged entry did not store it again".to_owned()),
    }
}

fn survives_a_reopen<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let summary = first(summaries)?;
    {
        let store = h.open(u64::MAX, clock())?;
        put(&store, key(7), summary)?;
    }
    let store = h.open(u64::MAX, clock())?;
    let stats = store.stats();
    match get(&store, key(7))? {
        Some(got) if got == *summary && stats.entries == 1 => Ok(()),
        other => Err(format!(
            "after a reopen: {} with {stats:?}",
            if other.is_some() {
                "another summary"
            } else {
                "a miss"
            }
        )),
    }
}

fn loses_only_time_when_wiped<H: Harness>(
    h: &mut H,
    summaries: &[FileSummary],
) -> Result<(), String> {
    let summary = first(summaries)?;
    {
        let store = h.open(u64::MAX, clock())?;
        put(&store, key(3), summary)?;
    }
    h.wipe()?;
    let store = h.open(u64::MAX, clock())?;
    if get(&store, key(3))?.is_some() {
        return Err("a wiped store still answered".to_owned());
    }
    put(&store, key(3), summary)?;
    match get(&store, key(3))? {
        Some(got) if got == *summary => Ok(()),
        _ => Err("a wiped store did not store again".to_owned()),
    }
}

fn indexes_reference_names<H: Harness>(h: &mut H, summaries: &[FileSummary]) -> Result<(), String> {
    let store = h.open(u64::MAX, clock())?;
    let declarations: Vec<Declarations> = summaries.iter().map(Declarations::of).collect();
    // The caller's ordinals, with gaps, given out of order: not positions.
    let ordinals: Vec<u32> = (0_u32..)
        .take(summaries.len())
        .map(|n| n.wrapping_mul(7).wrapping_add(3))
        .collect();
    let postings = store.postings(ordinals.iter().copied().zip(&declarations).rev());
    let mut names = BTreeSet::new();
    for (summary, &ordinal) in summaries.iter().zip(&ordinals) {
        for site in summary.references() {
            let name = summary.str(site.name);
            names.insert(name.to_owned());
            if !postings.files(name).contains(&ordinal) {
                return Err(format!("`{name}` does not list file {ordinal}"));
            }
        }
    }
    for name in &names {
        let files = postings.files(name);
        if !files.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
            return Err(format!(
                "`{name}` lists {files:?}, not ascending and distinct"
            ));
        }
        let want = summaries
            .iter()
            .filter(|s| s.references().iter().any(|r| s.str(r.name) == name))
            .count();
        if files.len() != want {
            return Err(format!("`{name}` lists {} files, want {want}", files.len()));
        }
    }
    if !postings.files("NoSuchNameInTheCorpus").is_empty() {
        return Err("an unused name lists files".to_owned());
    }
    if postings.names() == names.len() {
        Ok(())
    } else {
        Err(format!("{} names, want {}", postings.names(), names.len()))
    }
}

#[cfg(test)]
mod tests {
    use jfl_application::{StoreStats, SummaryStoreError};

    use super::*;
    use crate::fakes::summary_store::{Backing, MapPostings, MemorySummaryStore};

    struct FakeHarness(Backing);

    impl Harness for FakeHarness {
        type Store = MemorySummaryStore;

        fn open(&mut self, cap: u64, clock: Arc<dyn LruClock>) -> Result<Self::Store, String> {
            Ok(MemorySummaryStore::open(self.0.clone(), cap, clock))
        }

        fn damage(&mut self, store: &Self::Store, key: &SyntaxKey) -> Result<(), String> {
            store.damage(key);
            Ok(())
        }

        fn wipe(&mut self) -> Result<(), String> {
            self.0.wipe();
            Ok(())
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        /// Uses are not ticked: eviction by key order.
        IgnoresUses,
        /// Damage is never seen: the first summary put under the key answers.
        ServesDamaged,
        /// Answers the last summary put, whatever the key.
        AnswersAnotherKey,
        /// Declarations drop their last name.
        DropsAName,
        /// Postings drop each name's last file.
        DropsAPosting,
        /// Forgets everything on a reopen.
        ForgetsOnReopen,
        /// Answers a summary larger than the cap that it was asked to store.
        StoresTooLarge,
    }

    struct FrozenClock;

    impl LruClock for FrozenClock {
        fn now(&self) -> u64 {
            0
        }
    }

    struct Broken {
        inner: MemorySummaryStore,
        fault: Fault,
        cap: u64,
        copy: Arc<std::sync::Mutex<Vec<(SyntaxKey, FileSummary)>>>,
    }

    impl Broken {
        fn copy(&self) -> Vec<(SyntaxKey, FileSummary)> {
            self.copy.lock().unwrap().clone()
        }
    }

    impl SummaryStore for Broken {
        type Postings = MapPostings;

        fn get(
            &self,
            key: SyntaxKey,
        ) -> impl Future<Output = Result<Option<FileSummary>, SummaryStoreError>> + Send {
            let got = block_on(self.inner.get(key));
            let copy = self.copy();
            core::future::ready(match self.fault {
                Fault::ServesDamaged => {
                    Ok(copy.iter().find(|(k, _)| *k == key).map(|(_, s)| s.clone()))
                }
                Fault::AnswersAnotherKey => {
                    got.map(|g| g.and_then(|_| copy.last().map(|(_, s)| s.clone())))
                }
                Fault::StoresTooLarge => got.map(|g| {
                    g.or_else(|| {
                        copy.iter()
                            .find(|(k, s)| {
                                *k == key && u64::try_from(s.heap_bytes()).unwrap() > self.cap
                            })
                            .map(|(_, s)| s.clone())
                    })
                }),
                _ => got,
            })
        }

        fn get_declarations(
            &self,
            key: SyntaxKey,
        ) -> impl Future<Output = Result<Option<Declarations>, SummaryStoreError>> + Send {
            let got = block_on(self.inner.get_declarations(key));
            core::future::ready(match self.fault {
                Fault::DropsAName => got.map(|d| {
                    d.map(|d| {
                        let mut names = d.name_ids().to_vec();
                        names.pop();
                        Declarations::from_parts(d.summary().clone(), names.into()).unwrap()
                    })
                }),
                Fault::ServesDamaged => Ok(self
                    .copy()
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, s)| Declarations::of(s))),
                _ => got,
            })
        }

        fn put(
            &self,
            key: SyntaxKey,
            summary: Arc<FileSummary>,
        ) -> impl Future<Output = Result<(), SummaryStoreError>> + Send {
            self.copy.lock().unwrap().push((key, (*summary).clone()));
            self.inner.put(key, summary)
        }

        fn stats(&self) -> StoreStats {
            self.inner.stats()
        }

        fn postings<'a, I>(&self, files: I) -> MapPostings
        where
            I: IntoIterator<Item = (u32, &'a Declarations)>,
        {
            let files: Vec<_> = files.into_iter().collect();
            if self.fault != Fault::DropsAPosting {
                return MapPostings::build(files);
            }
            let trimmed: Vec<(u32, Declarations)> = files
                .iter()
                .map(|(n, d)| {
                    let mut names = d.name_ids().to_vec();
                    if *n == files.first().map_or(0, |f| f.0) {
                        names.pop();
                    }
                    (
                        *n,
                        Declarations::from_parts(d.summary().clone(), names.into()).unwrap(),
                    )
                })
                .collect();
            MapPostings::build(trimmed.iter().map(|(n, d)| (*n, d)))
        }
    }

    struct BrokenHarness {
        backing: Backing,
        fault: Fault,
        copy: Arc<std::sync::Mutex<Vec<(SyntaxKey, FileSummary)>>>,
    }

    impl Harness for BrokenHarness {
        type Store = Broken;

        fn open(&mut self, cap: u64, clock: Arc<dyn LruClock>) -> Result<Self::Store, String> {
            if self.fault == Fault::ForgetsOnReopen {
                self.backing.wipe();
            }
            let clock: Arc<dyn LruClock> = if self.fault == Fault::IgnoresUses {
                Arc::new(FrozenClock)
            } else {
                clock
            };
            Ok(Broken {
                inner: MemorySummaryStore::open(self.backing.clone(), cap, clock),
                fault: self.fault,
                cap,
                copy: Arc::clone(&self.copy),
            })
        }

        fn damage(&mut self, store: &Self::Store, key: &SyntaxKey) -> Result<(), String> {
            store.inner.damage(key);
            Ok(())
        }

        fn wipe(&mut self) -> Result<(), String> {
            self.backing.wipe();
            if self.fault != Fault::ServesDamaged {
                self.copy.lock().unwrap().clear();
            }
            Ok(())
        }
    }

    fn failed(fault: Fault) -> Vec<&'static str> {
        let mut harness = BrokenHarness {
            backing: Backing::new(),
            fault,
            copy: Arc::default(),
        };
        check(&mut harness).into_iter().map(|v| v.case).collect()
    }

    #[test]
    fn should_pass_every_case_when_the_memory_fake_answers() {
        // given
        let mut harness = FakeHarness(Backing::new());
        // when
        let violations = check(&mut harness);
        // then
        assert_eq!(violations, Vec::new());
    }

    #[test]
    fn should_fail_the_case_it_breaks_when_an_implementation_is_broken() {
        // given
        let faults = [
            (Fault::IgnoresUses, "evicts_least_recently_used"),
            (Fault::ServesDamaged, "misses_a_damaged_entry"),
            (Fault::AnswersAnotherKey, "reads_back_what_was_put"),
            (Fault::DropsAName, "takes_declarations_without_references"),
            (Fault::DropsAPosting, "indexes_reference_names"),
            (Fault::ForgetsOnReopen, "survives_a_reopen"),
            (
                Fault::StoresTooLarge,
                "refuses_a_summary_larger_than_the_cap",
            ),
        ];
        for (fault, case) in faults {
            // when
            let failed = failed(fault);
            // then
            assert!(
                failed.contains(&case),
                "{fault:?} failed {failed:?}, not {case}"
            );
        }
    }

    #[test]
    fn should_sort_keys_by_their_number_when_numbered() {
        // given
        let numbers = [0_u32, 1, 255, 256, u32::MAX];
        // when
        let keys = numbers.map(key);
        // then
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
    }
}
