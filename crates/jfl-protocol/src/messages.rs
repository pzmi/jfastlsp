//! The message catalogue of protocol v1: one marker type per method, with its params and result.
//!
//! | Method | Kind | Params | Result |
//! |:--|:--|:--|:--|
//! | `jfl/handshake` | request | [`HandshakeParams`] | [`HandshakeResult`] |
//! | `jfl/definition` | request | [`PositionParams`] | [`WireEnvelope`] |
//! | `jfl/references` | request | [`ReferencesParams`] | [`WireEnvelope`] |
//! | `jfl/hover` | request | [`PositionParams`] | [`HoverResult`] |
//! | `jfl/symbols` | request | [`SymbolsParams`] | [`SymbolsResult`] |
//! | `jfl/implementations` | request | [`PositionParams`] | [`WireEnvelope`] |
//! | `jfl/calls` | request | [`CallsParams`] | [`WireEnvelope`] |
//! | `jfl/diagnostics` | request | [`DiagnosticsParams`] | [`DiagnosticsResult`] |
//! | `jfl/status` | request | [`StatusParams`] | [`StatusResult`] |
//! | `jfl/sync` | request | [`SyncParams`] | [`SyncResult`] |
//! | `jfl/stop` | request | [`StopParams`] | [`StopResult`] |
//! | `jfl/didOpen` | notification | [`DidOpenParams`] | — |
//! | `jfl/didChange` | notification | [`DidChangeParams`] | — |
//! | `jfl/didClose` | notification | [`DidCloseParams`] | — |
//! | `$/cancelRequest` | notification | [`CancelParams`] | — |
//!
//! The handshake opens every connection (plan §5.3, §6.6): the proxy sends its
//! [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) and kind, and a daemon of another major answers
//! [`ErrorCode::VersionMismatch`] ([`version_mismatch`]), upon which the proxy asks it to
//! `jfl/stop` and starts its own (ADR 0021). Every result a carrier prints carries
//! `schema_version` and, where it holds positions, `position_encoding` (plan §2.3, §2.4).
//!
//! Additive only (plan §6.6): no struct refuses an unknown member, an enum a newer minor may
//! extend has an `other` variant (the params enums [`CallDirection`] and [`SymbolFilter`] are
//! closed: only the daemon reads them, and an unknown value is `InvalidParams`), and a field
//! added later must be optional. **A minor never adds a member that `complete` covers**: hits,
//! or anything else that changes what `complete=true` asserts (in the envelope, `symbols`,
//! `diagnostics` or a view's status). An older reader drops an unknown member silently and would
//! go on forwarding `complete=true` over a union it no longer holds, so such a change is a new
//! major ([`crate::version`]; `tests/compat.rs::should_keep_the_members_complete_covers_when_a_minor_is_made`
//! pins the covered members of today).
//!
//! **Frozen across majors** (so a proxy of major N+1 can always reach a daemon of major N, and
//! the reverse, to refuse it and stop it, ADR 0021): the `jfl/handshake` method with its
//! `params.protocol` member (`{"major","minor"}`), the [`ErrorCode::VersionMismatch`] code
//! (-32010) with its `data` ([`VersionMismatchData`]), and `jfl/stop`, whose params may be `{}`,
//! `null` or omitted and whose result is `{}`. No major changes these.
//!
//! `tests/golden.rs` pins every message, `tests/compat.rs` the additive and major rules.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use core::fmt;

use jfl_domain::{DeclKind, PositionEncoding};

