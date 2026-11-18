//! The large tier's name → files index (plan §5.5): every distinct reference name once, sorted, in
//! one text arena, and each name's file ordinals in one flat array.
//!
//! **Why a sorted table, not `fst`.** The index is rebuilt at every view open (plan §4), so its
//! build counts against the warm reopen (plan §1.7: under 1 s on the large corpus). In a one-off
//! measurement on 2026-11-18 (4 vCPU Xeon VM with another lane running; spring-framework, 44.4k
//! names, 310k postings), a version of this index over an `fst` map built in 101–126 ms in
//! `bench-store`'s postings step, against 50–54 ms for this table. A throwaway timing in the same
//! run put the `fst` build itself at about 49 ms (about 1 µs per name) against 0.6 ms for the table.
//! Lookups were no faster: 211 ns against 190 ns per name. The table costs about 0.6 MB more. The
//! comparison arm was not kept, since it would add `fst` to the graph for a bench alone. ADR 0025
//! names `fst` for `workspace/symbol`'s prefix and fuzzy lookups (IDX-10), where its automata pay;
//! an exact name lookup does not need them (plan §5.5 records the choice). Rejected too: a
//! `HashMap<Box<str>, Vec<u32>>` kept as the index (two allocations and about 60 bytes per name),
//! and `rusqlite` (a C build and a query per lookup, for an index that fits in memory; ADR 0025
//! keeps it for manifests, which DEC-03's tier does not need).
//!
//! **The build hashes each use once** (std's SipHash: the names are read from untrusted sources,
//! so a hasher an input can collide on is not used) and sorts only the distinct names.
//!
//! **Sound if it cannot be built**: should a table pass its 32-bit offsets, the index answers every
//! file for every name, which keeps a `refs` query complete at the cost of reading every file,
//! never missing one.

use std::collections::HashMap;

use jfl_application::{Declarations, NamePostings};

#[derive(Debug)]
enum Inner {
    Indexed {
        /// Every name, sorted, concatenated.
        text: Box<str>,
        /// Name `i` is `text[starts[i]..starts[i + 1]]`; one entry more than there are names.
        starts: Box<[u32]>,
        /// Name `i`'s files are `files[offsets[i]..offsets[i + 1]]`.
        offsets: Box<[u32]>,
        files: Box<[u32]>,
    },
    /// The table could not be built: every name lists every file.
    Everything { files: Box<[u32]>, names: usize },
}

/// The sorted-table [`NamePostings`].
#[derive(Debug)]
pub struct SortedPostings(Inner);

/// Too large for 32-bit offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TooLarge;

fn offset(len: usize) -> Result<u32, TooLarge> {
    u32::try_from(len).map_err(|_| TooLarge)
}

/// The table of `rows`: each distinct name in sorted order, with its files ascending and distinct.
fn table(rows: &[(&str, &[u32])]) -> Result<Inner, TooLarge> {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(rows.len().saturating_add(1));
    let mut offsets = Vec::with_capacity(rows.len().saturating_add(1));
    let mut files = Vec::new();
    for (name, ordinals) in rows {
        starts.push(offset(text.len())?);
        offsets.push(offset(files.len())?);
        text.push_str(name);
        files.extend_from_slice(ordinals);
    }
    starts.push(offset(text.len())?);
    offsets.push(offset(files.len())?);
    Ok(Inner::Indexed {
        text: text.into_boxed_str(),
        starts: starts.into_boxed_slice(),
        offsets: offsets.into_boxed_slice(),
        files: files.into_boxed_slice(),
    })
}

/// Entry `at` of `bounds` to the next, as a range.
fn span(bounds: &[u32], at: usize) -> Option<core::ops::Range<usize>> {
    let start = usize::try_from(*bounds.get(at)?).ok()?;
    let end = usize::try_from(*bounds.get(at.checked_add(1)?)?).ok()?;
    Some(start..end)
}

