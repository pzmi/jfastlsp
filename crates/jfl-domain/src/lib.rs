#![no_std]
//! Paths and URIs, positions and encodings, symbols, file summaries, cache keys, the result
//! envelope and generation policies: pure data and functions with no I/O. Role: pure.
//!
//! So far (IDX-01): worktree-relative paths ([`path`]), `file://` URIs ([`uri`]) and byte
//! offsets, positions, encodings and the [`LineIndex`] that converts between them ([`text`],
//! plan §2.3). Every refusal is a typed value (fnd §4.3).

extern crate alloc;

pub mod path;
pub mod text;
pub mod uri;

pub use path::{PathError, RelPath};
pub use text::{
    ByteOffset, ByteRange, CliPosition, CliPositionError, LineIndex, Position, PositionEncoding,
    PositionError, Range,
};
pub use uri::{FileUri, UriError};
