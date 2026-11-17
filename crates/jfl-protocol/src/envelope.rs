//! The envelope's JSON form (plan §2.1, §2.4): what `jfl/definition`, `jfl/references`,
//! `jfl/implementations` and `jfl/calls` answer and what `jfl … --json` prints.
//!
//! ```json
//! {"schema_version":{"major":1,"minor":0},"position_encoding":"utf-16",
//!  "query":{"op":"refs","uri":"file:///repo/src/Billing.java","line":41,"col":17},
//!  "view":{"worktree":"file:///repo","head":"3f2c…","generation":118},
//!  "target":{"symbol":"com.acme.Billing#charge(Money)","status":"exact"},
//!  "exact":[{"uri":"…","range":[41,17,41,23]}],
//!  "possible":[{"uri":"…","range":[…],"reason":"receiver_type_unknown","why":"receiver type unknown (var in lambda)"}],
//!  "complete":false,"incomplete_because":["module :payments not indexed"],
//!  "freshness":{"synced_at":1763400000000,"pending_events":0}}
//! ```
//!
//! **Wire and domain are separate types, on purpose.** A [`WireEnvelope`] is made from a domain
//! [`Envelope`] ([`WireEnvelope::from_domain`]) or decoded from bytes, and there is no way back:
//! no conversion builds a `jfl_domain::Envelope` or a `jfl_domain::Completeness` from the wire.
//! In the domain, `complete=true` exists only as the output of `jfl_domain::completeness`, which
//! checked every visible module at the pinned generation (ADR 0014, IDX-02); a decoded `complete`
//! is only what a peer *claimed*, so it is held as a [`ClaimedCompleteness`], which a proxy can
//! forward or print and never passes off as a proof. Rejected: a `From<WireEnvelope>` for the
//! domain type (it would make a byte stream a second source of `complete=true`), and decoding
//! straight into the domain types (the same, plus the domain would depend on serde).
//! `tests/claims.rs::should_never_name_the_completeness_proof_when_the_wire_form_is_built` pins
//! that this crate names no domain constructor of completeness.
//!
//! **`complete` covers `exact` and `possible`**: a later minor may add members, but never one
//! that carries hits or changes what `complete=true` asserts, since a reader of an older minor
//! drops unknown members and would forward the claim over a union it no longer holds; that is a
//! new major (`crate::messages`, plan §6.6;
//! `tests/compat.rs::should_keep_the_members_complete_covers_when_a_minor_is_made`).
//!
//! The view carries the worktree root as well as `HEAD` and the generation, so a reader can tell
//! which worktree a claim is about (IDX-09). Ranges are `[start line, start column, end line, end
//! column]`, 0-based, in `position_encoding`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use jfl_domain::{
    Completeness, Envelope, Location, ObjectId, PositionEncoding, PossibleHit, Range, Reason,
    Target, TargetStatus, View,
};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::uri::DocumentUri;
use crate::version::{SCHEMA_VERSION, Version};

/// A [`Range`] as `[start line, start column, end line, end column]`; a reversed one is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireRange(pub Range);

impl Serialize for WireRange {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self(range) = self;
        [
            range.start().line(),
            range.start().column(),
            range.end().line(),
            range.end().column(),
        ]
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WireRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [start_line, start_column, end_line, end_column] =
            <[u32; 4]>::deserialize(deserializer)?;
        Range::new(
            jfl_domain::Position::new(start_line, start_column),
            jfl_domain::Position::new(end_line, end_column),
        )
        .map(Self)
        .map_err(de::Error::custom)
    }
}

/// `position_encoding`: LSP's `utf-8`, `utf-16` or `utf-32` (plan §2.3).
pub mod encoding {
    use super::{Deserializer, PositionEncoding, Serializer, Visitor, de, fmt};

    /// Writes the encoding's LSP name.
    pub fn serialize<S: Serializer>(
        encoding: &PositionEncoding,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(encoding.lsp_name())
    }

