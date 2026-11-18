//! The golden corpus (ver §4, IDX-07): every `.java` file under `fixtures/java/` summarises to its
//! golden with tree-sitter-java-orchard, the per-feature corpus `fixtures/java/grammar/` also with
//! upstream tree-sitter-java (the switch-back grammar of ADR 0022), and upstream's syntax errors
//! there are exactly the committed known gaps.

use jfl_adapter_java::{
    Grammar, GrammarSpec, JavaIndexer, StartupError, UPSTREAM_LOCALS, UPSTREAM_TAGS,
};
use jfl_application::GrammarId;
use jfl_domain::{ByteOffset, LineIndex, PositionEncoding};
use jfl_testkit::exec::InlinePool;
use jfl_testkit::golden::{self, CorpusFile};

fn orchard() -> Result<JavaIndexer<InlinePool>, StartupError> {
    Ok(JavaIndexer::new(
        Grammar::new(&GrammarSpec::orchard())?,
        InlinePool,
    ))
}

fn upstream() -> Result<JavaIndexer<InlinePool>, StartupError> {
    let spec = GrammarSpec {
        id: GrammarId {
            name: "tree-sitter-java",
            version: "0.23.5",
        },
        language: tree_sitter_java::LANGUAGE.into(),
        tags: UPSTREAM_TAGS,
        locals: UPSTREAM_LOCALS,
    };
    Ok(JavaIndexer::new(Grammar::new(&spec)?, InlinePool))
}

fn summarise(indexer: &JavaIndexer<InlinePool>, file: &CorpusFile) -> Result<String, String> {
    let indexed = indexer
        .index_blocking(&file.bytes)
        .map_err(|e| format!("refused: {e}"))?;
    golden::render(&indexed, &file.bytes)
}

fn report(mismatches: &[golden::Mismatch]) -> String {
    mismatches
        .iter()
        .map(|m| format!("{}: {}", m.path, m.detail))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn should_match_every_golden_when_orchard_summarises_the_corpus() {
    // given
    let indexer = orchard().unwrap();
    let files = golden::corpus("").unwrap();
    // when
    let mismatches = golden::check_corpus("orchard", &files, |f| summarise(&indexer, f));
    let stale = golden::stale_goldens("orchard", &files).unwrap();
    // then
    assert!(
        files.len() >= 15,
        "the corpus shrank: {} files",
        files.len()
    );
    assert!(mismatches.is_empty(), "{}", report(&mismatches));
    assert!(stale.is_empty(), "goldens with no corpus file: {stale:?}");
}

#[test]
fn should_match_every_golden_when_upstream_summarises_the_grammar_corpus() {
    // given
    let indexer = upstream().unwrap();
    let files = golden::corpus("grammar").unwrap();
    // when
    let mismatches = golden::check_corpus("upstream", &files, |f| summarise(&indexer, f));
    let stale = golden::stale_goldens("upstream", &files).unwrap();
    // then
    assert!(mismatches.is_empty(), "{}", report(&mismatches));
    assert!(stale.is_empty(), "goldens with no corpus file: {stale:?}");
}

#[test]
fn should_report_exactly_the_known_gaps_when_upstream_parses_the_grammar_corpus() {
    // given: the per-feature corpus parses cleanly with orchard
    let files = golden::corpus("grammar").unwrap();
    let (orchard, upstream) = (orchard().unwrap(), upstream().unwrap());
    // when
    let mut orchard_errors = Vec::new();
    let mut upstream_errors = Vec::new();
    for file in &files {
        if !orchard
            .index_blocking(&file.bytes)
            .unwrap()
            .diagnostics
            .is_empty()
        {
            orchard_errors.push(file.path.clone());
        }
        if !upstream
            .index_blocking(&file.bytes)
            .unwrap()
            .diagnostics
            .is_empty()
        {
            upstream_errors.push(file.path.clone());
        }
    }
    // then: a gap that opens or closes fails until known-gaps.txt says so
    assert_eq!(orchard_errors, Vec::<String>::new());
    assert_eq!(upstream_errors, golden::known_gaps("upstream").unwrap());
}

/// Every range a summary holds converts to a position, in the file as written and with its line
/// terminators turned into `\r\n` (IDX-01's decision: no range ends inside a `\r\n`).
#[test]
fn should_give_ranges_that_convert_when_files_end_lines_with_crlf() {
    // given
    let indexer = orchard().unwrap();
    let files = golden::corpus("").unwrap();
    for file in files {
        let text = String::from_utf8(file.bytes.clone()).unwrap();
        let crlf = text.replace("\r\n", "\n").replace('\n', "\r\n");
        for variant in [text, crlf] {
            // when
            let indexed = indexer.index_blocking(variant.as_bytes()).unwrap();
            // then
            let lines = LineIndex::new(&variant).unwrap();
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
                    let converted = lines.position(at, PositionEncoding::Utf16);
                    assert!(converted.is_ok(), "{}: {at:?}: {converted:?}", file.path);
                }
            }
            let end = ByteOffset::try_from_usize(variant.len()).unwrap();
            assert!(lines.position(end, PositionEncoding::Utf16).is_ok());
        }
    }
}

