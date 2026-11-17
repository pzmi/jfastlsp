//! Protocol and schema versions (plan §6.6, fnd §6).
//!
//! A version is a major and a minor. Minors are additive: a newer minor only adds optional fields
//! and new messages, which an older reader ignores, so two peers of one major always understand
//! each other. **A minor never adds a member that `complete` covers** (hits, or anything else
//! that changes what `complete=true` asserts): an older reader would drop it unseen and still
//! pass `complete=true` on, which would then be wrong for it (ADR 0014). Such a change is a new
//! major (`crate::messages` documents the rule and the test that pins today's covered members). A different major is refused loudly ([`Version::check`]), never misread: the
//! handshake refuses it on connect, and every versioned result is checked before it is used
//! ([`crate::codec::decode_versioned`]).

use core::fmt;

use serde::{Deserialize, Serialize};

/// A `major.minor` version, `{"major":1,"minor":0}` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Version {
    /// Bumped by a rename, removal or retyped field: a new major and an ADR (plan §6.6).
    pub major: u16,
    /// Bumped by an added optional field or message.
    pub minor: u16,
}

/// The protocol a proxy and the daemon speak, checked by the handshake (`jfl/handshake`).
pub const PROTOCOL_VERSION: Version = Version { major: 1, minor: 0 };

/// The `schema_version` of every result a carrier prints (`--json`, plan §2.4): the envelope,
/// hover, symbols, diagnostics, status and sync.
pub const SCHEMA_VERSION: Version = Version { major: 1, minor: 0 };

/// A peer's version whose major is not ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("version {theirs} is not compatible with {ours} (another major): restart the daemon")]
pub struct MajorMismatch {
    /// The version this build speaks.
    pub ours: Version,
    /// The version the peer sent.
    pub theirs: Version,
}

impl Version {
    /// `Ok` when `theirs` has our major, whatever its minor; a different major is refused.
    ///
    /// Pinned by: `version::tests::should_accept_any_minor_when_the_major_matches` and
    /// `version::tests::should_refuse_a_peer_when_its_major_differs`.
    pub const fn check(self, theirs: Self) -> Result<(), MajorMismatch> {
        if self.major == theirs.major {
            Ok(())
        } else {
            Err(MajorMismatch { ours: self, theirs })
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_any_minor_when_the_major_matches() {
        // given
        let ours = Version { major: 1, minor: 0 };
        let newer = Version { major: 1, minor: 7 };

        // when
        let checked = ours.check(newer);

        // then
        assert_eq!(checked, Ok(()));
    }

    #[test]
    fn should_refuse_a_peer_when_its_major_differs() {
        // given
        let ours = Version { major: 1, minor: 3 };
        let theirs = Version { major: 2, minor: 0 };

        // when
        let checked = ours.check(theirs);

        // then
        assert_eq!(checked, Err(MajorMismatch { ours, theirs }));
    }
}
