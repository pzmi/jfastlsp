//! Query positions picked from the corpus by seed.
//!
//! A benchmark that queries names someone chose by hand measures what they remembered; one that
//! picks by seed measures the corpus. The same seed and the same pinned commit give the same
//! queries on every machine (`rng`'s stream, the sorted file list), so two runs and two machines
//! compare like with like. A query is the first top-level type declared in a picked file: its
//! name for `rg-floor`, and its file, line and column for the position-keyed suites that arrive
//! with the product. Names are looked up at bench time and never written into this repository
//! (nothing from the corpus is committed, doc/bench-corpus.md).
//!
//! Pinned by: `tests/queries.rs` (determinism, uniqueness) and this module's unit tests.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::e1;
use crate::error::{Context, Result};
use crate::rng::SplitMix64;

/// A picked declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// The type's simple name.
    pub name: String,
    /// The file, relative to the corpus root.
    pub file: String,
    /// Zero-based line of the name.
    pub line: u32,
    /// Zero-based column of the name, counted in characters.
    pub column: u32,
}

/// The first `class`, `interface`, `enum` or `record` declaration in `src`: its name, zero-based
/// line and column. Comment lines are skipped, `@interface` is not a type name here, and a
/// name must start with an uppercase ASCII letter (Java's convention, and what makes a query
/// selective).
pub fn first_type_decl(src: &str) -> Option<(String, u32, u32)> {
    let mut in_block = false;
    for (line_no, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        if in_block {
            in_block = !trimmed.contains("*/");
            continue;
        }
        if trimmed.starts_with("/*") {
            in_block = !trimmed.contains("*/");
            continue;
        }
        if trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }
        let mut after_keyword = false;
        for (start, word) in words(line) {
            if after_keyword && word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                let column = line.get(..start).map_or(0, |p| p.chars().count());
                return Some((
                    word.to_string(),
                    u32::try_from(line_no).unwrap_or(u32::MAX),
                    u32::try_from(column).unwrap_or(u32::MAX),
                ));
            }
            let before = line.get(..start).unwrap_or("");
            let member_or_annotation = before.ends_with('.') || before.ends_with('@');
            after_keyword =
                !member_or_annotation && matches!(word, "class" | "interface" | "enum" | "record");
        }
    }
    None
}

/// The identifier-like words of a line with their byte offsets.
fn words(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ' ')))
    {
        let in_word = c.is_alphanumeric() || c == '_' || c == '$';
        match (start, in_word) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                if let Some(w) = line.get(s..i) {
                    out.push((s, w));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// `count` distinct queries picked from the `.java` files under `root` by `seed`.
///
/// Files are shuffled (partial Fisher-Yates over the sorted list) and read in that order until
/// enough distinct names are found; unreadable or declaration-free files (`package-info.java`)
/// are skipped. Fewer than `count` are returned only when the corpus has fewer distinct names.
pub fn pick(root: &Path, count: usize, seed: u64) -> Result<Vec<Query>> {
    let mut files = e1::list_java(root);
    let mut rng = SplitMix64::new(seed);
    let mut out: Vec<Query> = Vec::new();
    let mut next = 0usize;
    while out.len() < count && next < files.len() {
        let j = next.saturating_add(rng.below(files.len().saturating_sub(next)));
        files.swap(next, j);
        let Some(path) = files.get(next) else { break };
        next = next.saturating_add(1);
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let src = String::from_utf8_lossy(&bytes);
        let Some((name, line, column)) = first_type_decl(&src) else {
            continue;
        };
        if out.iter().any(|q| q.name == name) {
            continue;
        }
        let rel = path.strip_prefix(root).context("strip the corpus root")?;
        out.push(Query {
            name,
            file: rel.display().to_string(),
            line,
            column,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_find_the_name_and_position_when_a_class_is_declared() {
        // given
        let src = "package a.b;\n\nimport java.util.List;\n\npublic final class Widget<T> {\n}\n";
        // when
        let found = first_type_decl(src);
        // then
        assert_eq!(found, Some(("Widget".to_string(), 4, 19)));
    }

    #[test]
    fn should_skip_comments_when_a_comment_mentions_class() {
        // given
        let src = "/**\n * A class Fake for docs.\n */\n// class Other\ninterface Real {}\n";
        // when
        let found = first_type_decl(src);
        // then
        assert_eq!(found.map(|(n, l, _)| (n, l)), Some(("Real".to_string(), 4)));
    }

    #[test]
    fn should_find_records_and_enums_when_they_come_first() {
        // given
        let record = "public record Point(int x, int y) {}\n";
        let enumeration = "enum Color { RED }\n";
        // when
        // then
        assert_eq!(
            first_type_decl(record).map(|t| t.0),
            Some("Point".to_string())
        );
        assert_eq!(
            first_type_decl(enumeration).map(|t| t.0),
            Some("Color".to_string())
        );
    }

    #[test]
    fn should_find_nothing_when_the_file_declares_no_type() {
        // given
        let src = "/** package docs */\npackage a;\n";
        // when
        // then
        assert_eq!(first_type_decl(src), None);
        assert_eq!(first_type_decl("@interface Marker {}\n").map(|t| t.0), None);
    }

    #[test]
    fn should_not_take_a_lowercase_word_when_it_follows_class() {
        // given: `Foo.class` style text and a generic lower-case word after the keyword
        let src = "var c = Foo.class;\nclass x {}\nclass Real {}\n";
        // when
        let found = first_type_decl(src);
        // then
        assert_eq!(found.map(|t| t.0), Some("Real".to_string()));
    }
}