use serde::de::{self, DeserializeOwned, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::envelope::{
    ClaimedCompleteness, WireEnvelope, WireLocation, WireRange, WireTarget, WireView, encoding,
};
use crate::rpc::{ErrorCode, ErrorObject, RequestId};
use crate::uri::DocumentUri;
use crate::version::{PROTOCOL_VERSION, SCHEMA_VERSION, Version};

/// A payload: params or a result. A versioned one names the member that holds its version and
/// the version this build writes there; a reader refuses another major
/// ([`crate::codec`]).
pub trait Payload: Serialize + DeserializeOwned {
    /// The version member's name and this build's version, for a versioned payload.
    const VERSIONED: Option<(&'static str, Version)> = None;

    /// The version the payload carries, for a versioned payload.
    fn version(&self) -> Option<Version> {
        None
    }
}

/// A request method.
pub trait Method {
    /// The method name.
    const NAME: &'static str;
    /// Its params.
    type Params: Payload;
    /// Its result.
    type Result: Payload;
}

/// A notification method.
pub trait Notify {
    /// The method name.
    const NAME: &'static str;
    /// Its params.
    type Params: Payload;
}

macro_rules! plain_payload {
    ($($ty:ty),* $(,)?) => {
        $(impl Payload for $ty {})*
    };
}

macro_rules! method {
    ($(#[$doc:meta])* $marker:ident, $name:literal, $params:ty, $result:ty) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $marker {}

        impl Method for $marker {
            const NAME: &'static str = $name;
            type Params = $params;
            type Result = $result;
        }
    };
}

macro_rules! notification {
    ($(#[$doc:meta])* $marker:ident, $name:literal, $params:ty) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $marker {}

        impl Notify for $marker {
            const NAME: &'static str = $name;
            type Params = $params;
        }
    };
}

method!(
    /// `jfl/handshake`: the first request of every connection.
    Handshake, "jfl/handshake", HandshakeParams, HandshakeResult
);
method!(
    /// `jfl/definition` (`jfl def`).
    Definition, "jfl/definition", PositionParams, WireEnvelope
);
method!(
    /// `jfl/references` (`jfl refs`).
    References, "jfl/references", ReferencesParams, WireEnvelope
);
method!(
    /// `jfl/hover` (`jfl hover`).
    Hover, "jfl/hover", PositionParams, HoverResult
);
method!(
    /// `jfl/symbols` (`jfl sym`).
    Symbols, "jfl/symbols", SymbolsParams, SymbolsResult
);
method!(
    /// `jfl/implementations` (`jfl impls`).
    Implementations, "jfl/implementations", PositionParams, WireEnvelope
);
method!(
    /// `jfl/calls` (`jfl calls`).
    Calls, "jfl/calls", CallsParams, WireEnvelope
);
method!(
    /// `jfl/diagnostics` (`jfl diag`).
    Diagnostics, "jfl/diagnostics", DiagnosticsParams, DiagnosticsResult
);
method!(
    /// `jfl/status` (`jfl status`).
    Status, "jfl/status", StatusParams, StatusResult
);
method!(
    /// `jfl/sync` (`jfl sync`): returns once every pending change is published (plan §4.3).
    Sync, "jfl/sync", SyncParams, SyncResult
);
method!(
    /// `jfl/stop`: the daemon exits after answering (a proxy of another major asks it to).
    Stop, "jfl/stop", StopParams, StopResult
);
notification!(
    /// `jfl/didOpen`: an overlay for an open document (plan §4.4).
    DidOpen, "jfl/didOpen", DidOpenParams
);
notification!(
    /// `jfl/didChange`: the overlay's new version.
    DidChange, "jfl/didChange", DidChangeParams
);
notification!(
    /// `jfl/didClose`: the overlay is dropped.
    DidClose, "jfl/didClose", DidCloseParams
);
notification!(
    /// `$/cancelRequest`: answer `id` with [`ErrorCode::RequestCancelled`] unless already done.
    Cancel, "$/cancelRequest", CancelParams
);

/// Every request method name, in catalogue order.
pub const REQUEST_METHODS: [&str; 11] = [
    Handshake::NAME,
    Definition::NAME,
    References::NAME,
    Hover::NAME,
    Symbols::NAME,
    Implementations::NAME,
    Calls::NAME,
    Diagnostics::NAME,
    Status::NAME,
    Sync::NAME,
    Stop::NAME,
];

/// Every notification method name, in catalogue order.
pub const NOTIFICATION_METHODS: [&str; 4] =
    [DidOpen::NAME, DidChange::NAME, DidClose::NAME, Cancel::NAME];

/// Who opened the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// The `jfl` CLI carrier.
    Cli,
    /// The `jfl lsp` proxy.
    Lsp,
    /// A kind this build does not know (a newer minor's).
    #[serde(other)]
    Other,
}

/// The proxy's side of the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Its kind.
    pub kind: ClientKind,
    /// Its release (`CARGO_PKG_VERSION`).
    pub version: String,
}

/// `jfl/handshake` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeParams {
    /// The proxy's [`PROTOCOL_VERSION`].
    pub protocol: Version,
    /// The proxy.
    pub client: ClientInfo,
}

/// The daemon's side of the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Its release.
    pub version: String,
    /// Its process id.
    pub pid: u32,
}

