//! The parsed trees of the files whose sites the queries asked about last (IDX-18): a definition,
//! a hover or a references answer parses its files for the syntax facts ([`crate::scopes`]), and
//! an agent or an editor asks several sites of one file in a row, so the last few trees are kept
//! and each later site of the same text is a walk, not a parse.
//!
//! **Keyed by the text itself**, compared whole (the shared text first by pointer): the port hands
//! the indexer a text, never a key, and the same text parses to the same tree, so a kept tree
//! gives the facts a fresh parse would, the work budget included (the parse's units are charged
//! again before each walk, [`scopes::Parsed`]). A file that changed is another text and misses.
//!
//! **Bounded** by a count of trees and by the bytes of their texts ([`TREE_CACHE_FILES`],
//! [`TREE_CACHE_BYTES`]): the least recently asked is let go first, and a text over the byte bound
//! is parsed and never kept. A tree's size follows its nodes, not its bytes: the corpus's largest
//! files cost about 11 to 14 times their text (the queries bench's `--memory`,
//! `doc/bench/README.md`), so a full cache holds about 14 MiB of trees for them; the worst case
//! is linear in the 1 MiB text bound, a larger multiple for a file of tiny tokens, and no more
//! than one fresh parse of the same texts allocates. The lock is held to find or insert an
//! entry, never across a parse.
//!
//! Pinned by: `trees::tests`, and every site of the fixture projects asked through the cache
//! against javac's bindings (`tests/definitions/main.rs`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use jfl_application::FileText;

use crate::scopes::Parsed;

/// The most trees kept.
pub const TREE_CACHE_FILES: usize = 16;

/// The most bytes of text the kept trees may have been parsed from.
pub const TREE_CACHE_BYTES: usize = 1024 * 1024;

/// The kept trees, least recently asked first.
pub(crate) struct Trees {
    files: usize,
    bytes: usize,
    held: Mutex<VecDeque<(Arc<FileText>, Arc<Parsed>)>>,
}

impl core::fmt::Debug for Trees {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Trees")
            .field("files", &self.files)
            .field("bytes", &self.bytes)
            .field("held", &held.len())
            .finish()
    }
}

impl Trees {
    /// At most `files` trees of at most `bytes` of text in all; `0` keeps none.
    pub(crate) fn new(files: usize, bytes: usize) -> Self {
        Self {
            files,
            bytes,
            held: Mutex::new(VecDeque::new()),
        }
    }

    /// The tree of `text`: the kept one, or `parse`'s, kept unless it is over the bounds.
    pub(crate) fn get_or_parse(
        &self,
        text: &Arc<FileText>,
        parse: impl FnOnce() -> Parsed,
    ) -> Arc<Parsed> {
        if let Some(parsed) = self.get(text) {
            return parsed;
        }
        let parsed = Arc::new(parse());
        self.put(text, &parsed);
        parsed
    }