    /// Reads an LSP name; any other is refused.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PositionEncoding, D::Error> {
        struct NameVisitor;

        impl Visitor<'_> for NameVisitor {
            type Value = PositionEncoding;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("utf-8, utf-16 or utf-32")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<PositionEncoding, E> {
                PositionEncoding::from_lsp_name(name)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Str(name), &self))
            }
        }

        deserializer.deserialize_str(NameVisitor)
    }
}

/// `head`: a Git object id in lower-case hexadecimal, or `null` on an unborn branch.
mod head {
    use super::{Deserializer, ObjectId, Serializer, Visitor, de, fmt};

    pub(super) fn serialize<S: Serializer>(
        head: &Option<ObjectId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match head {
            Some(id) => serializer.collect_str(id),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<ObjectId>, D::Error> {
        struct HeadVisitor;

        impl<'de> Visitor<'de> for HeadVisitor {
            type Value = Option<ObjectId>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("40 or 64 hexadecimal digits, or null")
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }

            fn visit_some<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
                inner.deserialize_str(self)
            }

            fn visit_str<E: de::Error>(self, hex: &str) -> Result<Self::Value, E> {
                ObjectId::parse(hex).map(Some).map_err(E::custom)
            }
        }

        deserializer.deserialize_option(HeadVisitor)
    }
}

/// `view`: the worktree, its `HEAD` and the generation the answer was computed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireView {
    /// The worktree root: which worktree the answer and its completeness are about.
    pub worktree: DocumentUri,
    /// Its `HEAD`; `null` on an unborn branch.
    #[serde(with = "head")]
    pub head: Option<ObjectId>,
    /// The generation the request pinned.
    pub generation: u64,
}

impl From<&View> for WireView {
    fn from(view: &View) -> Self {
        Self {
            worktree: DocumentUri::from(&view.worktree),
            head: view.head,
            generation: view.generation.0,
        }
    }
}

/// A place in a file: `{"uri":…,"range":[…]}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WireLocation {
    /// The file.
    pub uri: DocumentUri,
    /// The range, in the answer's `position_encoding`.
    pub range: WireRange,
}

impl From<&Location> for WireLocation {
    fn from(location: &Location) -> Self {
        Self {
            uri: DocumentUri::from(&location.uri),
            range: WireRange(location.range),
        }
    }
}

/// A `possible` hit: its place, a stable `reason` code and the `why` in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePossible {
    /// The file.
    pub uri: DocumentUri,
    /// The range.
    pub range: WireRange,
    /// The reason's code ([`reason_code`]); a code this build does not know is kept as sent.
    pub reason: String,
    /// The reason and its detail in words: `receiver type unknown (var in lambda)`.
    pub why: String,
}

impl From<&PossibleHit> for WirePossible {
    fn from(hit: &PossibleHit) -> Self {
        Self {
            uri: DocumentUri::from(&hit.location.uri),
            range: WireRange(hit.location.range),
            reason: reason_code(hit.why.reason()).into(),
            why: hit.why.to_string(),
        }
    }
}

/// The stable code of a [`Reason`]: `receiver_type_unknown`, `overload_not_proved`, ….
///
/// Pinned by: `envelope::tests::should_give_every_reason_a_distinct_code_when_codes_are_listed`.
#[must_use]
pub const fn reason_code(reason: Reason) -> &'static str {
    match reason {
        Reason::ReceiverTypeUnknown => "receiver_type_unknown",
        Reason::OverloadNotProved => "overload_not_proved",
        Reason::NameAndArity => "name_and_arity",
        Reason::MethodReference => "method_reference",
        Reason::ImportUnresolved => "import_unresolved",
        Reason::GeneratedMember => "generated_member",
        Reason::NoClasspath => "no_classpath",
        Reason::SyntaxError => "syntax_error",
    }
}

/// How well a target is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTargetStatus {
    /// Bound with evidence.
    Exact,
    /// The owner type is proved, not the overload.
    Ambiguous,
    /// A status this build does not know (a newer minor's).
    #[serde(other)]
    Other,
}