/// `jfl/handshake` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeResult {
    /// The daemon's [`PROTOCOL_VERSION`].
    pub protocol: Version,
    /// The daemon.
    pub server: ServerInfo,
}

/// The `data` of a [`ErrorCode::VersionMismatch`] error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionMismatchData {
    /// The daemon's protocol version.
    pub daemon: Version,
    /// The version the proxy sent.
    pub client: Version,
}

/// The error a daemon of protocol `daemon` answers a handshake of another major with.
///
/// Pinned by: `tests/golden.rs` (`error.version-mismatch.json`).
#[must_use]
pub fn version_mismatch(daemon: Version, client: Version) -> ErrorObject {
    ErrorObject {
        code: ErrorCode::VersionMismatch,
        message: format!(
            "protocol {client} is not compatible with the daemon's {daemon}: restart the daemon"
        ),
        data: serde_json::to_value(VersionMismatchData { daemon, client }).ok(),
    }
}

/// A position in a document: `jfl def`, `hover` and `impls`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionParams {
    /// The document, as the client wrote it.
    pub uri: DocumentUri,
    /// The 0-based line.
    pub line: u32,
    /// The 0-based column, in `position_encoding`.
    pub col: u32,
    /// The column's encoding, which the answer's ranges use too.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
}

/// `jfl/references` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferencesParams {
    /// Where.
    #[serde(flatten)]
    pub at: PositionParams,
    /// Leave `possible` empty (`jfl refs --exact-only`); `false` when absent.
    #[serde(default)]
    pub exact_only: bool,
}

/// Which calls `jfl/calls` lists. Closed: a value this build does not know is `InvalidParams`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallDirection {
    /// The callers of the target.
    Incoming,
    /// What the target calls.
    Outgoing,
}

/// `jfl/calls` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallsParams {
    /// Where.
    #[serde(flatten)]
    pub at: PositionParams,
    /// Which calls.
    pub direction: CallDirection,
}

/// `12 exact, 3 possible, complete` for a declaration under the cursor (plan §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceSummary {
    /// References bound with evidence.
    pub exact: u32,
    /// Other candidates.
    pub possible: u32,
    /// The daemon's claim that `exact ∪ possible` holds every reference: a claim, like
    /// [`ClaimedCompleteness`], never a proof, and covered by the minor rule (module
    /// documentation).
    pub complete: bool,
}

/// `jfl/hover` result (plan §2.2): the signature, Javadoc, module and the status line's parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoverResult {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The encoding of `range`.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// The view and generation.
    pub view: WireView,
    /// The hovered name's range; `null` when nothing is there.
    pub range: Option<WireRange>,
    /// The resolved target; `null` when unresolved.
    pub target: Option<WireTarget>,
    /// The declaration's signature.
    pub signature: Option<String>,
    /// Its Javadoc.
    pub documentation: Option<String>,
    /// Its module (`:payments`).
    pub module: Option<String>,
    /// The reference summary, for a declaration.
    pub references: Option<ReferenceSummary>,
    /// Why the target is unresolved, when it is: the `unresolved: <reason>` status line.
    pub unresolved: Option<String>,
}

/// The `--kind` filter of `jfl sym`. Closed: a value this build does not know is `InvalidParams`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolFilter {
    /// Types: classes, interfaces, enums, records and annotation types.
    Class,
    /// Methods and constructors.
    Method,
    /// Fields, enum constants and record components.
    Field,
}

