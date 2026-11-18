//! The `SyntaxIndexer` port (plan §6.4): a Java file's bytes in, its [`FileSummary`] and its syntax
//! diagnostics out, or a typed refusal.
//!
//! **What every implementation does with the bytes first** is a pure function here,
//! [`source_text`], so the testkit fake and the tree-sitter adapter refuse the same inputs and the
//! `syntax_indexer` contract suite (`jfl_testkit::contracts::syntax_indexer`) checks that each does:
//! a leading UTF-8 byte-order mark is dropped (editors drop it from document text, so offsets
//! count from after it; the `SyntaxKey`'s file digest is of the same bytes, `keys::SyntaxInputs`),
//! a file over [`MAX_SOURCE_BYTES`] is refused before it is parsed, bytes that are not UTF-8 are
//! refused (every range of a summary is a byte range of UTF-8 text, plan §2.3), and so is a file
//! nested deeper than [`MAX_NESTING`] brackets. A refused file is a gap the view reports: it is
//! never indexed as empty, so no answer counts it as seen (ADR 0014).
//!
//! **Bounded work.** Parsing hostile bytes must end (plan §11, ver §8): an implementation stops
//! after a work budget proportional to the input ([`work_budget`]) and refuses with
//! [`SyntaxIndexError::TooMuchWork`], which is again a gap and never a partial summary passed off
//! as whole.

use core::future::Future;

use jfl_domain::{ByteRange, FileSummary, SummaryError};

/// The largest file indexed: 16 MiB (the pinned corpus's largest `.java` is 1.6 MB). A larger file
/// is refused before it is parsed.
pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// The deepest bracket nesting accepted, counted over the raw bytes (`(`, `[` and `{`, inside
/// strings and comments too). A bracket spelled as a Unicode escape (`\u0028`) is not counted:
/// this is a cheap first refusal, not the depth bound. The depth of what is parsed is bounded by
/// the work budget ([`work_budget`]), and every walk of the tree is iterative, so a deeper tree
/// costs time within that budget and never a stack (`jfl-adapter-java`'s `tests/properties.rs`).
/// Real code stays far below it (`javac`'s own recursive parser fails some thousands deep).
pub const MAX_NESTING: u32 = 4096;

/// The most syntax diagnostics one file reports; [`Indexed::diagnostics_dropped`] counts the rest.
pub const MAX_DIAGNOSTICS: usize = 100;

/// The UTF-8 byte-order mark.
pub const BOM: &[u8] = b"\xEF\xBB\xBF";

/// The grammar an indexer parses with, by its crate name and exact version: both are inputs of
/// the `SyntaxKey` (plan §5.4, ADR 0022), so a switch of grammar re-parses every file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GrammarId {
    /// The grammar crate's name, such as `tree-sitter-java-orchard`.
    pub name: &'static str,
    /// Its exact version, such as `0.5.25`.
    pub version: &'static str,
}

/// What a syntax diagnostic reports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SyntaxProblem {
    /// Text the grammar could not place: the range covers it.
    Unexpected,
    /// Something the grammar expected and did not find, at an empty range: a token as written
    /// (`;`, `)`), or the name of a construct (`identifier`).
    Missing(Box<str>),
    /// Syntax that parses but is not Java: a string template (preview in Java 21 and 22, withdrawn
    /// in 23; ADR 0022).
    StringTemplate,
}

/// A syntax error, located.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxDiagnostic {
    /// Where, in bytes of the text after a dropped byte-order mark. A range never ends between
    /// the `\r` and `\n` of a line terminator (`LineIndex` refuses such a position).
    pub range: ByteRange,
    /// What.
    pub problem: SyntaxProblem,
}

/// A file's syntax index entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indexed {
    /// The summary: what declarations and references the syntax shows, with the parts it could
    /// read when the file has syntax errors.
    pub summary: FileSummary,
    /// The syntax errors, in source order, at most [`MAX_DIAGNOSTICS`].
    pub diagnostics: Vec<SyntaxDiagnostic>,
    /// How many more there were.
    pub diagnostics_dropped: u32,
}

