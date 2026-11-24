//! The `SyntaxIndexer` port (plan §6.4): a source file's bytes in, its [`FileSummary`] and its
//! syntax diagnostics out, or a typed refusal. **One indexer per language** (IDX-17): each names
//! its [`Language`] ([`SyntaxIndexer::language`]) and tags every summary with it, and the view
//! service reaches them through [`SyntaxIndexers`], which routes a file to the indexer of the
//! language the `languages` registry gives its name. One indexer is the set of its one language;
//! composition wires the set (`jfl-composition`'s `languages` module).
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
//!
//! **Scope facts at query time** ([`SyntaxIndexer::site_syntax`], IDX-10's syntax-tree resolver):
//! the same grammar's tree around one reference site, as plain data ([`SiteSyntax`]): what the
//! site's name is ([`SiteForm`]), every node around it up to the compilation unit, classified
//! ([`Around`]), and whether a local, a parameter, a pattern or a type parameter of that name is
//! declared where it may be in scope. The queries read it to prove a binding (`queries::admit`);
//! an indexer that gives none (the testkit's canned one) leaves every resolved name `possible`.

use core::future::Future;
use std::sync::Arc;

use jfl_domain::{ByteRange, FileSummary, Language, SummaryError};

use crate::queries::FileText;

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
    /// No indexer of this language is wired (IDX-17): a gap, as any refusal.
    #[error("no indexer is wired for {}", language.name())]
    NoIndexer {
        /// The file's language.
        language: Language,
    },
}

/// What a reference site's name is, read from the syntax tree at the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SiteForm {
    /// A bare name in an expression (`total`, the `total` of `total.add(x)`): a variable, or a
    /// type or package when no variable of that name is in scope (JLS 6.5.2).
    Name,
    /// The name of an unqualified call: `charge(…)`.
    Call,
    /// `this.name`.
    ThisName,
    /// `this.name(…)`.
    ThisCall,
    /// A simple type name where a type is expected: `Money total`, `List<Money>`, `(Money) x`,
    /// `extends Money`, `throws Failure`.
    Type,
    /// The simple name of an annotation: `Tag` of `@Tag` or `@Tag(…)`.
    Annotation,
    /// The simple type name of an instance creation without a class body: `Money` of
    /// `new Money(…)`.
    New,
    /// Anything else (a qualified name, `super.`, a method reference, an element key, a `case`
    /// label, ...): never proved.
    Other,
}

/// One node around a site, innermost first, as the resolver reads scopes from it. An indexer
/// classifies every node kind of its grammar into exactly one of these (pinned by
/// `jfl-adapter-java`'s `scopes::tests`), and anything it does not place in a position it
/// knows is [`Around::Denied`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Around {
    /// Expressions, statements, blocks and type syntax between the site and the next frame below:
    /// nodes that declare no member and open no class body (consecutive ones are one entry).
    Code,
    /// The body of a method or constructor, by the callable's name.
    CallableBody(ByteRange),
    /// A method's or constructor's header (modifiers, type parameters, result, parameters,
    /// `throws`), by its name.
    CallableHeader(ByteRange),
    /// A field declaration's modifiers or type (never its initializer).
    FieldHeader,
    /// The body of a top-level or member type, by the type's name.
    TypeBody(ByteRange),
    /// A top-level or member type's header (modifiers, type parameters, `extends`, `implements`,
    /// `permits`), by its name.
    TypeHeader(ByteRange),
    /// The compilation unit.
    Unit,
    /// A node kind, or a node in a position, the resolver does not handle (an anonymous, local
    /// or enum constant body, a lambda, a `switch`, an annotation's arguments, an initializer,
    /// a syntax error, ...), by its kind: the site is never proved.
    Denied(&'static str),
}

/// The syntax tree's facts about one reference site ([`SyntaxIndexer::site_syntax`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SiteSyntax {
    /// What the site's name is.
    pub form: SiteForm,
    /// The nodes around it, innermost first, ending with [`Around::Unit`] or at the first
    /// [`Around::Denied`].
    pub around: Vec<Around>,
    /// Whether a local variable, parameter, pattern, lambda, catch, `for` or resource variable,
    /// local type or type parameter of the site's name is declared in a callable or type around
    /// it, wherever in that callable or type: such a name may be the one meant.
    pub shadowed: bool,
}

/// Indexes one language's source.
pub trait SyntaxIndexer: Send + Sync + 'static {
    /// The language it reads: every summary it makes is of this language (the `syntax_indexer`
    /// contract suite's `summarises_the_golden_corpus`, whose goldens each name their language).
    fn language(&self) -> Language;

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

    /// The syntax tree's facts about the reference site whose name is at `name` of `text` (a
    /// file's text after a dropped byte-order mark, as the queries read it), or `None` when the
    /// indexer gives none: the text does not parse cleanly, holds Unicode escapes, spends its
    /// work budget, or `name` is not one name's bytes. `None` proves nothing, so the provided
    /// implementation (a fake's) is never looser than a real one.
    fn site_syntax(
        &self,
        _text: Arc<FileText>,
        _name: ByteRange,
    ) -> impl Future<Output = Option<SiteSyntax>> + Send {
        core::future::ready(None)
    }

    /// [`SyntaxIndexer::site_syntax`] for each of `names` of one file, in their order, over one
    /// parse (references prove many sites of a file, IDX-11). The provided implementation gives
    /// no facts, never looser than a real one.
    fn sites_syntax(
        &self,
        _text: Arc<FileText>,
        names: Vec<ByteRange>,
    ) -> impl Future<Output = Vec<Option<SiteSyntax>>> + Send {
        core::future::ready(vec![None; names.len()])
    }
}

