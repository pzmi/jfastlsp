//! The LSP conformance level (ver §2): every transcript of `fixtures/lsp/` against a server binary
//! over stdio, black box, through `jfl-testkit`'s runner (its module docs give the format and the
//! normalisation). The server is the test-only `jfl-lsp-stub` on this adapter until `jfl lsp`
//! exists; LSP-02 points this level at `jfl lsp` (`crates/jfl-cli/tests/lsp_*.rs`, ver §2).
//! The `lsp-conformance` stage runs this target.

use std::path::{Path, PathBuf};
use std::time::Duration;

use jfl_testkit::lsp_transcript::{Transcript, load_dir, parse, run};

/// A transcript's whole session, the 16 MiB oversized frame included, takes well under a second;
/// the bound only stops a server that does not exit.
const DEADLINE: Duration = Duration::from_secs(20);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/lsp")
}

fn stub(transcript: &Transcript) -> Result<(), jfl_testkit::lsp_transcript::Failure> {
    run(
        Path::new(env!("CARGO_BIN_EXE_jfl-lsp-stub")),
        &[],
        transcript,
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        DEADLINE,
    )
}

#[test]
fn should_pass_every_transcript_when_run_against_the_server() {
    // given
    let transcripts = load_dir(&fixtures()).unwrap();
    let names: Vec<&str> = transcripts.iter().map(|t| t.name.as_str()).collect();

    // when
    let failures: Vec<String> = transcripts
        .iter()
        .filter_map(|t| stub(t).err().map(|e| e.to_string()))
        .collect();

    // then
    for scenario in [
        "malformed-header-unknown-field",
        "oversized-frame",
        "unknown-request",
        "unknown-notification",
        "uri-echo",
    ] {
        assert!(
            names.contains(&scenario),
            "{scenario} missing from {names:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn should_fail_a_transcript_when_the_server_answers_otherwise() {
    // given
    let wrong = parse(
        "negative-control",
        r#"{"send": {"jsonrpc": "2.0", "id": 1, "method": "textDocument/noSuchMethod"}}
{"expect": {"jsonrpc": "2.0", "id": 1, "result": null}}
"#,
    )
    .unwrap();
    let silent = parse(
        "negative-control-unanswered",
        r#"{"send": {"jsonrpc": "2.0", "method": "workspace/noSuchNotification"}}
{"expect": {"jsonrpc": "2.0", "id": null, "error": {"code": -32601, "message": "<any>"}}}
"#,
    )
    .unwrap();

    let wrong_exit = parse(
        "negative-control-exit",
        r#"{"send": {"jsonrpc": "2.0", "method": "workspace/noSuchNotification"}}
{"exit": 3}
"#,
    )
    .unwrap();

    // when
    let outcomes = [stub(&wrong), stub(&silent), stub(&wrong_exit)];

    // then
    for outcome in outcomes {
        assert!(outcome.is_err(), "{outcome:?}");
    }
}

#[test]
fn should_check_the_exit_code_when_a_transcript_states_it() {
    // given
    let clean = parse(
        "exit-after-eof",
        r#"{"send": {"jsonrpc": "2.0", "method": "workspace/noSuchNotification"}}
{"exit": 0}
"#,
    )
    .unwrap();

    // when
    let outcome = stub(&clean);

    // then
    assert_eq!(outcome, Ok(()));
}