/// `jfl/symbols` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolsParams {
    /// The worktree searched.
    pub worktree: DocumentUri,
    /// The prefix or name queried.
    pub query: String,
    /// Only this kind; `null` for every kind.
    pub kind: Option<SymbolFilter>,
    /// At most this many symbols; `null` for the daemon's cap.
    pub limit: Option<u32>,
    /// The encoding of the answer's ranges.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
}

/// A declaration's kind on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// `class`.
    Class,
    /// `interface`.
    Interface,
    /// `enum`.
    Enum,
    /// `record`.
    Record,
    /// `@interface`.
    Annotation,
    /// A method.
    Method,
    /// A constructor.
    Constructor,
    /// A field.
    Field,
    /// An enum constant.
    EnumConstant,
    /// A record component.
    RecordComponent,
    /// An annotation type's element.
    AnnotationElement,
    /// A kind this build does not know (a newer minor's).
    #[serde(other)]
    Other,
}

impl From<DeclKind> for SymbolKind {
    fn from(kind: DeclKind) -> Self {
        match kind {
            DeclKind::Class => Self::Class,
            DeclKind::Interface => Self::Interface,
            DeclKind::Enum => Self::Enum,
            DeclKind::Record => Self::Record,
            DeclKind::Annotation => Self::Annotation,
            DeclKind::Method => Self::Method,
            DeclKind::Constructor => Self::Constructor,
            DeclKind::Field => Self::Field,
            DeclKind::EnumConstant => Self::EnumConstant,
            DeclKind::RecordComponent => Self::RecordComponent,
            DeclKind::AnnotationElement => Self::AnnotationElement,
        }
    }
}

/// One declaration found by `jfl/symbols`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSymbol {
    /// The `SymbolId` text.
    pub symbol: String,
    /// Its kind.
    pub kind: SymbolKind,
    /// Where it is declared.
    pub location: WireLocation,
}

/// `jfl/symbols` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolsResult {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The encoding of every range.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// The view and generation.
    pub view: WireView,
    /// The declarations found.
    pub symbols: Vec<WireSymbol>,
    /// Whether every declaration matching the query is listed.
    #[serde(flatten)]
    pub completeness: ClaimedCompleteness,
}

/// `jfl/diagnostics` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsParams {
    /// The document.
    pub uri: DocumentUri,
    /// The encoding of the answer's ranges.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// How long to wait for the javac worker, in milliseconds; `null` for the daemon's default.
    pub deadline_ms: Option<u32>,
}

/// A diagnostic's severity, LSP's four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// Information.
    Information,
    /// A hint.
    Hint,
    /// A severity this build does not know.
    #[serde(other)]
    Other,
}

/// Who produced a diagnostic (plan §2.2, §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSource {
    /// The syntax index.
    Syntax,
    /// A javac worker.
    Javac,
    /// A source this build does not know.
    #[serde(other)]
    Other,
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDiagnostic {
    /// Where.
    pub range: WireRange,
    /// How bad.
    pub severity: Severity,
    /// Who said so.
    pub source: DiagnosticSource,
    /// The producer's code (javac's `compiler.err.…`), if any.
    pub code: Option<String>,
    /// What.
    pub message: String,
}

/// `jfl/diagnostics` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsResult {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The encoding of every range.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// The document.
    pub uri: DocumentUri,
    /// The document version the diagnostics are for; `null` for the disk content (plan §3.2).
    pub version: Option<i32>,
    /// The diagnostics.
    pub diagnostics: Vec<WireDiagnostic>,
    /// Whether every source ran (semantic diagnostics need a classpath, plan §2.2).
    #[serde(flatten)]
    pub completeness: ClaimedCompleteness,
}

/// `jfl/status` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusParams {
    /// One worktree; `null` for every open view.
    pub worktree: Option<DocumentUri>,
}

/// The daemon in `jfl/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    /// Its release.
    pub version: String,
    /// Its process id.
    pub pid: u32,
    /// Its protocol.
    pub protocol: Version,
}