/// What a [`SyntaxKey`](jfl_domain::SyntaxKey) reads of a language's indexer: its grammar and its
/// summariser's format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyInputs {
    /// [`SyntaxIndexer::grammar`].
    pub grammar: GrammarId,
    /// [`SyntaxIndexer::summariser_version`].
    pub summariser_version: u32,
}

/// The indexers of every wired language, as the view service and the queries reach them: each
/// call names the file's language (from the `languages` registry), and goes to that language's
/// [`SyntaxIndexer`]. A language with no indexer is refused, never read as another language.
///
/// Every single [`SyntaxIndexer`] is the set of its one language. A second language is wired in
/// composition by a type holding one indexer per language whose methods `match` on the language
/// (`jfl-composition`'s `languages` module); the port stays generic, so the request path makes
/// no dynamic call. Rejected alternative: `dyn SyntaxIndexer` per language, which the port's
/// `impl Future` methods rule out without boxing every parse.
///
/// Pinned by: `syntax_indexer::tests::should_route_a_file_to_its_language_when_one_indexer_is_the_set`.
pub trait SyntaxIndexers: Send + Sync + 'static {
    /// The key inputs of `language`'s indexer, `None` when none is wired.
    fn key_inputs(&self, language: Language) -> Option<KeyInputs>;

    /// [`SyntaxIndexer::index`] by `language`'s indexer; [`SyntaxIndexError::NoIndexer`] when none
    /// is wired.
    fn index(
        &self,
        language: Language,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send;

    /// [`SyntaxIndexer::site_syntax`] by the indexer of `text`'s language; `None` when none is
    /// wired (which proves nothing).
    fn site_syntax(
        &self,
        text: Arc<FileText>,
        name: ByteRange,
    ) -> impl Future<Output = Option<SiteSyntax>> + Send;

    /// [`SyntaxIndexer::sites_syntax`] by the indexer of `text`'s language; no facts when none is
    /// wired.
    fn sites_syntax(
        &self,
        text: Arc<FileText>,
        names: Vec<ByteRange>,
    ) -> impl Future<Output = Vec<Option<SiteSyntax>>> + Send;
}

impl<X: SyntaxIndexer> SyntaxIndexers for X {
    fn key_inputs(&self, language: Language) -> Option<KeyInputs> {
        (language == self.language()).then(|| KeyInputs {
            grammar: self.grammar(),
            summariser_version: self.summariser_version(),
        })
    }

    async fn index(&self, language: Language, bytes: Vec<u8>) -> Result<Indexed, SyntaxIndexError> {
        if language != self.language() {
            return Err(SyntaxIndexError::NoIndexer { language });
        }
        SyntaxIndexer::index(self, bytes).await
    }

    async fn site_syntax(&self, text: Arc<FileText>, name: ByteRange) -> Option<SiteSyntax> {
        if text.language != self.language() {
            return None;
        }
        SyntaxIndexer::site_syntax(self, text, name).await
    }

    async fn sites_syntax(
        &self,
        text: Arc<FileText>,
        names: Vec<ByteRange>,
    ) -> Vec<Option<SiteSyntax>> {
        if text.language != self.language() {
            return vec![None; names.len()];
        }
        SyntaxIndexer::sites_syntax(self, text, names).await
    }
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

    struct Tagging;

    impl SyntaxIndexer for Tagging {
        fn language(&self) -> Language {
            Language::Java
        }

        fn grammar(&self) -> GrammarId {
            GrammarId {
                name: "tagging",
                version: "1",
            }
        }

        fn summariser_version(&self) -> u32 {
            7
        }

        fn index(
            &self,
            _bytes: Vec<u8>,
        ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
            core::future::ready(Ok(Indexed {
                summary: jfl_domain::SummaryBuilder::new(Language::Java).finish(),
                diagnostics: Vec::new(),
                diagnostics_dropped: 0,
            }))
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        loop {
            if let core::task::Poll::Ready(out) = future.as_mut().poll(&mut context) {
                return out;
            }
        }
    }

    #[test]
    fn should_route_a_file_to_its_language_when_one_indexer_is_the_set() {
        // given
        let set = Tagging;
        let text = Arc::new(FileText::new(Language::Java, "class A {}".to_owned()).unwrap());
        // when
        let inputs = SyntaxIndexers::key_inputs(&set, Language::Java);
        let indexed = block_on(SyntaxIndexers::index(
            &set,
            Language::Java,
            b"class A {}".to_vec(),
        ));
        let facts = block_on(SyntaxIndexers::sites_syntax(
            &set,
            text,
            vec![
                ByteRange::new(
                    jfl_domain::ByteOffset::new(6),
                    jfl_domain::ByteOffset::new(7),
                )
                .unwrap(),
            ],
        ));
        // then
        assert_eq!(
            inputs,
            Some(KeyInputs {
                grammar: set.grammar(),
                summariser_version: 7,
            })
        );
        assert_eq!(indexed.map(|i| i.summary.language()), Ok(Language::Java));
        assert_eq!(facts, vec![None]);
        assert!(Language::ALL.iter().all(|language| {
            SyntaxIndexers::key_inputs(&set, *language).is_some() == (*language == Language::Java)
        }));
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
