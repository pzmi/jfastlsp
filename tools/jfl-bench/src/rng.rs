//! A seeded generator, so the same seed picks the same queries on every machine.
//!
//! SplitMix64 (Steele, Lea, Flood): twelve lines, no dependency, and identical output across
//! platforms and releases, which a `rand` version bump could not promise. Quality only has to
//! be good enough to shuffle a file list.

/// A SplitMix64 stream.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// A stream starting at `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value below `bound` (0 when `bound` is 0). The modulo bias is below 2^-40 for any
    /// corpus-sized bound, which a shuffle does not care about.
    pub fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).unwrap_or(u64::MAX);
        if bound == 0 {
            return 0;
        }
        self.next_u64()
            .checked_rem(bound)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_repeat_the_same_stream_when_the_seed_is_the_same() {
        // given
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        // when
        let xs: Vec<u64> = (0..4).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..4).map(|_| b.next_u64()).collect();
        // then
        assert_eq!(xs, ys);
    }

    #[test]
    fn should_match_the_reference_vector_when_seeded_with_zero() {
        // given
        let mut rng = SplitMix64::new(0);
        // when
        let first = rng.next_u64();
        let second = rng.next_u64();
        // then: the published SplitMix64 outputs for seed 0
        assert_eq!(first, 0xE220_A839_7B1D_CDAF);
        assert_eq!(second, 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn should_stay_below_the_bound_when_asked_for_a_bounded_value() {
        // given
        let mut rng = SplitMix64::new(7);
        // when
        let all_below = (0..1000).all(|_| rng.below(13) < 13);
        // then
        assert!(all_below);
        assert_eq!(rng.below(0), 0);
    }
}
