//! The stage catalog frozen by XT-01 (ADR 0035): `xtask list --json` must name exactly the stages
//! the justfile's `check` ran before XT-01 (its `check_stages`), in the same order, with
//! `quick`'s stages (its `quick_stages`) marked, and each stage's command the one its recipe ran.
//! A stage added later adds its row here in the commit that adds it; a stage is never dropped
//! (ADR 0035, the XT lane's rule "nothing stops running") except by the owner's order:
//! `private-names` was removed on 2026-11-17 (FND-11). XT-08 reruns this test after just goes.

use std::error::Error;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

/// The justfile's `check_stages` before XT-01 (commit 201a4cc), word for word, less the
/// `private-names` stage the owner had removed (FND-11).
const CHECK_STAGES: &str = "fmt-check architecture clippy nostd docs unit-tests integration-tests doctests script-tests deny secrets workflows";

/// The justfile's `quick_stages` before XT-01, word for word.
const QUICK_STAGES: &str = "fmt-check architecture clippy unit-tests script-tests";

/// `xtask list --json` as XT-01 froze it: each `command` is the one its recipe ran (without the
/// recipe's `env RUSTFLAGS='-D warnings' RUSTDOCFLAGS= RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER=`
/// prefix on cargo, which xtask sets on every cargo of the gate: `CARGO_ENV` in catalog.rs).
const FROZEN: &str = r#"[
  {"id": "fmt-check", "command": "cargo fmt --all --check", "quick": true, "check": true},
  {"id": "architecture", "command": "cargo nextest run --locked --retries 0 --no-tests fail -p jfl-architecture --test architecture --test rules --test ci_lockstep --test gate_config --test workflow_yaml", "quick": true, "check": true},
  {"id": "clippy", "command": "bash scripts/clippy-lists.sh --check, then cargo clippy --workspace --all-targets --locked -- -D warnings (failing on a `does not refer to a` warning)", "quick": true, "check": true},
  {"id": "nostd", "command": "cargo build --locked -p <each pure crate of architecture.toml> --target x86_64-unknown-none", "quick": false, "check": true},
  {"id": "docs", "command": "bash scripts/adr-index.sh check doc/adr", "quick": false, "check": true},
  {"id": "unit-tests", "command": "cargo nextest run --workspace --locked --retries 0 --no-tests fail --lib --bins", "quick": true, "check": true},
  {"id": "integration-tests", "command": "cargo nextest run --workspace --locked --retries 0 --no-tests fail --test *", "quick": false, "check": true},
  {"id": "doctests", "command": "cargo test --workspace --locked --doc", "quick": false, "check": true},
  {"id": "script-tests", "command": "bash scripts/test-*.sh, each in name order", "quick": true, "check": true},
  {"id": "deny", "command": "cargo deny --locked check", "quick": false, "check": true},
  {"id": "secrets", "command": "bash scripts/secrets-check.sh", "quick": false, "check": true},
  {"id": "workflows", "command": "bash scripts/workflows-check.sh workflows, then bash scripts/workflows-check.sh shell", "quick": false, "check": true}
]
"#;

fn list(args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("list")
        .args(args)
        .output()?)
}

#[test]
fn should_name_exactly_the_stages_just_check_ran_when_the_catalog_is_listed() -> TestResult {
    // given
    let expected: Vec<&str> = CHECK_STAGES.split(' ').collect();

    // when
    let output = list(&[])?;

    // then
    assert!(output.status.success());
    let listed = String::from_utf8(output.stdout)?;
    assert_eq!(listed.lines().collect::<Vec<_>>(), expected);
    Ok(())
}

#[test]
fn should_match_the_frozen_catalog_when_it_is_listed_as_json() -> TestResult {
    // given
    let quick: Vec<&str> = QUICK_STAGES.split(' ').collect();

    // when
    let output = list(&["--json"])?;

    // then
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout)?;
    assert_eq!(json, FROZEN);
    for line in FROZEN.lines().filter(|l| l.contains("\"id\"")) {
        let id = line.split('"').nth(3).unwrap_or_default();
        let marked = line.contains("\"quick\": true");
        assert_eq!(marked, quick.contains(&id), "{id}");
    }
    Ok(())
}
