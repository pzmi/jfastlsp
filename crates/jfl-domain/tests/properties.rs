//! Property tests of positions, paths and URIs (ver §2, plan §2.3), with `line-index`
//! (rust-analyzer, ADR 0020) as the independent oracle for UTF-16 and UTF-32 columns.
//!
//! Seeded (ver §3): every run starts from [`SEED`], so a failure reproduces as it is; proptest
//! shrinks it and writes the minimal case to `proptest-regressions/`, which is committed.

use jfl_domain::{
    ByteOffset, FileUri, LineIndex, Position, PositionEncoding, PositionError, RelPath,
};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The fixed seed of every property here (ver §3 rule 2).
const SEED: u64 = 0x1DC0_0001;

const ENCODINGS: [PositionEncoding; 3] = [
    PositionEncoding::Utf8,
    PositionEncoding::Utf16,
    PositionEncoding::Utf32,
];

fn config() -> ProptestConfig {
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        // ver §3 rule 6: the crate's `proptest-regressions/` (the tests' working directory is the
        // crate's), not proptest's default beside a file outside `src/`.
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/properties.txt",
        ))),
        ..ProptestConfig::default()
    }
}

/// Text built from pieces that stress the index: ASCII, every terminator, 2-, 3- and 4-byte
/// scalars (astral ones are surrogate pairs in UTF-16), and arbitrary scalars.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => "[a-z ]{1,6}",
        2 => Just("\n".to_owned()),
        2 => Just("\r\n".to_owned()),
        1 => Just("\r".to_owned()),
        1 => Just("é".to_owned()),
        1 => Just("€".to_owned()),
        2 => Just("😀".to_owned()),
        1 => Just("\u{10FFFF}".to_owned()),
        2 => any::<char>().prop_map(String::from),
    ];
    prop::collection::vec(piece, 0..48).prop_map(|pieces| pieces.concat())
}

/// Whether `at` is the `\n` of a `\r\n`.
fn inside_crlf(text: &str, at: usize) -> bool {
    let bytes = text.as_bytes();
    at.checked_sub(1).and_then(|before| bytes.get(before)) == Some(&b'\r')
        && bytes.get(at) == Some(&b'\n')
}

/// `text` with every `\r` that does not start a `\r\n` replaced by `\n`: the same bytes at the
/// same offsets, read by `line-index` (which splits on `\n` only) with LSP 3.17's terminators.
fn lone_cr_as_lf(text: &str) -> String {
    let mut read = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && chars.peek() != Some(&'\n') {
            read.push('\n');
        } else {
            read.push(c);
        }
    }
    read
}

