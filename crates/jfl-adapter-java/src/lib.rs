//! `SyntaxIndexer` over tree-sitter Java: file summaries and syntax diagnostics. Role: adapter.
//!
//! The only crate that names a grammar's nodes and fields (ADR 0022, amended 2026-11-17 and
//! 2026-11-18): it parses with tree-sitter-java-orchard, compiles our own `tags.scm` and
//! `locals.scm` (`queries/orchard/`; upstream tree-sitter-java's in `queries/upstream/` are the
//! switch-back set, run by the tests), applies them as a kind-to-captures table in one walk and
//! fills a `jfl_domain::FileSummary`. Every node kind, field and
//! capture it reads is resolved when a [`Grammar`] is made, and a missing one fails start-up
//! naming it ([`grammar`]).
//!
//! **Extraction rules** (plan §2.1, §5.4, §5.5; IDX-02's and IDX-07's decisions):
//!
//! - **Input.** A leading byte-order mark is dropped, the size, UTF-8 and nesting bounds of
//!   `jfl_application::syntax_indexer::source_text` apply, eligible `\uXXXX` escapes are
//!   translated before parsing (JLS §3.3) and every range maps back to the file as written
//!   (`source.rs`). The parse and the extraction after it spend one work budget proportional to
//!   the input, and a file that runs out is refused (`extract.rs`'s `Budget`): one declaration
//!   node's parts are read once for all its declarators, and locals are looked up by hash, so
//!   the extraction is linear (`extract::tests` count its steps on inputs built to be
//!   quadratic).
//! - **Declarations**: classes, interfaces, enums, records, annotation types, methods,
//!   constructors (a compact one takes its record's component types), fields and interface
//!   constants (one per declarator), enum constants, record components and annotation elements.
//!   A declaration in a block, a lambda or an anonymous class body (a local or anonymous class,
//!   and everything in it) has no symbol identity and is left out; its reference sites stay.
//! - **Signature**: the header as written from the declaration's start (annotations and their
//!   arguments included) to its body, its initializer (a field) or its arguments (an enum
//!   constant), comments left out, white space collapsed, no trailing `;`.
//! - **Modifiers** as written, plus `VARARGS` when the last parameter is `T...`; **annotations**
//!   by name as written; **supertypes** from `extends` and `implements`, **permits** apart, both
//!   erased; **parameter types** through `jfl_domain::erase_parameter_type` with the method's
//!   type parameters, then each enclosing type's, in scope (a receiver parameter is not one).
//! - **Javadoc**: the nearest `/** */` comment before the declaration with only white space and
//!   other comments between.
//! - **Names** are interned after the escapes are translated and the identifier-ignorable
//!   characters of JLS §3.8 are dropped (`source.rs`).
//! - **Reference sites**: every identifier and type identifier outside comments and string
//!   literals (the grammar never puts one inside), except declaration names, local and pattern
//!   variable names, labels, a type parameter's own name, the package name and import names (an
//!   import adds its last segment as a qualified name). Shapes: a call with or without receiver
//!   (with its arity, comments not counted), `new T(...)`, `this(...)` and `super(...)` (named
//!   by the enclosing class and its superclass), a method reference (matched by name whatever the
//!   arity; `T::new` by the type), a field access, an annotation, a type use and a bare name.
//! - **Locals** (plan §5.5): a bare name bound to a local variable or a parameter in an
//!   enclosing block, callable, lambda, loop, catch clause or resource is dropped. A type body
//!   between the use and the local (a field there may shadow it), a pattern variable (flow
//!   scoping) or no declaration before the use keeps the site: an unclear binding stays.
//! - **Syntax errors** are diagnostics with ranges (an `ERROR` node's extent, or the empty range
//!   where a `MISSING` token belongs), at most `MAX_DIAGNOSTICS`, and a string template is one.
//!   No range ends between the `\r` and `\n` of a line terminator.
//!
//! Pinned by: the goldens of `fixtures/java/summaries/` (`tests/goldens.rs`, both grammars), the
//! `syntax_indexer` contract suite (`tests/contract.rs`) and `tests/properties.rs`.

pub mod grammar;
pub mod indexer;

mod extract;
mod scopes;
mod source;

pub use extract::Work;
pub use grammar::{
    Grammar, GrammarSpec, ORCHARD_LOCALS, ORCHARD_TAGS, ORCHARD_VERSION, StartupError,
    UPSTREAM_LOCALS, UPSTREAM_TAGS,
};
pub use indexer::{JavaIndexer, SUMMARISER_VERSION};
