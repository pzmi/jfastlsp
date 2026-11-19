//! No panic on any bytes, and bounded work (plan §11, ver §8): arbitrary bytes, token soup and
//! mutated corpus files either summarise, with every range inside the text and convertible to a
//! position, or are refused with a typed error. Seeded (ver §3): every run starts from [`SEED`],
//! and a shrunk failure is written to `proptest-regressions/properties.txt` and committed; case
//! counts come from the gate stage, never from here.

use std::sync::OnceLock;

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_application::syntax_indexer::{Indexed, SyntaxIndexError, source_text};
use jfl_domain::{ByteOffset, ByteRange, LineIndex, PositionEncoding};
use jfl_testkit::exec::InlinePool;
use jfl_testkit::golden::{self, CorpusFile};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The fixed seed of every property here (ver §3 rule 2).
const SEED: u64 = 0x1DC0_0007;

fn config() -> ProptestConfig {
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/properties.txt",
        ))),
        ..ProptestConfig::default()
    }
}

fn indexer() -> Result<&'static JavaIndexer<InlinePool>, String> {
    static INDEXER: OnceLock<Result<JavaIndexer<InlinePool>, String>> = OnceLock::new();
    INDEXER
        .get_or_init(|| {
            Grammar::new(&GrammarSpec::orchard())
                .map(|grammar| JavaIndexer::new(grammar, InlinePool))
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn corpus() -> Result<&'static [CorpusFile], String> {
    static CORPUS: OnceLock<Result<Vec<CorpusFile>, String>> = OnceLock::new();
    CORPUS
        .get_or_init(|| golden::corpus(""))
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// Every range of `indexed` lies in the text of `bytes` and converts to a position.
fn ranges_hold(bytes: &[u8], indexed: &Indexed) -> Result<(), String> {
    let text = source_text(bytes).map_err(|e| e.to_string())?;
    let lines = LineIndex::new(text).map_err(|e| e.to_string())?;
    let summary = &indexed.summary;
    let ranges = summary
        .imports()
        .iter()
        .map(|i| i.range)
        .chain(summary.declarations().flat_map(|(_, d)| {
            [Some(d.range), Some(d.name_range), d.javadoc]
                .into_iter()
                .flatten()
        }))
        .chain(summary.references().iter().map(|r| r.range))
        .chain(indexed.diagnostics.iter().map(|d| d.range));
    for range in ranges {
        for at in [range.start(), range.end()] {
            lines
                .position(at, PositionEncoding::Utf16)
                .map_err(|e| format!("{range:?}: {e}"))?;
        }
    }
    Ok(())
}

fn answers(bytes: &[u8]) -> Result<(), String> {
    match indexer()?.index_blocking(bytes) {
        Ok(indexed) => ranges_hold(bytes, &indexed),
        Err(
            SyntaxIndexError::NotUtf8 { .. }
            | SyntaxIndexError::TooDeep
            | SyntaxIndexError::TooMuchWork
            | SyntaxIndexError::TooLarge { .. },
        ) => Ok(()),
        Err(other) => Err(format!("unexpected refusal: {other}")),
    }
}

const TOKENS: [&str; 40] = [
    "class ",
    "interface ",
    "record ",
    "enum ",
    "@interface ",
    "A",
    "b",
    "\\u0041",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "<",
    ">",
    ";",
    ",",
    ".",
    "::",
    "->",
    "...",
    "@",
    "=",
    "\"",
    "\"\"\"",
    "'",
    "/*",
    "*/",
    "/**",
    "//",
    "\n",
    "\r\n",
    "\r",
    " ",
    "import ",
    "package ",
    "new ",
    "this",
    "\u{200b}",
];

#[derive(Debug, Clone)]
enum Mutation {
    Delete { at: usize, len: usize },
    Insert { at: usize, token: usize },
    Flip { at: usize, byte: u8 },
    Duplicate { at: usize, len: usize },
}

fn apply(bytes: &[u8], mutations: &[Mutation]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for mutation in mutations {
        let at =
            |at: usize, out: &[u8]| at.checked_rem(out.len().max(1)).unwrap_or(0).min(out.len());
        match *mutation {
            Mutation::Delete { at: start, len } => {
                let start = at(start, &out);
                let end = start.saturating_add(len).min(out.len());
                out.drain(start..end);
            }
            Mutation::Insert { at: start, token } => {
                let start = at(start, &out);
                let token = TOKENS
                    .get(token.checked_rem(TOKENS.len()).unwrap_or(0))
                    .map_or(&b""[..], |t| t.as_bytes());
                out.splice(start..start, token.iter().copied());
            }
            Mutation::Flip { at: start, byte } => {
                let start = at(start, &out);
                if let Some(b) = out.get_mut(start) {
                    *b = byte;
                }
            }
            Mutation::Duplicate { at: start, len } => {
                let start = at(start, &out);
                let end = start.saturating_add(len).min(out.len());
                let copy = out.get(start..end).unwrap_or(&[]).to_vec();
                out.splice(end..end, copy);
            }
        }
    }
    out
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<usize>(), 0usize..64).prop_map(|(at, len)| Mutation::Delete { at, len }),
        (any::<usize>(), any::<usize>()).prop_map(|(at, token)| Mutation::Insert { at, token }),
        (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Flip { at, byte }),
        (any::<usize>(), 0usize..256).prop_map(|(at, len)| Mutation::Duplicate { at, len }),
    ]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_answer_without_panicking_when_given_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // given: the bytes
        // when
        let answered = answers(&bytes);
        // then
        prop_assert_eq!(answered, Ok(()));
    }

    #[test]
    fn should_answer_without_panicking_when_given_token_soup(tokens in proptest::collection::vec(0usize..TOKENS.len(), 0..512)) {
        // given
        let text: String = tokens.iter().map(|&t| TOKENS[t]).collect();
        // when
        let answered = answers(text.as_bytes());
        // then
        prop_assert_eq!(answered, Ok(()));
    }

    #[test]
    fn should_answer_without_panicking_when_a_corpus_file_is_mutated(
        file in 0usize..64,
        mutations in proptest::collection::vec(mutation(), 1..8),
    ) {
        // given
        let corpus = corpus().unwrap();
        let original = corpus.get(file.checked_rem(corpus.len()).unwrap()).unwrap();
        let bytes = apply(&original.bytes, &mutations);
        // when
        let answered = answers(&bytes);
        // then
        prop_assert_eq!(answered, Ok(()), "{} mutated by {:?}", original.path, mutations);
    }

    #[test]
    fn should_give_site_facts_without_panicking_when_a_corpus_file_is_mutated(
        file in 0usize..64,
        mutations in proptest::collection::vec(mutation(), 1..8),
        start in 0u32..4096,
        len in 0u32..16,
    ) {
        // given: a mutated file, and every reference site of the original plus an arbitrary
        // range (IDX-10's syntax facts read the request's file at query time)
        let corpus = corpus().unwrap();
        let original = corpus.get(file.checked_rem(corpus.len()).unwrap()).unwrap();
        let bytes = apply(&original.bytes, &mutations);
        let indexer = indexer().unwrap();
        let mut ranges: Vec<ByteRange> = indexer
            .index_blocking(&original.bytes)
            .map(|i| i.summary.references().iter().map(|r| r.range).collect())
            .unwrap_or_default();
        ranges.extend(
            ByteRange::new(ByteOffset::new(start), ByteOffset::new(start.saturating_add(len))).ok(),
        );
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // when
        let facts = indexer.sites_syntax_blocking(&text, &ranges);
        // then: an answer (or none) for each range
        prop_assert_eq!(facts.len(), ranges.len());
    }
}

#[test]
fn should_answer_without_panicking_when_nesting_reaches_the_limit() {
    // given: nesting at the port's limit, in the shapes a parser recurses on
    let depth = usize::try_from(jfl_application::syntax_indexer::MAX_NESTING).unwrap();
    let inputs = [
        format!(
            "class A {{ int x = {}1{}; }}",
            "(".repeat(depth.saturating_sub(1)),
            ")".repeat(depth.saturating_sub(1))
        ),
        format!(
            "class A {{ void f() {}{} }}",
            "{".repeat(depth.saturating_sub(2)),
            "}".repeat(depth.saturating_sub(2))
        ),
        format!(
            "class A {{ int[] x = {}; }}",
            "new int[1][".repeat(depth.checked_div(2).unwrap())
        ),
        "class A { int x = 1".to_owned() + &" + 1".repeat(100_000) + "; }",
        "class A { ".to_owned() + &"int x; ".repeat(50_000) + "}",
    ];
    for input in inputs {
        // when
        let answered = answers(input.as_bytes());
        // then
        assert_eq!(answered, Ok(()), "{}", &input[..64]);
    }
}
