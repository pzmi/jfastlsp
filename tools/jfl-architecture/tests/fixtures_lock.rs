//! `ARCH-fixtures-lock`'s cases (ver §4, IDX-07): the repository's own fixtures and lock in
//! memory, with one change each that the rule must name. The repository itself is checked by
//! `tests/architecture.rs` (the `architecture` stage).

use std::error::Error;
use std::path::Path;

use jfl_architecture::Finding;
use jfl_architecture::fixtures_lock::{self, LOCK, RULE};
use jfl_architecture::tree::{DiskTree, MemoryTree, Tree};

type TestResult = Result<(), Box<dyn Error>>;

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The repository's locked fixtures and its lock, in memory.
fn baseline() -> Result<MemoryTree, Box<dyn Error>> {
    let disk = DiskTree::new(&root());
    let mut tree = MemoryTree::new().with(LOCK, &disk.read(LOCK).ok_or("no fixtures.lock")?);
    for path in fixtures_lock::locked_files(&disk) {
        let text = disk
            .read(&path)
            .ok_or_else(|| format!("{path} is not text"))?;
        tree = tree.with(&path, &text);
    }
    Ok(tree)
}

fn assert_reported(findings: &[Finding], reason: &str) {
    let got: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        findings
            .iter()
            .any(|f| f.rule == RULE && f.message.contains(reason)),
        "expected {RULE} with `{reason}`; got:\n  {}",
        got.join("\n  ")
    );
}

#[test]
fn should_find_nothing_when_the_fixtures_are_the_repository_s_own() -> TestResult {
    // given
    let tree = baseline()?;
    // when
    let findings = fixtures_lock::check(&tree);
    // then
    assert_eq!(findings, Vec::new());
    assert!(fixtures_lock::locked_files(&tree).len() > 30);
    Ok(())
}

#[test]
fn should_fail_fixtures_lock_when_a_golden_is_edited_and_the_lock_is_stale() -> TestResult {
    // given: a summary golden rewritten (one reference dropped), its lock line unchanged
    let path = "fixtures/java/summaries/orchard/syntax/References.java.json";
    let golden = baseline()?.read(path).ok_or("no golden")?;
    let edited = golden.replacen("\"shape\":\"call\"", "\"shape\":\"name\"", 1);
    assert_ne!(golden, edited);
    let tree = baseline()?.with(path, &edited);
    // when
    let findings = fixtures_lock::check(&tree);
    // then
    assert_reported(&findings, &format!("`{path}` changed"));
    Ok(())
}

#[test]
fn should_fail_fixtures_lock_when_a_fixture_is_added_or_removed_without_its_line() -> TestResult {
    // given
    let added = baseline()?.with("fixtures/java/syntax/New.java", "class New {}\n");
    let removed = baseline()?.without("fixtures/java/syntax/Locals.java");
    // when
    let unlisted = fixtures_lock::check(&added);
    let stale = fixtures_lock::check(&removed);
    // then
    assert_reported(&unlisted, "`fixtures/java/syntax/New.java` is not locked");
    assert_reported(
        &stale,
        "`fixtures/java/syntax/Locals.java` is locked but is not a fixture",
    );
    Ok(())
}

#[test]
fn should_fail_fixtures_lock_when_the_lock_is_out_of_order() -> TestResult {
    // given: the first two entries swapped
    let tree = baseline()?;
    let lock = tree.read(LOCK).ok_or("no lock")?;
    let mut lines: Vec<&str> = lock.lines().collect();
    let first = lines
        .iter()
        .position(|l| !l.starts_with('#'))
        .ok_or("empty lock")?;
    lines.swap(first, first + 1);
    let tree = tree.with(LOCK, &(lines.join("\n") + "\n"));
    // when
    let findings = fixtures_lock::check(&tree);
    // then
    assert_reported(&findings, "out of order");
    Ok(())
}

#[test]
fn should_write_what_the_rule_accepts_when_the_lock_is_regenerated() -> TestResult {
    // given
    let tree = baseline()?;
    // when
    let written = fixtures_lock::expected(&tree).map_err(|f| f.to_string())?;
    // then
    let rewritten = tree.with(LOCK, &written);
    assert_eq!(fixtures_lock::check(&rewritten), Vec::new());
    Ok(())
}
