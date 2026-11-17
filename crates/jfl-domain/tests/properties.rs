//! Property tests of positions, paths and URIs (ver §2, plan §2.3), with `line-index`
//! (rust-analyzer, ADR 0020) as the independent oracle for UTF-16 and UTF-32 columns; and of
//! symbol identities, summaries and the result envelope (plan §2.1, IDX-02).
//!
//! Seeded (ver §3): every run starts from [`SEED`], so a failure reproduces as it is; proptest
//! shrinks it and writes the minimal case to `proptest-regressions/`, which is committed.

use jfl_domain::{
    Arity, ByteOffset, ByteRange, Completeness, DeclId, DeclKind, DeclarationInput, Envelope,
    EnvelopeParts, FileSummary, FileUri, Freshness, Generation, IncompleteReason, IndexState,
    LineIndex, Location, Modifiers, ModuleIndex, ModuleName, Position, PositionEncoding,
    PositionError, PossibleHit, Range, Reason, RefShape, RelPath, SummaryBuilder, SymbolId, View,
    Why, completeness,
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

/// A simple name: ASCII, `$` (quoted when printed) and non-ASCII letters.
fn name() -> impl Strategy<Value = String> {
    "[a-zA-Z_$é][a-zA-Z0-9_$é]{0,6}"
}

/// A parameter type as a summary stores it: dotted names, then array dimensions.
fn param_type() -> impl Strategy<Value = String> {
    (prop::collection::vec(name(), 1..3), 0..3_usize)
        .prop_map(|(parts, dims)| format!("{}{}", parts.join("."), "[]".repeat(dims)))
}

fn byte_range(start: u32, len: u32) -> Option<ByteRange> {
    ByteRange::new(
        ByteOffset::new(start),
        ByteOffset::new(start.saturating_add(len)),
    )
    .ok()
}

fn declaration<'a>(
    kind: DeclKind,
    parent: Option<DeclId>,
    name: &'a str,
    params: &'a [&'a str],
    at: ByteRange,
) -> DeclarationInput<'a> {
    DeclarationInput {
        kind,
        modifiers: Modifiers::empty(),
        parent,
        name,
        name_range: at,
        range: at,
        signature: name,
        params,
        supertypes: &[],
        permits: &[],
        annotations: &[],
        javadoc: None,
    }
}

/// A location among a few files and lines, so that duplicates are frequent.
fn location() -> impl Strategy<Value = Location> {
    (0..3_u32, 0..4_u32, 0..3_u32).prop_filter_map("a valid location", |(file, line, column)| {
        let at = Position::new(line, column);
        Some(Location {
            uri: FileUri::from_path(&format!("/repo/F{file}.java")).ok()?,
            range: Range::new(at, at).ok()?,
        })
    })
}

fn possible_hit() -> impl Strategy<Value = PossibleHit> {
    let reason = prop_oneof![
        Just(Reason::NameAndArity),
        Just(Reason::MethodReference),
        Just(Reason::ReceiverTypeUnknown),
    ];
    (location(), reason, prop::option::of("[a-z ]{0,6}")).prop_map(|(location, reason, detail)| {
        let why = detail.map_or(Why::new(reason), |detail| Why::with_detail(reason, &detail));
        PossibleHit { location, why }
    })
}

fn envelope(
    worktree: FileUri,
    exact: Vec<Location>,
    possible: Vec<PossibleHit>,
    completeness: Completeness,
) -> Envelope {
    Envelope::new(EnvelopeParts {
        view: View {
            worktree,
            head: None,
            generation: Generation(3),
        },
        target: None,
        exact,
        possible,
        completeness,
        freshness: Freshness {
            synced_at: None,
            pending_events: 0,
        },
        position_encoding: PositionEncoding::Utf16,
    })
}

/// Module `:m<module>`'s index state, consistent by construction: `indexed <= files`,
/// `stale <= indexed`.
fn module_state(module: u8) -> impl Strategy<Value = ModuleIndex> {
    (2..4_u64, 0..3_u32, 0..3_u32, 0..2_u32, 0..2_u32).prop_filter_map(
        "consistent counts",
        move |(generation, files, indexed, stale, missing)| {
            let indexed = indexed.min(files);
            let stale = stale.min(indexed);
            let name = ModuleName::new(&format!(":m{module}")).ok()?;
            if files == 0 {
                return Some(ModuleIndex::empty(name, Generation(generation), missing));
            }
            ModuleIndex::new(name, Generation(generation), files, indexed, stale, missing).ok()
        },
    )
}

