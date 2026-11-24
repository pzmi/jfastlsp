//! The LSP conformance level (ver §2): every transcript of `fixtures/lsp/` against the built
//! `jfl lsp` over stdio, black box, through `jfl-testkit`'s runner (its module docs give the
//! format and the normalisation). The `lsp-conformance` stage runs this target and
//! `lsp_daemon.rs`.
//!
//! Each transcript runs twice, the worktree spelled as it is and through a link above it, each
//! time in its own scratch directory: a git copy of `fixtures/lsp/worktree/` under
//! `a+b/c@d/` (so every answer naming a file the client never sent shows the canonical,
//! percent-encoded form), substituted for `{{root}}` as the client spells it and `{{root_uri}}` as
//! the daemon writes it, and the daemon's state directory, which `jfl lsp` starts a daemon in on
//! the first request that needs one. That daemon is asked to stop after the transcript, whatever
//! its outcome; the environment is cleared but for `PATH` (the daemon runs `git`).

#[cfg(test)]
mod support;

use std::path::Path;

use jfl_testkit::lsp_transcript::{Failure, parse, run_with_env, texts};
use support::{DEADLINE, World, fixtures};

/// Runs the transcript `text` against `jfl lsp` in `world`, its placeholders substituted.
fn run(world: &World, name: &str, text: &str) -> Result<(), Failure> {
    let transcript = parse(name, &world.substitute(text))?;
    let (args, path) = world.command();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_with_env(
        Path::new(env!("CARGO_BIN_EXE_jfl")),
        &args,
        &[("PATH", path.as_os_str())],
        &transcript,
        world.scratch(),
        DEADLINE,
    )
}

#[test]
fn should_pass_every_transcript_when_run_against_jfl_lsp() {
    // given
    let transcripts = texts(&fixtures()).unwrap();
    let names: Vec<&str> = transcripts.iter().map(|(name, _)| name.as_str()).collect();

    // when: each one with the worktree spelled as it is, and through a link above it (FND-17)
    let failures: Vec<String> = transcripts
        .iter()
        .flat_map(|(name, text)| {
            [
                ("plain", World::new()),
                ("through a link", World::through_link()),
            ]
            .into_iter()
            .filter_map(move |(spelling, world)| {
                run(&world, name, text)
                    .err()
                    .map(|e| format!("{spelling}: {e}"))
            })
        })
        .collect();

    // then
    for scenario in [
        "lifecycle-shutdown-exit",
        "lifecycle-exit-without-shutdown",
        "cancel-after-completion",
        "position-encoding-utf16-astral",
        "position-encoding-utf8",
        "document-sync",
        "uri-echo-worktree",
        "malformed-header-unknown-field",
        "oversized-frame",
        "unknown-request",
        "unknown-notification",
        "diagnostics-broken-syntax-errors",
        "diagnostics-broken-string-template",
        "diagnostics-position-encoding-utf16-astral",
        "diagnostics-position-encoding-utf8",
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
    let world = World::new();
    let wrong = r#"{"send": {"jsonrpc": "2.0", "id": 1, "method": "textDocument/noSuchMethod"}}
{"expect": {"jsonrpc": "2.0", "id": 1, "result": null}}
"#;
    let silent = r#"{"send": {"jsonrpc": "2.0", "method": "workspace/noSuchNotification"}}
{"expect": {"jsonrpc": "2.0", "id": null, "error": {"code": -32601, "message": "<any>"}}}
"#;
    let wrong_exit = r#"{"send": {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}}
{"send": {"jsonrpc": "2.0", "method": "exit"}}
{"expect": {"jsonrpc": "2.0", "id": 1, "result": "<any>"}}
{"exit": 0}
"#;

    // when
    let outcomes = [
        run(&world, "negative-control", wrong),
        run(&world, "negative-control-unanswered", silent),
        run(&world, "negative-control-exit", wrong_exit),
    ];

    // then
    for outcome in outcomes {
        assert!(outcome.is_err(), "{outcome:?}");
    }
}

#[test]
fn should_exit_zero_when_the_input_ends_after_shutdown() {
    // given
    let world = World::new();
    let clean = r#"{"send": {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}}
{"send": {"jsonrpc": "2.0", "id": 2, "method": "shutdown"}}
{"expect": {"jsonrpc": "2.0", "id": 1, "result": "<any>"}}
{"expect": {"jsonrpc": "2.0", "id": 2, "result": null}}
{"exit": 0}
"#;
    let early = r#"{"send": {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}}
{"expect": {"jsonrpc": "2.0", "id": 1, "result": "<any>"}}
{"exit": 1}
"#;

    // when
    let outcomes = [
        run(&world, "eof-after-shutdown", clean),
        run(&world, "eof-before-shutdown", early),
    ];

    // then
    assert_eq!(outcomes, [Ok(()), Ok(())]);
}
