#![no_std]
//! The daemon wire protocol and the `--json` schema: versioned messages, handshake and frame
//! limits. Role: pure.
//!
//! One wire protocol for every carrier (ADR 0021): JSON-RPC 2.0 with the LSP base protocol's
//! `Content-Length` framing, over the daemon's Unix socket and `jfl lsp`'s stdio alike.
//!
//! - [`frame`]: the sans-io frame decoder (the length checked before any allocation) and encoder.
//! - [`rpc`]: JSON-RPC requests, responses, notifications, ids and typed error codes.
//! - [`messages`]: the catalogue of protocol v1, every operation of plan §2.4 plus the handshake,
//!   document sync and cancellation.
//! - [`envelope`]: the envelope's JSON form, a wire type that is never turned back into the
//!   domain's (`complete=true` stays the domain's to prove, ADR 0014).
//! - [`uri`]: a document URI as the client sent it, beside the file it names (LSP-01).
//! - [`version`]: the protocol and schema versions; additive minors, a different major refused
//!   (plan §6.6).
//! - [`codec`]: encode and decode with that rule applied.
//!
//! Goldens for every message are in `tests/fixtures/v1/` (ver §4).

extern crate alloc;

pub mod codec;
pub mod envelope;
pub mod frame;
pub mod messages;
pub mod rpc;
pub mod uri;
pub mod version;

pub use codec::{DecodeError, EncodeError};
pub use envelope::{ClaimedCompleteness, WireEnvelope};
pub use frame::{DEFAULT_MAX_FRAME_LEN, FrameDecoder, FrameError, Oversized, Progress};
pub use messages::{Method, Notify, Payload};
pub use rpc::{ErrorCode, ErrorObject, Notification, Outcome, Request, RequestId, Response};
pub use uri::DocumentUri;
pub use version::{MajorMismatch, PROTOCOL_VERSION, SCHEMA_VERSION, Version};