impl SortedPostings {
    /// The index of `files`: each file's ordinal and its declarations.
    ///
    /// Pinned by: the `summary_store` contract suite's `indexes_reference_names` and
    /// `postings::tests`.
    pub fn build<'a, I>(files: I) -> Self
    where
        I: IntoIterator<Item = (u32, &'a Declarations)>,
    {
        let mut ids: HashMap<&'a str, usize> = HashMap::new();
        let mut uses: Vec<Vec<u32>> = Vec::new();
        let mut every = Vec::new();
        for (ordinal, declarations) in files {
            every.push(ordinal);
            for name in declarations.names() {
                let next = uses.len();
                let id = *ids.entry(name).or_insert(next);
                if id == next {
                    uses.push(Vec::new());
                }
                if let Some(files) = uses.get_mut(id) {
                    files.push(ordinal);
                }
            }
        }
        let mut names: Vec<(&'a str, usize)> = ids.into_iter().collect();
        names.sort_unstable();
        for files in &mut uses {
            files.sort_unstable();
            files.dedup();
        }
        let rows: Vec<(&str, &[u32])> = names
            .iter()
            .map(|&(name, id)| (name, uses.get(id).map_or(&[][..], Vec::as_slice)))
            .collect();
        Self(table(&rows).unwrap_or_else(|TooLarge| {
            every.sort_unstable();
            every.dedup();
            Inner::Everything {
                files: every.into_boxed_slice(),
                names: rows.len(),
            }
        }))
    }
}

impl NamePostings for SortedPostings {
    fn files(&self, name: &str) -> &[u32] {
        match &self.0 {
            Inner::Indexed {
                text,
                starts,
                offsets,
                files,
            } => {
                let count = starts.len().saturating_sub(1);
                let name_at = |at: usize| span(starts, at).and_then(|r| text.get(r)).unwrap_or("");
                let (mut low, mut high) = (0_usize, count);
                while low < high {
                    let mid = low.midpoint(high);
                    if name_at(mid) < name {
                        low = mid.saturating_add(1);
                    } else {
                        high = mid;
                    }
                }
                if low < count && name_at(low) == name {
                    span(offsets, low).and_then(|r| files.get(r)).unwrap_or(&[])
                } else {
                    &[]
                }
            }
            Inner::Everything { files, .. } => files,
        }
    }

    fn names(&self) -> usize {
        match &self.0 {
            Inner::Indexed { starts, .. } => starts.len().saturating_sub(1),
            Inner::Everything { names, .. } => *names,
        }
    }

    fn heap_bytes(&self) -> usize {
        match &self.0 {
            Inner::Indexed {
                text,
                starts,
                offsets,
                files,
            } => text
                .len()
                .saturating_add(starts.len().saturating_mul(4))
                .saturating_add(offsets.len().saturating_mul(4))
                .saturating_add(files.len().saturating_mul(4)),
            Inner::Everything { files, .. } => files.len().saturating_mul(4),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_find_each_name_and_no_other_when_looked_up() {
        // given
        let rows: [(&str, &[u32]); 4] = [("", &[0]), ("a", &[1, 5]), ("ab", &[2]), ("b", &[3])];
        let postings = SortedPostings(table(&rows).unwrap());
        // when
        let found = ["", "a", "ab", "b", "aa", "c", "B"].map(|n| postings.files(n));
        // then
        assert_eq!(found, [&[0][..], &[1, 5], &[2], &[3], &[], &[], &[]]);
        assert_eq!(postings.names(), 4);
    }

    #[test]
    fn should_list_every_file_for_any_name_when_the_table_could_not_be_built() {
        // given
        let postings = SortedPostings(Inner::Everything {
            files: vec![1, 4, 9].into_boxed_slice(),
            names: 2,
        });
        // when
        let files = [postings.files("anything"), postings.files("")];
        // then
        assert_eq!(files, [&[1, 4, 9][..], &[1, 4, 9][..]]);
        assert_eq!(postings.names(), 2);
    }
}