/// The states of a few distinct modules.
fn module_states() -> impl Strategy<Value = Vec<ModuleIndex>> {
    prop::collection::btree_set(0..5_u8, 0..5)
        .prop_flat_map(|modules| modules.into_iter().map(module_state).collect::<Vec<_>>())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_round_trip_a_symbol_id_when_it_names_any_declaration_kind(
        package in prop::collection::vec(name(), 0..4),
        types in prop::collection::vec(name(), 1..4),
        kind in prop::sample::select(DeclKind::ALL.to_vec()),
        member in name(),
        params in prop::collection::vec(param_type(), 0..4),
    ) {
        // given: the declaration in a summary, under its enclosing types
        let at = byte_range(0, 2).unwrap();
        let mut builder = SummaryBuilder::new();
        if !package.is_empty() {
            builder.package(&package.join(".")).unwrap();
        }
        let mut parent = None;
        for name in &types {
            parent = Some(builder.declaration(&declaration(DeclKind::Class, parent, name, &[], at)).unwrap());
        }
        let params: Vec<&str> = params.iter().map(String::as_str).collect();
        let params: &[&str] = if kind.is_callable() { &params } else { &[] };
        let id = builder.declaration(&declaration(kind, parent, &member, params, at)).unwrap();
        let summary = builder.finish();
        // when
        let symbol = summary.symbol_id(id).unwrap();
        let printed = symbol.to_string();
        let parsed = SymbolId::parse(&printed);
        // then
        prop_assert_eq!(parsed.as_ref(), Ok(&symbol));
        prop_assert_eq!(parsed.unwrap().to_string(), printed);
        prop_assert_eq!(symbol.package().collect::<Vec<_>>(), package.iter().map(String::as_str).collect::<Vec<_>>());
        let mut expected_types: Vec<&str> = types.iter().map(String::as_str).collect();
        if kind.is_type() {
            expected_types.push(&member);
            prop_assert!(symbol.member().is_none());
        } else {
            let member_id = symbol.member().unwrap();
            prop_assert_eq!(member_id.params().is_some(), kind.is_callable());
            if kind != DeclKind::Constructor {
                prop_assert_eq!(member_id.name(), member.as_str());
            }
        }
        prop_assert_eq!(symbol.types().collect::<Vec<_>>(), expected_types);
    }

    #[test]
    fn should_never_panic_when_parsing_arbitrary_symbol_text(text in "[a-z$`.#(),<>\\[\\] é]{0,24}") {
        // given: an alphabet dense in the text form's punctuation
        // when
        let parsed = SymbolId::parse(&text);
        // then: whatever is accepted is printed back as it was read
        if let Ok(parsed) = parsed {
            prop_assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn should_build_the_same_envelope_when_inputs_are_shuffled(
        (exact, shuffled_exact) in prop::collection::vec(location(), 0..24)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
        (possible, shuffled_possible) in prop::collection::vec(possible_hit(), 0..48)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
        (modules, shuffled_modules) in module_states()
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
        (visible, shuffled_visible) in prop::collection::vec(0..6_u8, 0..6)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
    ) {
        // given
        let names = |v: &[u8]| -> Vec<ModuleName> {
            v.iter().map(|m| ModuleName::new(&format!(":m{m}")).unwrap()).collect()
        };
        let index = IndexState::new(Generation(3), modules).unwrap();
        let shuffled_index = IndexState::new(Generation(3), shuffled_modules).unwrap();
        // when
        let completeness_one = completeness(&names(&visible), &index);
        let completeness_two = completeness(&names(&shuffled_visible), &shuffled_index);
        let worktree = FileUri::from_path("/repo").unwrap();
        let one = envelope(worktree.clone(), exact.clone(), possible.clone(), completeness_one.clone());
        let two = envelope(worktree, shuffled_exact, shuffled_possible, completeness_two.clone());
        // then
        prop_assert_eq!(&completeness_one, &completeness_two);
        prop_assert_eq!(&one, &two);
        prop_assert!(one.exact().windows(2).all(|w| w[0] < w[1]));
        prop_assert!(one.possible().windows(2).all(|w| w[0].location < w[1].location));
        prop_assert!(one.possible().iter().all(|hit| !one.exact().contains(&hit.location)));
        let all: std::collections::BTreeSet<_> = exact.iter().chain(possible.iter().map(|h| &h.location)).collect();
        let kept: std::collections::BTreeSet<_> = one.exact().iter().chain(one.possible().iter().map(|h| &h.location)).collect();
        prop_assert_eq!(all, kept, "no location is lost or invented");
        prop_assert_eq!(one.complete(), one.incomplete_because().is_empty());
        if one.complete() {
            prop_assert!(!visible.is_empty());
        }
    }

    #[test]
    fn should_be_complete_only_when_no_visible_module_misses_a_condition(
        modules in module_states(),
        visible in prop::collection::vec(0..6_u8, 0..6),
    ) {
        // given: the definition, checked independently of `completeness`'s code
        let unique: std::collections::BTreeMap<_, _> =
            modules.iter().map(|m| (m.module().clone(), m)).collect();
        let index = IndexState::new(Generation(3), modules.clone()).unwrap();
        let names: Vec<ModuleName> = visible.iter().map(|m| ModuleName::new(&format!(":m{m}")).unwrap()).collect();
        let complete_by_definition = !names.is_empty()
            && names.iter().all(|name| {
                unique.get(name).is_some_and(|state| {
                    state.generation() == Generation(3)
                        && state.indexed() == state.files()
                        && state.stale() == 0
                        && state.missing_generated_roots() == 0
                })
            });
        // when
        let found = completeness(&names, &index);
        // then
        prop_assert_eq!(found.is_complete(), complete_by_definition);
        prop_assert_eq!(found.is_complete(), found.reasons().is_empty());
        prop_assert!(!found.clone().deadline().is_complete());
        prop_assert!(found.clone().deadline().reasons().contains(&IncompleteReason::Deadline));
    }

    #[test]
    fn should_rebuild_the_same_summary_when_its_parts_round_trip(
        names in prop::collection::vec(name(), 1..12),
        references in prop::collection::vec((0..12_usize, 0..1000_u32, 0..8_usize, 0..300_usize), 0..32),
    ) {
        // given
        let at = byte_range(0, 2).unwrap();
        let mut builder = SummaryBuilder::new();
        builder.package("com.acme").unwrap();
        let mut parent = None;
        for (i, name) in names.iter().enumerate() {
            let kind = if i % 3 == 0 { DeclKind::Class } else { DeclKind::Method };
            let id = builder.declaration(&declaration(kind, parent, name, &["int"], at)).unwrap();
            if kind.is_type() {
                parent = Some(id);
            }
        }
        for &(name, start, shape, arity) in &references {
            let name = names.get(name.checked_rem(names.len()).unwrap_or(0)).unwrap();
            let shape = RefShape::ALL[shape];
            builder.reference(name, byte_range(start, 3).unwrap(), shape, Arity::of(arity)).unwrap();
        }
        let summary = builder.finish();
        // when
        let rebuilt = FileSummary::from_parts(summary.parts().clone());
        // then
        prop_assert_eq!(rebuilt.as_ref(), Ok(&summary));
        prop_assert_eq!(summary.references().len(), references.len());
        let fixed = ["com.acme".to_owned(), "int".to_owned()];
        let distinct: std::collections::BTreeSet<_> = names.iter().chain(fixed.iter()).collect();
        prop_assert!(summary.parts().strings.len() <= distinct.len(), "each string stored once");
    }

    #[test]
    fn should_never_panic_when_summary_parts_are_corrupt(
        span_start in any::<u32>(), span_len in any::<u32>(), id in any::<u32>(),
        list_start in any::<u32>(), list_len in any::<u32>(), parent in any::<u32>(),
    ) {
        // given: a valid summary with one field of each kind overwritten
        let at = byte_range(0, 2).unwrap();
        let mut builder = SummaryBuilder::new();
        let class = builder.declaration(&declaration(DeclKind::Class, None, "A", &[], at)).unwrap();
        builder.declaration(&declaration(DeclKind::Method, Some(class), "f", &["int"], at)).unwrap();
        builder.reference("f", byte_range(0, 1).unwrap(), RefShape::Call, Arity::of(1)).unwrap();
        let good = builder.finish().into_parts();
        let mut variants = Vec::new();
        let mut parts = good.clone();
        parts.strings[0] = jfl_domain::Span { start: span_start, len: span_len };
        variants.push(parts);
        let mut parts = good.clone();
        parts.references[0].name = jfl_domain::StrId(id);
        variants.push(parts);
        let mut parts = good.clone();
        parts.declarations[1].params = jfl_domain::ListRef { start: list_start, len: list_len };
        variants.push(parts);
        let mut parts = good.clone();
        parts.declarations[1].parent = Some(DeclId(parent));
        variants.push(parts);
        for parts in variants {
            // when
            let checked = FileSummary::from_parts(parts);
            // then: accepted parts answer every query without panicking
            if let Ok(summary) = checked {
                for (id, decl) in summary.declarations() {
                    let _ = summary.symbol_id(id);
                    let _ = summary.list(decl.params).count();
                    let _ = summary.str(decl.name);
                }
                for reference in summary.references() {
                    let _ = summary.str(reference.name);
                }
            }
        }
    }
}
