#![no_std]
//! Paths and URIs, positions and encodings, symbols, file summaries, cache keys, the result
//! envelope and generation policies: pure data and functions with no I/O. Role: pure.
//!
//! - [`path`], [`uri`], [`text`] (IDX-01): worktree-relative paths, `file://` URIs, and byte
//!   offsets, positions, encodings and the [`LineIndex`] that converts between them (plan §2.3).
//! - [`symbol`] (IDX-02): declaration kinds and the [`SymbolId`] text form (plan §2.1).
//! - [`summary`] (IDX-02): the compact per-file [`FileSummary`] the syntax index keeps.
//! - [`digest`] and [`keys`] (IDX-02): caller-supplied digests and the cache keys of plan §5.4.
//! - [`envelope`] (IDX-02): the result [`Envelope`] and the [`completeness`] function (plan §2.1,
//!   ADR 0014); `complete=true` is never wrong.
//!
//! Every refusal is a typed value (fnd §4.3).

extern crate alloc;

pub mod digest;
pub mod envelope;
pub mod keys;
pub mod path;
pub mod summary;
pub mod symbol;
pub mod text;
pub mod uri;

pub use digest::{Digest, KeyHasher};
pub use envelope::{
    Completeness, Envelope, EnvelopeParts, Freshness, Generation, IncompleteReason, IndexState,
    IndexStateError, Location, ModuleIndex, ModuleName, ModuleNameError, ObjectId, ObjectIdError,
    PossibleHit, Reason, Target, TargetStatus, UnixMillis, View, Why, completeness,
};
pub use keys::{
    EnvironmentInputs, EnvironmentKey, JdkIdentity, SemanticKey, SyntaxInputs, SyntaxKey,
};
pub use path::{PathError, RelPath};
pub use summary::{
    Arity, DeclId, Declaration, DeclarationInput, FileSummary, Import, ListRef, Modifiers,
    RefShape, ReferenceSite, Span, StrId, SummaryBuilder, SummaryError, SummaryParts,
};
pub use symbol::{
    DeclKind, MemberId, SymbolError, SymbolId, TypeParam, erase_parameter_type,
    erase_parameter_type_with, parameter_type_base,
};
pub use text::{
    ByteOffset, ByteRange, CliPosition, CliPositionError, LineIndex, Position, PositionEncoding,
    PositionError, Range,
};
pub use uri::{FileUri, UriError};
