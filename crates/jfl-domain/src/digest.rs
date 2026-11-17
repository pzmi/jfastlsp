//! Content digests and the hash function that combines them, both supplied by the caller
//! (plan §5.4, ADR 0025).
//!
//! `jfl-domain` computes no hash: an adapter reads bytes and digests them with `blake3`, and the
//! key derivations here feed a [`KeyHasher`] the adapter supplies. What the domain owns is the
//! *encoding* of each key's inputs: every field length-prefixed and every key kind tagged with its
//! own name and format version, so two different inputs never feed the hasher the same bytes and a
//! `SyntaxKey` can never equal an `EnvironmentKey` by construction.
//!
//! Rejected alternative: taking `blake3` as a dependency here. It compiles C and assembly through a
//! build script and is a choice of the storage adapter (ADR 0025); a trait keeps the pure crate
//! free of it and lets tests use a deterministic in-crate hasher.

use core::fmt;

/// A 32-byte content digest (blake3 in production), computed by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The digest with these bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Lower-case hexadecimal, 64 digits.
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// An incremental hash function that ends in a [`Digest`]: the adapter's `blake3::Hasher`.
///
/// The key derivations call [`KeyHasher::update`] with an unambiguous encoding of their inputs
/// (see the module documentation) and then [`KeyHasher::finish`] once.
pub trait KeyHasher {
    /// Feeds `bytes` to the hash.
    fn update(&mut self, bytes: &[u8]);

    /// The digest of everything fed so far.
    fn finish(self) -> Digest;
}

/// Writes length-prefixed fields into a [`KeyHasher`], so that no two field sequences encode to
/// the same bytes (a field boundary can never be moved by the content of a field).
pub(crate) struct Encoder<H> {
    hasher: H,
}

impl<H: KeyHasher> Encoder<H> {
    /// An encoder whose first field is `tag`, the name and format version of what is encoded.
    pub(crate) fn new(hasher: H, tag: &str) -> Self {
        let mut encoder = Self { hasher };
        encoder.str(tag);
        encoder
    }

    /// A length, as 8 little-endian bytes.
    pub(crate) fn len(&mut self, len: usize) {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        self.hasher.update(&len.to_le_bytes());
    }

    /// A byte string, length first.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.len(bytes.len());
        self.hasher.update(bytes);
    }

    /// A UTF-8 string, length first.
    pub(crate) fn str(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }

    /// A number, as 4 little-endian bytes.
    pub(crate) fn u32(&mut self, value: u32) {
        self.hasher.update(&value.to_le_bytes());
    }

    /// A digest, fixed width.
    pub(crate) fn digest(&mut self, digest: &Digest) {
        self.hasher.update(digest.as_bytes());
    }

    /// The digest of everything encoded.
    pub(crate) fn finish(self) -> Digest {
        self.hasher.finish()
    }
}

/// A deterministic stand-in for blake3 in this crate's tests: it records every byte fed to it
/// and folds them into a digest, so two encodings compare equal exactly when their bytes do.
#[cfg(test)]
pub(crate) mod testing {
    use super::{Digest, KeyHasher};
    use alloc::vec::Vec;

    /// Keeps the bytes; the digest is a 32-lane FNV-1a fold of them.
    #[derive(Debug, Default)]
    pub(crate) struct RecordingHasher {
        pub(crate) bytes: Vec<u8>,
    }

    impl KeyHasher for RecordingHasher {
        fn update(&mut self, bytes: &[u8]) {
            self.bytes.extend_from_slice(bytes);
        }

        fn finish(self) -> Digest {
            let mut lanes = [0xcbf2_9ce4_8422_2325_u64; 4];
            for (i, byte) in self.bytes.iter().enumerate() {
                if let Some(lane) = lanes.get_mut(i % 4) {
                    *lane = (*lane ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
            let mut out = [0_u8; 32];
            for (chunk, lane) in out.as_chunks_mut::<8>().0.iter_mut().zip(lanes) {
                chunk.copy_from_slice(&lane.to_le_bytes());
            }
            Digest::from_bytes(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::RecordingHasher;
    use super::*;
    use alloc::format;

    #[test]
    fn should_encode_differently_when_a_field_boundary_moves() {
        // given
        let mut ab_c = Encoder::new(RecordingHasher::default(), "t");
        ab_c.str("ab");
        ab_c.str("c");
        let mut a_bc = Encoder::new(RecordingHasher::default(), "t");
        a_bc.str("a");
        a_bc.str("bc");
        // when
        let (left, right) = (ab_c.finish(), a_bc.finish());
        // then
        assert_ne!(left, right);
    }

    #[test]
    fn should_print_lower_case_hex_when_a_digest_is_displayed() {
        // given
        let mut bytes = [0_u8; 32];
        bytes[0] = 0xAB;
        bytes[31] = 0x01;
        // when
        let text = format!("{}", Digest::from_bytes(bytes));
        // then
        assert_eq!(text.len(), 64);
        assert!(text.starts_with("ab00"));
        assert!(text.ends_with("0001"));
    }
}