fn offset(at: usize) -> ByteOffset {
    // Generated texts are far below 4 GiB.
    ByteOffset::new(u32::try_from(at).unwrap_or(u32::MAX))
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_round_trip_every_char_boundary_when_converting_through_a_position(text in text()) {
        // given
        let index = LineIndex::new(&text).unwrap();
        for at in 0..=text.len() {
            for encoding in ENCODINGS {
                // when
                let position = index.position(offset(at), encoding);
                // then
                if !text.is_char_boundary(at) {
                    prop_assert_eq!(position, Err(PositionError::InsideCharacter { offset: offset(at).get() }));
                } else if inside_crlf(&text, at) {
                    prop_assert_eq!(position, Err(PositionError::InsideLineTerminator { offset: offset(at).get() }));
                } else {
                    let position = position.unwrap();
                    prop_assert_eq!(index.offset(position, encoding), Ok(offset(at)));
                }
            }
        }
        prop_assert_eq!(
            index.position(offset(text.len().saturating_add(1)), PositionEncoding::Utf16),
            Err(PositionError::OffsetPastEnd { offset: index.len().saturating_add(1), len: index.len() })
        );
    }

    #[test]
    fn should_refuse_or_round_trip_every_column_when_converting_to_an_offset(text in text()) {
        // given
        let index = LineIndex::new(&text).unwrap();
        for line in 0..=index.line_count() {
            for encoding in ENCODINGS {
                let Ok(length) = index.line_length(line, encoding) else {
                    prop_assert_eq!(line, index.line_count());
                    continue;
                };
                for column in 0..=length.saturating_add(1) {
                    // when
                    let found = index.offset(Position::new(line, column), encoding);
                    // then
                    match found {
                        Ok(at) => {
                            prop_assert!(text.is_char_boundary(at.get() as usize));
                            prop_assert_eq!(index.position(at, encoding), Ok(Position::new(line, column)));
                        }
                        Err(PositionError::ColumnPastLineEnd { line_length, .. }) => {
                            prop_assert_eq!(column, length.saturating_add(1));
                            prop_assert_eq!(line_length, length);
                        }
                        Err(PositionError::InsideSurrogatePair { .. }) => {
                            prop_assert_eq!(encoding, PositionEncoding::Utf16);
                        }
                        Err(PositionError::InsideCharacter { offset }) => {
                            prop_assert_eq!(encoding, PositionEncoding::Utf8);
                            prop_assert!(!text.is_char_boundary(offset as usize));
                        }
                        Err(other) => prop_assert!(false, "unexpected refusal {other:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn should_round_trip_between_encodings_when_the_position_is_valid(text in text()) {
        // given
        let index = LineIndex::new(&text).unwrap();
        for (at, _) in text.char_indices().filter(|&(at, _)| !inside_crlf(&text, at)) {
            let utf16 = index.position(offset(at), PositionEncoding::Utf16).unwrap();
            for to in ENCODINGS {
                // when
                let there = index.convert(utf16, PositionEncoding::Utf16, to).unwrap();
                let back = index.convert(there, to, PositionEncoding::Utf16);
                // then
                prop_assert_eq!(back, Ok(utf16));
            }
        }
    }

    #[test]
    fn should_agree_with_line_index_when_a_lone_cr_reads_as_a_newline(text in text()) {
        // given: line-index splits on `\n` only, so it reads a copy whose lone `\r`s are `\n`s
        let ours = LineIndex::new(&text).unwrap();
        let oracle = line_index::LineIndex::new(&lone_cr_as_lf(&text));
        for (at, _) in text.char_indices().chain([(text.len(), ' ')]) {
            if inside_crlf(&text, at) {
                continue;
            }
            let size = line_index::TextSize::new(offset(at).get());
            let utf8 = oracle.line_col(size);
            // when
            let found: Vec<_> = ENCODINGS.iter().map(|&e| ours.position(offset(at), e).unwrap()).collect();
            // then
            let utf16 = oracle.to_wide(line_index::WideEncoding::Utf16, utf8).unwrap();
            let utf32 = oracle.to_wide(line_index::WideEncoding::Utf32, utf8).unwrap();
            prop_assert_eq!(found, vec![
                Position::new(utf8.line, utf8.col),
                Position::new(utf16.line, utf16.col),
                Position::new(utf32.line, utf32.col),
            ]);
        }
    }

    #[test]
    fn should_hold_no_dot_dot_when_a_relative_path_is_accepted(path in "[a./\\x00é]{0,12}") {
        // given: an alphabet dense in separators and dots
        // when
        let accepted = RelPath::new(&path);
        // then
        if let Ok(accepted) = accepted {
            prop_assert!(accepted.components().all(|c| !c.is_empty() && c != "." && c != ".."));
            prop_assert!(!accepted.as_str().contains('\0'));
        } else {
            prop_assert!(path.is_empty() || path.split('/').any(|c| matches!(c, "" | "." | "..")) || path.contains('\0'));
        }
    }

    #[test]
    fn should_parse_what_it_prints_when_a_path_is_valid(
        components in prop::collection::vec("[^/\\x00]{1,8}", 1..6)
    ) {
        // given
        let path = format!("/{}", components.join("/"));
        let Ok(uri) = FileUri::from_path(&path) else {
            prop_assert!(components.iter().any(|c| c == "." || c == ".."));
            return Ok(());
        };
        // when
        let printed = uri.to_string();
        let parsed = FileUri::parse(&printed);
        // then
        prop_assert_eq!(parsed.as_ref(), Ok(&uri));
        prop_assert_eq!(uri.path(), path.as_str());
        prop_assert!(printed.bytes().all(|b| b.is_ascii_graphic()));
    }

    #[test]
    fn should_never_panic_when_parsing_arbitrary_uris(uri in "(file:|FILE:)?(//[a-z]{0,4})?[/%.a-f0-9?#\\x00é]{0,24}") {
        // given: an alphabet dense in escapes, dots and separators
        // when
        let parsed = FileUri::parse(&uri);
        // then: an accepted URI holds no `.` or `..` segment once decoded
        if let Ok(parsed) = parsed {
            prop_assert!(parsed.path().starts_with('/'));
            prop_assert!(parsed.path().split('/').skip(1).all(|c| c != "." && c != ".."));
        }
    }
}
