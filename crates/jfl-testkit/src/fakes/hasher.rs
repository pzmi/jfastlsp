//! [`FnvHasher`]: a [`KeyHasher`] for tests, so a test keys summaries without the store adapter's
//! blake3 (which only `jfl-adapter-store` may name). Not collision-resistant: fine for the few
//! distinct contents a test writes.

use jfl_domain::{Digest, KeyHasher};

/// FNV-1a over the bytes fed, spread over a 32-byte digest.
#[derive(Debug, Clone, Copy)]
pub struct FnvHasher(u64);

impl Default for FnvHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl KeyHasher for FnvHasher {
    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(self) -> Digest {
        let mut out = [0_u8; 32];
        let mut state = self.0;
        for chunk in out.chunks_mut(8) {
            chunk.copy_from_slice(&state.to_le_bytes());
            state = state.rotate_left(17).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
        Digest::from_bytes(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_give_equal_digests_when_the_bytes_are_equal() {
        // given
        let digest = |bytes: &[u8]| {
            let mut hasher = FnvHasher::default();
            hasher.update(bytes);
            hasher.finish()
        };

        // when
        let (a, b, c) = (
            digest(b"class A {}"),
            digest(b"class A {}"),
            digest(b"class B {}"),
        );

        // then
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