/// The names of the summary are the file's names as Java reads them (JLS §3.3, §3.8): an escaped
/// or ignorable spelling interns as the plain one, and its range is the escape as written.
#[test]
fn should_intern_one_spelling_when_names_use_escapes_or_ignorable_characters() {
    // given
    let file = golden::corpus("syntax")
        .unwrap()
        .into_iter()
        .find(|f| f.path == "syntax/UnicodeEscapes.java")
        .unwrap();
    // when
    let indexed = orchard().unwrap().index_blocking(&file.bytes).unwrap();
    // then
    let summary = &indexed.summary;
    let names: Vec<&str> = summary
        .declarations()
        .map(|(_, d)| summary.str(d.name))
        .collect();
    assert_eq!(
        names,
        [
            "UnicodeEscapes",
            "count",
            "name",
            "quote",
            "nul",
            "read",
            "run",
            "été"
        ]
    );
    let uses: Vec<&str> = summary
        .references()
        .iter()
        .map(|r| summary.str(r.name))
        .filter(|n| ["count", "name", "run"].contains(n))
        .collect();
    assert_eq!(uses, ["count", "name", "count"]);
    let (_, count) = summary.declarations().nth(1).unwrap();
    assert_eq!(count.name_range.len(), 10, "`\\u0063ount` as written");
}

/// A file read with a byte-order mark summarises as the file without it, at the same offsets
/// (IDX-01's decision: editors drop the mark from document text).
#[test]
fn should_summarise_as_without_it_when_a_file_starts_with_a_bom() {
    // given
    let indexer = orchard().unwrap();
    for file in golden::corpus("syntax").unwrap() {
        let mut with_bom = b"\xEF\xBB\xBF".to_vec();
        with_bom.extend_from_slice(&file.bytes);
        // when
        let plain = indexer.index_blocking(&file.bytes);
        let marked = indexer.index_blocking(&with_bom);
        // then
        assert_eq!(plain, marked, "{}", file.path);
    }
}

/// A NUL byte is not the end of the file: what follows it is still summarised, or the file says
/// it has a syntax error there (plan §2.1: no silently partial summary).
#[test]
fn should_read_past_a_nul_byte_when_one_sits_in_a_comment() {
    // given
    let text = b"class A { /* a \0 here */ void f() { g(); } }";
    // when
    let indexed = orchard().unwrap().index_blocking(text).unwrap();
    // then
    let summary = &indexed.summary;
    let calls: Vec<&str> = summary
        .references()
        .iter()
        .map(|r| summary.str(r.name))
        .collect();
    assert!(
        calls.contains(&"g") || !indexed.diagnostics.is_empty(),
        "{calls:?} {:?}",
        indexed.diagnostics
    );
}

/// The grammar's version in the `SyntaxKey` is the one `Cargo.toml` pins exactly and `Cargo.lock`
/// resolves (ADR 0022, amended 2026-11-17).
#[test]
fn should_name_the_locked_version_when_the_grammar_is_identified() {
    // given
    let manifest = include_str!("../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    // when
    let id = GrammarSpec::orchard().id;
    // then
    assert_eq!(id.name, "tree-sitter-java-orchard");
    assert!(
        manifest.contains(&format!("tree-sitter-java-orchard = \"={}\"", id.version)),
        "the manifest pins another version than {}",
        id.version
    );
    assert!(
        lock.contains(&format!(
            "name = \"tree-sitter-java-orchard\"\nversion = \"{}\"",
            id.version
        )),
        "Cargo.lock resolves another version than {}",
        id.version
    );
}

/// `syntax/LineEnds.java` with every line end turned into `\r\n` in memory (the repository keeps
/// LF only: `.gitattributes` allows no `-text`, fnd §7.4) summarises to its own golden in
/// `summaries/orchard-crlf/`: line comments end before the `\r`, no range ends inside a `\r\n`.
#[test]
fn should_match_the_crlf_golden_when_line_ends_are_crlf() {
    // given
    let indexer = orchard().unwrap();
    let file = golden::corpus("syntax")
        .unwrap()
        .into_iter()
        .find(|f| f.path == "syntax/LineEnds.java")
        .unwrap();
    let text = String::from_utf8(file.bytes).unwrap().replace('\n', "\r\n");
    let crlf = CorpusFile {
        path: file.path,
        bytes: text.into_bytes(),
    };
    // when
    let mismatches = golden::check_corpus("orchard-crlf", core::slice::from_ref(&crlf), |f| {
        summarise(&indexer, f)
    });
    let stale = golden::stale_goldens("orchard-crlf", core::slice::from_ref(&crlf)).unwrap();
    // then
    assert!(mismatches.is_empty(), "{}", report(&mismatches));
    assert!(stale.is_empty(), "{stale:?}");
}
