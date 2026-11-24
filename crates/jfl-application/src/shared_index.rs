//! An ordered map whose clone shares its entries (IDX-09d): a generation's index, copied into a
//! build and published again, costs in proportion to what the build changes, not to the view.
//!
//! The entries are held in sorted chunks of at most [`MAX_CHUNK`] behind `Arc`s; a clone copies
//! the chunks' pointers (one per [`CHUNK`] entries or so), and a write copies the one chunk it
//! touches when another map still shares it (`Arc::make_mut`). Iteration is in key order, as a
//! `BTreeMap`'s, so every reader of an index sees the same order it saw before.
//!
//! **Reuse considered:** `im` and `rpds` (persistent maps, both MPL-2.0, which `deny.toml`'s
//! allowlist does not admit, and both larger than the few operations the index needs); `std`'s
//! `BTreeMap` behind an `Arc` per entry (the clone still walks every entry). The map is
//! hand-built over `std` only, as `jfl-application` takes no dependency for it.
//!
//! Pinned by: `shared_index::tests` (the property test against `BTreeMap`).

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

/// The size a chunk is cut to when it is built or split.
const CHUNK: usize = 64;
/// The size past which a chunk is split in two.
const MAX_CHUNK: usize = 2 * CHUNK;

/// An ordered map from `K` to `V` whose clone shares its entries; see the module.
pub struct SharedIndex<K, V> {
    /// Non-empty chunks, each sorted, every key of one below every key of the next.
    chunks: Vec<Arc<Vec<(K, V)>>>,
    len: usize,
}

impl<K, V> Clone for SharedIndex<K, V> {
    fn clone(&self) -> Self {
        Self {
            chunks: self.chunks.clone(),
            len: self.len,
        }
    }
}

impl<K, V> Default for SharedIndex<K, V> {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for SharedIndex<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: PartialEq, V: PartialEq> PartialEq for SharedIndex<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

impl<K: Eq, V: Eq> Eq for SharedIndex<K, V> {}

impl<K, V> SharedIndex<K, V> {
    /// An empty map.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }

    /// How many entries it holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether it holds none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> + '_ {
        self.chunks
            .iter()
            .flat_map(|chunk| chunk.iter().map(|(k, v)| (k, v)))
    }

    /// The keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.iter().map(|(k, _)| k)
    }

    /// The values in key order.
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, v)| v)
    }
}

impl<K: Ord, V> SharedIndex<K, V> {
    /// The chunk that holds `key` or would: the last whose first key is not above it.
    fn chunk_of<Q: Ord + ?Sized>(&self, key: &Q) -> usize
    where
        K: Borrow<Q>,
    {
        self.chunks
            .partition_point(|c| c.first().is_some_and(|(k, _)| k.borrow() <= key))
            .saturating_sub(1)
    }

    /// The value of `key`.
    pub fn get<Q: Ord + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        let chunk = self.chunks.get(self.chunk_of(key))?;
        let at = chunk.binary_search_by(|(k, _)| k.borrow().cmp(key)).ok()?;
        chunk.get(at).map(|(_, v)| v)
    }

    /// Whether it holds `key`.
    pub fn contains_key<Q: Ord + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.get(key).is_some()
    }
}

