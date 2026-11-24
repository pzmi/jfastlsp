//! Translation between LSP 3.17 and `jfl-protocol` for the `jfl lsp` proxy. Role: adapter.
//!
//! Built on rust-analyzer's `lsp-server` (its message type) and `gen-lsp-types` (the LSP types,
//! generated from the specification's metamodel), both pinned exactly (ADR 0020), and on
//! `jfl-protocol`'s bounded frame decoder (ADR 0021). It depends on no other workspace crate than
//! `jfl-protocol` and `jfl-domain` (plan §6.2: the LSP translation never touches the engine).
//!
//! - [`server`]: the session: a synchronous main loop that applies notifications in order, a
//!   bounded pool that runs requests against a snapshot, cancellation bookkeeping, and the rule
//!   that every request is answered once.
//! - [`transport`]: frames in through the bounded decoder, frames out (results on the typed path).
//! - [`translate`]: LSP params and results to and from `jfl-protocol`'s, URIs echoed as sent and
//!   columns clamped to the line end.
//! - [`document`]: an open document's text, `didChange`'s ranged edits applied through
//!   `jfl-domain`'s conversions.
//! - [`memory`]: a byte pipe for running a session in process (tests, benchmarks).
//!
//! `jfl lsp` (`jfl-composition`'s `daemon::lsp`, LSP-02) is the server these make; the black-box
//! transcripts of `fixtures/lsp/` run against it (`crates/jfl-cli/tests/lsp_transcripts.rs`).

pub mod document;
pub mod memory;
pub mod server;
pub mod translate;
pub mod transport;

pub use lsp_server::{Notification, Request, RequestId};
pub use server::{Cancel, Config, Dispatch, Ended, Handler, Job, Notifier, Reply, serve};
