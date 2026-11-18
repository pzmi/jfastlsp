//! The `syntax_indexer` contract (ver §5, plan §6.4): the golden corpus's summaries, syntax
//! errors located, the port's refusals, a dropped byte-order mark, determinism, and an answer
//! (a summary or a typed refusal, never a panic) on hostile bytes. Checked the same way against
//! [`crate::fakes::syntax_indexer::CannedSyntaxIndexer`] (`syntax_indexer::tests` here) and
//! against `jfl-adapter-java` in `crates/jfl-adapter-java/tests/contract.rs`.
//!
//! The expectations are literal (the goldens, the error positions below), never computed from
//! code an implementation shares, so a wrong shared rule fails both. The suite's own tests run it
//! against deliberately broken implementations and assert each fails the case it should.

use jfl_application::syntax_indexer::{
    MAX_NESTING, MAX_SOURCE_BYTES, SyntaxIndexError, SyntaxIndexer,
};

use crate::exec::block_on;
use crate::golden;

/// A case the implementation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

type Case<I> = fn(&I) -> Result<(), String>;

/// Runs every case; an empty list is a pass.
pub fn check<I: SyntaxIndexer>(indexer: &I) -> Vec<Violation> {
    let cases: [(&'static str, Case<I>); 6] = [
        ("summarises_the_golden_corpus", summarises_the_golden_corpus),
        ("locates_syntax_errors", locates_syntax_errors),
        ("refuses_what_it_cannot_index", refuses_what_it_cannot_index),
        ("drops_a_leading_bom", drops_a_leading_bom),
        ("is_deterministic", is_deterministic),
        ("answers_hostile_bytes", answers_hostile_bytes),
    ];
    cases
        .into_iter()
        .filter_map(|(case, run)| run(indexer).err().map(|detail| Violation { case, detail }))
        .collect()
}

/// The goldens' directory and corpus of the implementation's grammar.
fn goldens_of<I: SyntaxIndexer>(indexer: &I) -> (&'static str, &'static str) {
    if indexer.grammar().name == "tree-sitter-java-orchard" {
        ("orchard", "")
    } else {
        ("upstream", "grammar")
    }
}

fn corpus_file(path: &str) -> Result<golden::CorpusFile, String> {
    golden::corpus("")?
        .into_iter()
        .find(|f| f.path == path)
        .ok_or_else(|| format!("{path} is not in the corpus"))
}

fn summarises_the_golden_corpus<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let (dir, files) = goldens_of(indexer);
    let mut problems = Vec::new();
    for file in golden::corpus(files)? {
        let path = golden::golden_path(dir, &file.path);
        let want =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let got = block_on(indexer.index(file.bytes.clone()))
            .map_err(|e| format!("{}: refused: {e}", file.path))
            .and_then(|indexed| golden::render(&indexed, &file.bytes))?;
        if let Some(difference) = golden::first_difference(&want, &got) {
            problems.push(format!("{}: {difference}", file.path));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn locates_syntax_errors<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let file = corpus_file("broken/SyntaxErrors.java")?;
    let indexed = block_on(indexer.index(file.bytes.clone())).map_err(|e| e.to_string())?;
    let golden = golden::render(&indexed, &file.bytes)?;
    // The positions the fixture's errors are at, as written there.
    let want = [
        "{\"at\":\"5:29-5:29\",\"problem\":{\"missing\":\";\"}",
        "{\"at\":\"9:17-9:18\",\"problem\":\"unexpected\"",
        "{\"at\":\"10:15-10:15\",\"problem\":{\"missing\":\")\"}",
        "{\"at\":\"15:6-15:6\",\"problem\":{\"missing\":\"}\"}",
    ];
    let diagnostics: Vec<&str> = golden
        .lines()
        .skip_while(|l| !l.contains("\"diagnostics\": ["))
        .skip(1)
        .take_while(|l| l.trim_start().starts_with('{'))
        .map(str::trim_start)
        .collect();
    let located = diagnostics.len() == want.len()
        && diagnostics
            .iter()
            .zip(want)
            .all(|(got, want)| got.starts_with(want));
    if located && indexed.diagnostics_dropped == 0 {
        Ok(())
    } else {
        Err(format!("diagnostics {diagnostics:?}, want {want:?}"))
    }
}

fn refuses_what_it_cannot_index<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let depth = usize::try_from(MAX_NESTING).map_err(|e| e.to_string())?;
    let cases = [
        (
            vec![b' '; MAX_SOURCE_BYTES.saturating_add(1)],
            SyntaxIndexError::TooLarge {
                len: MAX_SOURCE_BYTES.saturating_add(1),
            },
        ),
        (
            b"class A { String s = \"\xC3\x28\"; }".to_vec(),
            SyntaxIndexError::NotUtf8 { valid_up_to: 22 },
        ),
        (
            format!("class A {{ int x = {}; }}", "(".repeat(depth)).into_bytes(),
            SyntaxIndexError::TooDeep,
        ),
    ];
    for (bytes, want) in cases {
        let got = block_on(indexer.index(bytes));
        if got.as_ref().err() != Some(&want) {
            return Err(format!("got {:?}, want {want:?}", got.map(|_| "a summary")));
        }
    }
    Ok(())
}

fn drops_a_leading_bom<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let file = corpus_file("broken/SyntaxErrors.java")?;
    let mut marked = b"\xEF\xBB\xBF".to_vec();
    marked.extend_from_slice(&file.bytes);
    let plain = block_on(indexer.index(file.bytes));
    let with_bom = block_on(indexer.index(marked));
    if plain.is_ok() && plain == with_bom {
        Ok(())
    } else {
        Err(format!(
            "with a BOM: {:?}; without: {:?}",
            with_bom.map(|i| i.diagnostics),
            plain.map(|i| i.diagnostics)
        ))
    }
}