impl<K: Ord + Clone, V: Clone> SharedIndex<K, V> {
    /// Sets `key` to `value`; the value it replaced. Copies the one chunk it writes when another
    /// map shares it.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.chunks.is_empty() {
            self.chunks.push(Arc::new(vec![(key, value)]));
            self.len = 1;
            return None;
        }
        let at = self.chunk_of(&key);
        let chunk = Arc::make_mut(self.chunks.get_mut(at)?);
        match chunk.binary_search_by(|(k, _)| k.cmp(&key)) {
            Ok(i) => chunk
                .get_mut(i)
                .map(|slot| core::mem::replace(&mut slot.1, value)),
            Err(i) => {
                chunk.insert(i, (key, value));
                self.len = self.len.saturating_add(1);
                if chunk.len() > MAX_CHUNK {
                    let rest = chunk.split_off(chunk.len() / 2);
                    self.chunks.insert(at.saturating_add(1), Arc::new(rest));
                }
                None
            }
        }
    }

    /// Removes `key`; its value.
    pub fn remove<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        let at = self.chunk_of(key);
        let found = self
            .chunks
            .get(at)?
            .binary_search_by(|(k, _)| k.borrow().cmp(key))
            .ok()?;
        let shared = self.chunks.get_mut(at)?;
        let chunk = Arc::make_mut(shared);
        let (_, value) = chunk.remove(found);
        self.len = self.len.saturating_sub(1);
        if chunk.is_empty() {
            self.chunks.remove(at);
        }
        Some(value)
    }
}

impl<K: Ord, V> FromIterator<(K, V)> for SharedIndex<K, V> {
    /// The map of `entries`; of two entries with one key, the later is kept.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Self {
        let mut all: Vec<(K, V)> = entries.into_iter().collect();
        // A stable sort keeps equal keys in their order, so the last of them is the later one.
        all.sort_by(|a, b| a.0.cmp(&b.0));
        let mut unique: Vec<(K, V)> = Vec::with_capacity(all.len());
        for entry in all {
            match unique.last_mut() {
                Some(last) if last.0 == entry.0 => *last = entry,
                _ => unique.push(entry),
            }
        }
        let len = unique.len();
        let mut chunks = Vec::with_capacity(len.div_ceil(CHUNK));
        let mut rest = unique.into_iter().peekable();
        while rest.peek().is_some() {
            chunks.push(Arc::new(rest.by_ref().take(CHUNK).collect()));
        }
        Self { chunks, len }
    }
}

impl<'a, K, V> IntoIterator for &'a SharedIndex<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (&'a K, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u16, u8),
        Remove(u16),
        Clone,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (0..600_u16, any::<u8>()).prop_map(|(k, v)| Op::Insert(k, v)),
            2 => (0..600_u16).prop_map(Op::Remove),
            1 => Just(Op::Clone),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        #[test]
        fn should_read_as_a_btree_map_when_any_writes_and_clones_are_applied(
            start in proptest::collection::vec((0..600_u16, any::<u8>()), 0..300),
            ops in proptest::collection::vec(op(), 0..400),
        ) {
            // given
            let mut map: SharedIndex<u16, u8> = start.iter().copied().collect();
            let mut reference: BTreeMap<u16, u8> = start.iter().copied().collect();
            let mut earlier = Vec::new();

            // when
            for op in ops {
                match op {
                    Op::Insert(k, v) => prop_assert_eq!(map.insert(k, v), reference.insert(k, v)),
                    Op::Remove(k) => prop_assert_eq!(map.remove(&k), reference.remove(&k)),
                    Op::Clone => earlier.push((map.clone(), reference.clone())),
                }
            }

            // then: the map, and every clone taken along the way, reads as its reference did
            for (map, reference) in earlier.iter().chain([(map, reference)].iter()) {
                prop_assert_eq!(map.len(), reference.len());
                prop_assert!(map.iter().eq(reference.iter()));
                for k in 0..600_u16 {
                    prop_assert_eq!(map.get(&k), reference.get(&k));
                }
            }
        }
    }

    #[test]
    fn should_copy_only_the_written_chunk_when_a_clone_is_written() {
        // given
        let map: SharedIndex<u32, u32> = (0..1000).map(|k| (k, k)).collect();
        let mut copy = map.clone();

        // when
        copy.insert(500, 0);

        // then: every chunk but the written one is still shared
        let shared = map
            .chunks
            .iter()
            .zip(&copy.chunks)
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count();
        assert_eq!(shared, map.chunks.len() - 1);
        assert_eq!((map.get(&500), copy.get(&500)), (Some(&500), Some(&0)));
    }
}