/// One view in `jfl/status`: coverage and why it is incomplete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewStatus {
    /// The view.
    pub view: WireView,
    /// Source files in the worktree.
    pub files: u32,
    /// Files with a summary at the generation.
    pub indexed: u32,
    /// Files changed on disk after their summaries.
    pub stale: u32,
    /// Changes seen and not yet published.
    pub pending_events: u32,
    /// Whether the view is wholly indexed and fresh.
    #[serde(flatten)]
    pub completeness: ClaimedCompleteness,
}

/// `jfl/status` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResult {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The daemon.
    pub daemon: DaemonStatus,
    /// The views asked for.
    pub views: Vec<ViewStatus>,
}

/// `jfl/sync` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncParams {
    /// The file or worktree to sync; `null` for every open view.
    pub uri: Option<DocumentUri>,
}

/// `jfl/sync` result: the generations published once the barrier passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResult {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The synced views.
    pub views: Vec<WireView>,
}

/// `jfl/stop` params: none. Written `{}`; read from `{}` (members ignored), `null` or an omitted
/// `params`, so a proxy of any major can stop a daemon of any other (the frozen cross-major
/// contract, module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct StopParams {}

impl<'de> Deserialize<'de> for StopParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Nothing;

        impl<'de> Visitor<'de> for Nothing {
            type Value = StopParams;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object or null")
            }

            fn visit_unit<E: de::Error>(self) -> Result<StopParams, E> {
                Ok(StopParams {})
            }

            fn visit_none<E: de::Error>(self) -> Result<StopParams, E> {
                Ok(StopParams {})
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<StopParams, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(StopParams {})
            }
        }

        deserializer.deserialize_any(Nothing)
    }
}

/// `jfl/stop` result: none (`{}`), sent before the daemon exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StopResult {}

/// `jfl/didOpen` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidOpenParams {
    /// The document.
    pub uri: DocumentUri,
    /// Its version; later changes increase it.
    pub version: i32,
    /// Its full text.
    pub text: String,
}

/// One edit: a range replaced by `text`, or the whole text when `range` is `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentChange {
    /// The replaced range, in the params' `position_encoding`.
    pub range: Option<WireRange>,
    /// The new text.
    pub text: String,
}

/// `jfl/didChange` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidChangeParams {
    /// The document.
    pub uri: DocumentUri,
    /// Its version after the changes.
    pub version: i32,
    /// The encoding of the changes' ranges.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// The edits, applied in order.
    pub changes: Vec<ContentChange>,
}

/// `jfl/didClose` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DidCloseParams {
    /// The document.
    pub uri: DocumentUri,
}

/// `$/cancelRequest` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelParams {
    /// The request to cancel.
    pub id: RequestId,
}

plain_payload!(
    PositionParams,
    ReferencesParams,
    CallsParams,
    SymbolsParams,
    DiagnosticsParams,
    StatusParams,
    SyncParams,
    StopParams,
    StopResult,
    DidOpenParams,
    DidChangeParams,
    DidCloseParams,
    CancelParams,
);

impl Payload for HandshakeParams {
    const VERSIONED: Option<(&'static str, Version)> = Some(("protocol", PROTOCOL_VERSION));

    fn version(&self) -> Option<Version> {
        Some(self.protocol)
    }
}

impl Payload for HandshakeResult {
    const VERSIONED: Option<(&'static str, Version)> = Some(("protocol", PROTOCOL_VERSION));

    fn version(&self) -> Option<Version> {
        Some(self.protocol)
    }
}

macro_rules! schema_payload {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Payload for $ty {
                const VERSIONED: Option<(&'static str, Version)> =
                    Some(("schema_version", SCHEMA_VERSION));

                fn version(&self) -> Option<Version> {
                    Some(self.schema_version)
                }
            }
        )*
    };
}

schema_payload!(
    WireEnvelope,
    HoverResult,
    SymbolsResult,
    DiagnosticsResult,
    StatusResult,
    SyncResult,
);