fn is_deterministic<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let first = corpus_file("syntax/References.java")?;
    let other = corpus_file("broken/SyntaxErrors.java")?;
    let once = block_on(indexer.index(first.bytes.clone()));
    let _ = block_on(indexer.index(other.bytes));
    let again = block_on(indexer.index(first.bytes));
    if once == again {
        Ok(())
    } else {
        Err("the same bytes summarised differently after another file".to_owned())
    }
}

fn answers_hostile_bytes<I: SyntaxIndexer>(indexer: &I) -> Result<(), String> {
    let depth = usize::try_from(MAX_NESTING).map_err(|e| e.to_string())?;
    let hostile: [Vec<u8>; 6] = [
        Vec::new(),
        b"class".to_vec(),
        b"}}}}))))]]]]".to_vec(),
        "(".repeat(depth).into_bytes(),
        "\u{0}\u{1}\u{7f}\u{feff}\u{200b}\\u0000\\uD800"
            .as_bytes()
            .to_vec(),
        b"class A { void f() { g(\"unterminated); } }".to_vec(),
    ];
    for bytes in hostile {
        let once = block_on(indexer.index(bytes.clone()));
        let again = block_on(indexer.index(bytes.clone()));
        match &once {
            Ok(_) | Err(SyntaxIndexError::TooMuchWork) => {}
            Err(other) => return Err(format!("{bytes:?}: unexpected refusal {other}")),
        }
        if once != again {
            return Err(format!("{bytes:?}: answered differently twice"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use jfl_application::syntax_indexer::{GrammarId, Indexed};

    use super::*;
    use crate::fakes::syntax_indexer::CannedSyntaxIndexer;

    const ORCHARD: GrammarId = GrammarId {
        name: "tree-sitter-java-orchard",
        version: "0.5.25",
    };

    fn canned() -> CannedSyntaxIndexer {
        CannedSyntaxIndexer::from_goldens(ORCHARD, "orchard", "").unwrap()
    }

    #[derive(Debug, Clone, Copy)]
    enum Fault {
        RefusesABom,
        Unbounded,
        DropsADiagnostic,
        DropsAReference,
        Unstable,
    }

    struct Broken {
        inner: CannedSyntaxIndexer,
        fault: Fault,
        /// How often each input was seen: `Unstable` answers differently on a repeat.
        seen: Mutex<BTreeMap<Vec<u8>, u32>>,
    }

    impl Broken {
        fn answer(&self, bytes: &[u8]) -> Result<Indexed, SyntaxIndexError> {
            let seen = self
                .seen
                .lock()
                .map(|mut seen| {
                    let count = seen.entry(bytes.to_vec()).or_default();
                    *count = count.saturating_add(1);
                    *count
                })
                .unwrap();
            let answer = self.inner.answer(bytes);
            match self.fault {
                Fault::RefusesABom if bytes.starts_with(b"\xEF\xBB\xBF") => {
                    Err(SyntaxIndexError::NotUtf8 { valid_up_to: 0 })
                }
                Fault::Unbounded => answer.or_else(|_| self.inner.answer(b"")),
                Fault::DropsADiagnostic => answer.map(|mut i| {
                    i.diagnostics.pop();
                    i
                }),
                Fault::DropsAReference => answer.map(|i| {
                    let mut parts = i.summary.clone().into_parts();
                    let mut refs = parts.references.into_vec();
                    refs.pop();
                    parts.references = refs.into_boxed_slice();
                    Indexed {
                        summary: jfl_domain::FileSummary::from_parts(parts).unwrap(),
                        ..i
                    }
                }),
                Fault::Unstable if seen.is_multiple_of(2) => answer.map(|mut i| {
                    i.diagnostics_dropped = 7;
                    i
                }),
                _ => answer,
            }
        }
    }

    impl SyntaxIndexer for Broken {
        fn grammar(&self) -> GrammarId {
            ORCHARD
        }

        fn summariser_version(&self) -> u32 {
            1
        }

        fn index(
            &self,
            bytes: Vec<u8>,
        ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
            core::future::ready(self.answer(&bytes))
        }
    }

    fn failed(fault: Fault) -> Vec<&'static str> {
        let broken = Broken {
            inner: canned(),
            fault,
            seen: Mutex::new(BTreeMap::new()),
        };
        check(&broken).into_iter().map(|v| v.case).collect()
    }

    #[test]
    fn should_pass_every_case_when_the_canned_fake_answers() {
        // given
        let fake = canned();
        // when
        let violations = check(&fake);
        // then
        assert_eq!(violations, Vec::new());
    }

    #[test]
    fn should_fail_the_case_it_breaks_when_an_implementation_is_broken() {
        // given
        let faults = [
            (Fault::RefusesABom, "drops_a_leading_bom"),
            (Fault::Unbounded, "refuses_what_it_cannot_index"),
            (Fault::DropsADiagnostic, "locates_syntax_errors"),
            (Fault::DropsAReference, "summarises_the_golden_corpus"),
            (Fault::Unstable, "is_deterministic"),
        ];
        for (fault, case) in faults {
            // when
            let failed = failed(fault);
            // then
            assert!(
                failed.contains(&case),
                "{fault:?} failed {failed:?}, not {case}"
            );
        }
    }
}