/// `target`: the symbol the query resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireTarget {
    /// The `SymbolId` text: `com.acme.Billing#charge(Money)`.
    pub symbol: String,
    /// How well it is known.
    pub status: WireTargetStatus,
    /// How many overloads, when ambiguous; left out otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overloads: Option<u32>,
}

impl From<&Target> for WireTarget {
    fn from(target: &Target) -> Self {
        let (status, overloads) = match target.status {
            TargetStatus::Exact => (WireTargetStatus::Exact, None),
            TargetStatus::Ambiguous { overloads } => (WireTargetStatus::Ambiguous, Some(overloads)),
        };
        Self {
            symbol: target.symbol.to_string(),
            status,
            overloads,
        }
    }
}

/// `freshness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFreshness {
    /// When the pinned generation's last reconcile finished, in milliseconds since the Unix
    /// epoch; `null` before the first.
    pub synced_at: Option<u64>,
    /// Changes seen and not yet in a published generation.
    pub pending_events: u32,
}

/// `complete` and `incomplete_because` as a peer stated them: data, not a proof (module
/// documentation). `incomplete_because` is non-empty exactly when `complete` is false; a
/// message that breaks that is refused.
///
/// Pinned by: `envelope::tests::should_refuse_a_claim_when_complete_and_reasons_disagree`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawClaim")]
pub struct ClaimedCompleteness {
    complete: bool,
    incomplete_because: Vec<String>,
}

#[derive(Deserialize)]
struct RawClaim {
    complete: bool,
    #[serde(default)]
    incomplete_because: Vec<String>,
}

/// Why a claim is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaimError {
    /// `complete` is false and no reason is given.
    #[error("complete is false and incomplete_because is empty")]
    NoReason,
    /// `complete` is true and reasons are given.
    #[error("complete is true and incomplete_because is not empty")]
    CompleteWithReasons,
}

impl TryFrom<RawClaim> for ClaimedCompleteness {
    type Error = ClaimError;

    fn try_from(raw: RawClaim) -> Result<Self, ClaimError> {
        match (raw.complete, raw.incomplete_because.is_empty()) {
            (true, false) => Err(ClaimError::CompleteWithReasons),
            (false, true) => Err(ClaimError::NoReason),
            _ => Ok(Self {
                complete: raw.complete,
                incomplete_because: raw.incomplete_because,
            }),
        }
    }
}

impl ClaimedCompleteness {
    /// Whether the peer claimed every reference is in `exact ∪ possible`.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// The reasons stated, in words; empty exactly when complete.
    #[must_use]
    pub fn incomplete_because(&self) -> &[String] {
        &self.incomplete_because
    }
}

/// The claim a domain [`Completeness`] makes: the only source of a complete one besides decoding.
impl From<&Completeness> for ClaimedCompleteness {
    fn from(completeness: &Completeness) -> Self {
        Self {
            complete: completeness.is_complete(),
            incomplete_because: completeness
                .reasons()
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

/// The operation a query ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// `jfl def`.
    Def,
    /// `jfl refs`.
    Refs,
    /// `jfl hover`.
    Hover,
    /// `jfl impls`.
    Impls,
    /// `jfl calls`.
    Calls,
    /// An operation this build does not know (a newer minor's).
    #[serde(other)]
    Other,
}

/// `query`: what was asked, the URI as the client sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// The operation.
    pub op: Operation,
    /// The file, as the client wrote it.
    pub uri: DocumentUri,
    /// The 0-based line.
    pub line: u32,
    /// The 0-based column, in `position_encoding`.
    pub col: u32,
}

/// One navigation answer on the wire (module documentation).
///
/// Pinned by: `tests/golden.rs` (`jfl.definition.response.json` and the others) and
/// `tests/properties.rs::should_decode_what_it_encodes_when_envelopes_are_generated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEnvelope {
    /// [`SCHEMA_VERSION`] when made by this build.
    pub schema_version: Version,
    /// The encoding of every range.
    #[serde(with = "encoding")]
    pub position_encoding: PositionEncoding,
    /// What was asked; `null` when the carrier did not say.
    pub query: Option<Query>,
    /// The view and generation.
    pub view: WireView,
    /// The target; `null` when unresolved.
    pub target: Option<WireTarget>,
    /// Hits bound with evidence, by URI and range.
    pub exact: Vec<WireLocation>,
    /// The other candidates, by URI and range.
    pub possible: Vec<WirePossible>,
    /// `complete` and `incomplete_because`.
    #[serde(flatten)]
    pub completeness: ClaimedCompleteness,
    /// How current the view was.
    pub freshness: WireFreshness,
}