    fn get(&self, text: &Arc<FileText>) -> Option<Arc<Parsed>> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let at = held
            .iter()
            .position(|(kept, _)| Arc::ptr_eq(kept, text) || kept.text == text.text)?;
        let entry = held.remove(at)?;
        let parsed = Arc::clone(&entry.1);
        held.push_back(entry);
        Some(parsed)
    }

    fn put(&self, text: &Arc<FileText>, parsed: &Arc<Parsed>) {
        let len = text.text.len();
        if self.files == 0 || len > self.bytes {
            return;
        }
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        // Another thread may have parsed the same text meanwhile: keep one.
        if held.iter().any(|(kept, _)| kept.text == text.text) {
            return;
        }
        held.push_back((Arc::clone(text), Arc::clone(parsed)));
        let mut total: usize = held.iter().map(|(kept, _)| kept.text.len()).sum();
        while held.len() > self.files || total > self.bytes {
            let Some((gone, _)) = held.pop_front() else {
                break;
            };
            total = total.saturating_sub(gone.text.len());
        }
    }

    /// How many trees are kept, and the bytes of their texts.
    #[cfg(test)]
    fn held(&self) -> (usize, usize) {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        (
            held.len(),
            held.iter().map(|(kept, _)| kept.text.len()).sum(),
        )
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use jfl_domain::{ByteOffset, ByteRange, Language};

    use super::*;
    use crate::extract::Worker;
    use crate::grammar::{Grammar, GrammarSpec};
    use crate::scopes::{facts, facts_of, parse_for_facts};

    fn text(source: &str) -> Arc<FileText> {
        Arc::new(FileText::new(Language::Java, source.to_owned()).unwrap())
    }

    /// The ranges of every occurrence of `name` in `source`.
    fn sites(source: &str, name: &str) -> Vec<ByteRange> {
        source
            .match_indices(name)
            .map(|(at, _)| {
                let start = u32::try_from(at).unwrap();
                let end = start
                    .checked_add(u32::try_from(name.len()).unwrap())
                    .unwrap();
                ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
            })
            .collect()
    }

    const SOURCE: &str = "package p;\nclass A extends B {\n  int size;\n  int size() { return size; }\n  void run(int n) { size(); this.size = n; new A(); }\n}\nclass B { int run; }\n";

    #[test]
    fn should_give_a_fresh_parse_s_facts_when_a_kept_tree_is_walked_again() {
        // given: the facts of every site of a few names, parsed afresh for each
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let trees = Trees::new(TREE_CACHE_FILES, TREE_CACHE_BYTES);
        let shared = text(SOURCE);
        let escaped = text("class C { int \\u0061; void m() { a = 1; } }\n");
        let broken = text("class D { void m( { size(); } }\n");
        for (file, name) in [
            (&shared, "size"),
            (&shared, "run"),
            (&shared, "A"),
            (&escaped, "a"),
            (&broken, "size"),
        ] {
            let ranges = sites(&file.text, name);
            let fresh = facts(&grammar, &mut Worker::new(), &file.text, &ranges);
            // when: the same sites over the cache, twice (a parse, then the kept tree)
            let parse = || parse_for_facts(&grammar, &mut Worker::new(), &file.text);
            let first = facts_of(&trees.get_or_parse(file, parse), &file.text, &ranges);
            let again = facts_of(
                &trees.get_or_parse(file, || unreachable!("kept")),
                &file.text,
                &ranges,
            );
            // then
            assert_eq!(first, fresh, "{name}");
            assert_eq!(again, fresh, "{name}");
        }
        assert!(
            facts(&grammar, &mut Worker::new(), SOURCE, &sites(SOURCE, "size"))
                .0
                .iter()
                .any(Option::is_some)
        );
    }

    #[test]
    fn should_parse_once_when_an_equal_text_is_asked_again() {
        // given: a text, and another holding the same characters
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let trees = Trees::new(TREE_CACHE_FILES, TREE_CACHE_BYTES);
        let parses = Cell::new(0_u32);
        let parse = |t: &str| {
            parses.set(parses.get().saturating_add(1));
            parse_for_facts(&grammar, &mut Worker::new(), t)
        };
        let first = text(SOURCE);
        let equal = text(SOURCE);
        let other = text("class Z {}\n");
        // when
        let a = trees.get_or_parse(&first, || parse(&first.text));
        let b = trees.get_or_parse(&equal, || parse(&equal.text));
        let _ = trees.get_or_parse(&other, || parse(&other.text));
        // then
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(parses.get(), 2);
    }

    #[test]
    fn should_keep_at_most_its_bounds_and_let_the_oldest_go_when_more_are_asked() {
        // given: room for three trees or 64 bytes of text
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let trees = Trees::new(3, 64);
        let parses = Cell::new(0_u32);
        let ask = |t: &Arc<FileText>| {
            trees.get_or_parse(t, || {
                parses.set(parses.get().saturating_add(1));
                parse_for_facts(&grammar, &mut Worker::new(), &t.text)
            })
        };
        let small: Vec<Arc<FileText>> = (0..4)
            .map(|i| text(&format!("class S{i} {{}}\n")))
            .collect();
        let large = text(&format!("class L {{ /* {} */ }}\n", "x".repeat(80)));
        // when: four small texts, then the first again, then one over the byte bound twice
        for t in &small {
            let _ = ask(t);
        }
        let held_after_four = trees.held();
        let _ = ask(small.first().unwrap());
        let _ = ask(&large);
        let _ = ask(&large);
        // then: three kept; the first was let go, so it parsed again; the large one never kept
        assert_eq!(held_after_four.0, 3);
        assert!(held_after_four.1 <= 64);
        assert_eq!(parses.get(), 7);
        assert_eq!(trees.held().0, 3);
    }

    #[test]
    fn should_keep_nothing_when_its_bounds_are_zero() {
        // given
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let trees = Trees::new(0, TREE_CACHE_BYTES);
        let shared = text(SOURCE);
        // when
        let _ = trees.get_or_parse(&shared, || {
            parse_for_facts(&grammar, &mut Worker::new(), &shared.text)
        });
        // then
        assert_eq!(trees.held(), (0, 0));
    }
}