/// Why a file was not indexed. A refused file is a gap, never an empty summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SyntaxIndexError {
    /// Larger than [`MAX_SOURCE_BYTES`].
    #[error("file of {len} bytes is larger than the {MAX_SOURCE_BYTES}-byte limit")]
    TooLarge {
        /// Its length, after a byte-order mark.
        len: usize,
    },
    /// Not UTF-8.
    #[error("file is not UTF-8 from byte {valid_up_to}")]
    NotUtf8 {
        /// How many leading bytes are valid.
        valid_up_to: usize,
    },
    /// Nested deeper than [`MAX_NESTING`].
    #[error("file nests brackets deeper than {MAX_NESTING}")]
    TooDeep,
    /// The parse or the extraction used up its work budget ([`work_budget`]).
    #[error("file needed more work than its budget allows")]
    TooMuchWork,
    /// The summary outgrew its 32-bit indices.
    #[error("summary refused: {0}")]
    Summary(#[from] SummaryError),
    /// The pool shut down, or the job panicked.
    #[error("the indexing job was interrupted")]
    Interrupted,
}

/// Indexes Java source.
pub trait SyntaxIndexer: Send + Sync + 'static {
    /// The grammar, for the `SyntaxKey`.
    fn grammar(&self) -> GrammarId;

    /// The summariser's format version, for the `SyntaxKey`: bumped whenever the same bytes would
    /// summarise differently.
    fn summariser_version(&self) -> u32;

    /// Indexes `bytes`, a whole file as read from disk (a leading byte-order mark included).
    ///
    /// Deterministic: the same bytes give the same result, whatever ran before. Never panics on any
    /// input (`jfl-adapter-java`'s `tests/properties.rs`).
    fn index(
        &self,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send;
}

/// `bytes` without a leading byte-order mark.
#[must_use]
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(BOM).unwrap_or(bytes)
}

/// The text an indexer reads from `bytes`: a byte-order mark dropped, then the size, UTF-8 and
/// nesting checks of the module documentation, in that order.
///
/// Pinned by: `syntax_indexer::tests::should_refuse_each_bound_when_the_input_breaks_it` and the
/// contract suite's `refuses_what_it_cannot_index`.
pub fn source_text(bytes: &[u8]) -> Result<&str, SyntaxIndexError> {
    let bytes = strip_bom(bytes);
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(SyntaxIndexError::TooLarge { len: bytes.len() });
    }
    let text = core::str::from_utf8(bytes).map_err(|e| SyntaxIndexError::NotUtf8 {
        valid_up_to: e.valid_up_to(),
    })?;
    let mut depth = 0_u32;
    for &byte in bytes {
        match byte {
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                if depth > MAX_NESTING {
                    return Err(SyntaxIndexError::TooDeep);
                }
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(text)
}

/// The work an implementation may spend on a text of `len` bytes, in tree-sitter progress
/// callbacks (one per 100 parser operations): a fixed allowance plus one per byte, so 100
/// operations per byte. No file of the pinned corpus is refused by it (`just bench-syntax`,
/// `doc/bench/README.md`); arbitrary bytes end within it (`jfl-adapter-java`'s
/// `tests/properties.rs`).
#[must_use]
pub const fn work_budget(len: usize) -> usize {
    len.saturating_add(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_each_bound_when_the_input_breaks_it() {
        // given
        let large = vec![b' '; MAX_SOURCE_BYTES + 1];
        let deep = "(".repeat(usize::try_from(MAX_NESTING).unwrap() + 1);
        let at_limit = "(".repeat(usize::try_from(MAX_NESTING).unwrap());
        // when
        let refused = [
            source_text(&large),
            source_text(b"class A { \xFF }"),
            source_text(deep.as_bytes()),
        ];
        // then
        assert_eq!(
            refused,
            [
                Err(SyntaxIndexError::TooLarge {
                    len: MAX_SOURCE_BYTES + 1
                }),
                Err(SyntaxIndexError::NotUtf8 { valid_up_to: 10 }),
                Err(SyntaxIndexError::TooDeep),
            ]
        );
        assert_eq!(source_text(at_limit.as_bytes()), Ok(at_limit.as_str()));
    }

    #[test]
    fn should_drop_only_a_leading_bom_when_reading_source() {
        // given
        let with_bom = b"\xEF\xBB\xBFclass A {}";
        let inner_bom = b"class A {}\xEF\xBB\xBF";
        // when
        let stripped = source_text(with_bom);
        let kept = source_text(inner_bom);
        // then
        assert_eq!(stripped, Ok("class A {}"));
        assert_eq!(kept, Ok("class A {}\u{FEFF}"));
    }
}