impl WireEnvelope {
    /// The wire form of `envelope`, in its order and encoding, with the `query` it answered.
    ///
    /// Pinned by: `tests/golden.rs::should_match_the_golden_when_every_message_is_encoded`.
    #[must_use]
    pub fn from_domain(envelope: &Envelope, query: Option<Query>) -> Self {
        let freshness = envelope.freshness();
        Self {
            schema_version: SCHEMA_VERSION,
            position_encoding: envelope.position_encoding(),
            query,
            view: WireView::from(envelope.view()),
            target: envelope.target().map(WireTarget::from),
            exact: envelope.exact().iter().map(WireLocation::from).collect(),
            possible: envelope.possible().iter().map(WirePossible::from).collect(),
            completeness: ClaimedCompleteness {
                complete: envelope.complete(),
                incomplete_because: envelope
                    .incomplete_because()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            },
            freshness: WireFreshness {
                synced_at: freshness.synced_at.map(|at| at.0),
                pending_events: freshness.pending_events,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_give_every_reason_a_distinct_code_when_codes_are_listed() {
        // given
        let reasons = [
            Reason::ReceiverTypeUnknown,
            Reason::OverloadNotProved,
            Reason::NameAndArity,
            Reason::MethodReference,
            Reason::ImportUnresolved,
            Reason::GeneratedMember,
            Reason::NoClasspath,
            Reason::SyntaxError,
        ];

        // when
        let mut codes: Vec<&str> = reasons.iter().map(|r| reason_code(*r)).collect();
        codes.sort_unstable();
        codes.dedup();

        // then
        assert_eq!(codes.len(), reasons.len());
    }

    #[test]
    fn should_refuse_a_claim_when_complete_and_reasons_disagree() {
        // given
        let cases = [
            (
                r#"{"complete":true,"incomplete_because":["deadline"]}"#,
                false,
            ),
            (r#"{"complete":false,"incomplete_because":[]}"#, false),
            (r#"{"complete":false}"#, false),
            (r#"{"complete":true,"incomplete_because":[]}"#, true),
            (r#"{"complete":true}"#, true),
            (
                r#"{"complete":false,"incomplete_because":["deadline"]}"#,
                true,
            ),
        ];

        for (text, accepted) in cases {
            // when
            let claim = serde_json::from_str::<ClaimedCompleteness>(text);

            // then
            assert_eq!(claim.is_ok(), accepted, "{text}");
        }
    }

    #[test]
    fn should_refuse_a_range_when_it_is_reversed_or_not_four_numbers() {
        // given
        let cases = [
            "[1,0,0,0]",
            "[0,0,0]",
            "[0,0,0,0,0]",
            "[-1,0,0,0]",
            "[0,0,0,4294967296]",
        ];

        for text in cases {
            // when
            let range = serde_json::from_str::<WireRange>(text);

            // then
            assert!(range.is_err(), "{text}");
        }
    }

    #[test]
    fn should_refuse_a_head_when_it_is_not_an_object_id() {
        // given
        let text = r#"{"worktree":"file:///repo","head":"3f2c","generation":1}"#;

        // when
        let view = serde_json::from_str::<WireView>(text);

        // then
        assert!(view.is_err());
    }
}
