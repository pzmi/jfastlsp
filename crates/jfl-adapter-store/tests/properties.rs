//! Property tests of the entry decoders on hostile bytes (plan §11, ver §8): arbitrary bytes, a
//! valid entry with any byte flipped or cut at any length, and arbitrary or mutated sections sealed
//! with a valid header and digest, so they reach postcard and `jfl-domain`'s checks. No input
//! panics; no input but a whole entry decodes; what decodes is a summary the domain accepts.
//!
//! Seeded (ver §3): every run starts from [`SEED`]; proptest writes a shrunk failure to
//! `proptest-regressions/`, which is committed.

use std::sync::OnceLock;

use jfl_adapter_store::codec::{decode, decode_declarations, encode, seal};
use jfl_application::Declarations;
use jfl_domain::FileSummary;
use jfl_testkit::contracts::summary_store::{corpus_summaries, key};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The fixed seed of every property here (ver §3 rule 2).
const SEED: u64 = 0x1DC0_0801;

fn config() -> ProptestConfig {
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/properties.txt",
        ))),
        ..ProptestConfig::default()
    }
}

/// The corpus's summaries and their entries under key 1.
fn entries() -> &'static [(FileSummary, Vec<u8>)] {
    static ENTRIES: OnceLock<Vec<(FileSummary, Vec<u8>)>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        corpus_summaries()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| encode(&key(1), &s).ok().map(|entry| (s, entry)))
            .collect()
    })
}

/// The sections of an entry: its head and its reference sites.
fn sections(entry: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let head_len = entry
        .get(40..44)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .and_then(|b| usize::try_from(u32::from_le_bytes(b)).ok())
        .unwrap_or(0);
    let payload = entry.get(64..).unwrap_or(&[]);
    let (head, references) = payload.split_at_checked(head_len).unwrap_or((payload, &[]));
    (head.to_vec(), references.to_vec())
}

/// Whatever `entry` decodes to is consistent: a whole summary re-encodes to itself, and its
/// declarations are [`Declarations::of`] it.
fn consistent(entry: &[u8]) -> Result<(), TestCaseError> {
    if let Ok(summary) = decode(&key(1), entry) {
        let again = encode(&key(1), &summary).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(decode(&key(1), &again), Ok(summary.clone()));
        prop_assert_eq!(
            decode_declarations(&key(1), &again),
            Ok(Declarations::of(&summary))
        );
    }
    let _ = decode_declarations(&key(1), entry);
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_refuse_arbitrary_bytes_when_decoding(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        // given
        let entry = bytes;
        // when
        let whole = decode(&key(1), &entry);
        let declarations = decode_declarations(&key(1), &entry);
        // then
        prop_assert!(whole.is_err());
        prop_assert!(declarations.is_err());
    }

    #[test]
    fn should_refuse_an_entry_when_any_byte_is_flipped(file in any::<prop::sample::Index>(), at in any::<prop::sample::Index>(), bit in 0_u8..8) {
        // given
        let (_, entry) = file.get(entries());
        let mut flipped = entry.clone();
        let at = at.index(flipped.len());
        flipped[at] ^= 1 << bit;
        // when
        let whole = decode(&key(1), &flipped);
        let declarations = decode_declarations(&key(1), &flipped);
        // then
        prop_assert!(whole.is_err(), "byte {} bit {}", at, bit);
        prop_assert!(declarations.is_err(), "byte {} bit {}", at, bit);
    }

    #[test]
    fn should_refuse_an_entry_when_it_is_cut_short(file in any::<prop::sample::Index>(), cut in any::<prop::sample::Index>()) {
        // given
        let (_, entry) = file.get(entries());
        let cut = cut.index(entry.len());
        // when
        let whole = decode(&key(1), &entry[..cut]);
        let declarations = decode_declarations(&key(1), &entry[..cut]);
        // then
        prop_assert!(whole.is_err());
        prop_assert!(declarations.is_err());
    }

    #[test]
    fn should_never_panic_when_arbitrary_sections_are_sealed(
        head in proptest::collection::vec(any::<u8>(), 0..256),
        references in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        // given
        let entry = seal(&key(1), &head, &references).unwrap();
        // when
        let checked = consistent(&entry);
        // then
        checked?;
    }

    #[test]
    fn should_never_panic_when_sealed_sections_are_mutated(
        file in any::<prop::sample::Index>(),
        edits in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>(), any::<bool>()), 1..4),
    ) {
        // given
        let (_, entry) = file.get(entries());
        let (mut head, mut references) = sections(entry);
        for (at, byte, in_head) in edits {
            let section = if in_head || references.is_empty() { &mut head } else { &mut references };
            if !section.is_empty() {
                let at = at.index(section.len());
                section[at] = byte;
            }
        }
        let entry = seal(&key(1), &head, &references).unwrap();
        // when
        let checked = consistent(&entry);
        // then
        checked?;
    }
}

#[test]
fn should_read_back_every_corpus_summary_when_its_entry_is_whole() {
    // given
    let entries = entries();
    assert_eq!(entries.len(), corpus_summaries().unwrap().len());
    for (summary, entry) in entries {
        // when
        let whole = decode(&key(1), entry);
        let declarations = decode_declarations(&key(1), entry);
        // then
        assert_eq!(whole.as_ref(), Ok(summary));
        assert_eq!(declarations, Ok(Declarations::of(summary)));
    }
}
