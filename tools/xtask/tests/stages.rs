//! The stages' negative cases over the fixture workspaces of tools/jfl-architecture/fixtures
//! (FND-02; fnd §3.3, §3.4 rule 2), moved here by XT-01 from
//! tools/jfl-architecture/tests/gate_negative_cases.rs with their names and positive controls: each
//! runs `xtask check-only <stage>` with the fixture as working directory (as `just --working-directory
//! <fixture> <stage>` did) and must fail for the rule's reason; the control shows the fixture
//! builds, so the failure is the rule's. Every case is a nested cargo build, in nextest's
//! `nested-cargo` group (.config/nextest.toml).

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jfl-architecture/fixtures")
        .join(name)
}

/// A fresh target directory per case under cargo's scratch directory for integration tests, so a
/// cached result of an earlier run never answers for this one.
fn fresh_target(case: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("xtask-stages")
        .join(case);
    if dir.try_exists()? {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The variables a gate run may inherit, passed through; everything else, the test runner's own
/// `CARGO_*` and `NEXTEST_*` included, is cleared.
const PASSED_THROUGH: [&str; 7] = [
    "PATH",
    "HOME",
    "RUSTUP_HOME",
    "CARGO_HOME",
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
    "CARGO_HTTP_CAINFO",
];

/// `xtask check-only <stage>` in `workspace`, building into `target`.
fn stage(name: &str, workspace: &Path, target: &Path) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .args(["check-only", name])
        .current_dir(workspace)
        .env_clear()
        .env("CARGO_TARGET_DIR", target);
    for var in PASSED_THROUGH {
        if let Some(value) = std::env::var_os(var) {
            command.env(var, value);
        }
    }
    Ok(command.output()?)
}

#[test]
fn should_fail_the_nostd_stage_when_a_pure_crate_depends_on_a_std_crate() -> TestResult {
    // given
    let workspace = fixture("pure-depends-on-std");

    // when
    let output = stage("nostd", &workspace, &fresh_target("nostd")?)?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "nostd passed:\n{text}");
    assert!(text.contains("can't find crate for `std`"), "{text}");
    Ok(())
}

#[test]
fn should_build_on_the_host_when_a_pure_crate_depends_on_a_std_crate() -> TestResult {
    // given
    let workspace = fixture("pure-depends-on-std");

    // when
    let output = Command::new("cargo")
        .args(["build", "--locked", "-p", "pure"])
        .current_dir(&workspace)
        .env("CARGO_TARGET_DIR", fresh_target("nostd-host")?)
        .output()?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_fail_the_unit_tests_stage_when_a_test_passes_only_on_a_retry() -> TestResult {
    // given: the fixture's own nextest config asks for three retries
    let workspace = fixture("flaky-test");

    // when
    let output = stage("unit-tests", &workspace, &fresh_target("flaky-stage")?)?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "unit-tests passed:\n{text}");
    assert!(text.contains("FAIL"), "{text}");
    Ok(())
}

#[test]
fn should_pass_plain_nextest_when_the_flaky_test_is_retried() -> TestResult {
    // given: the control, nextest without the gate's flags, without the runner's own profile
    // (CI's NEXTEST_PROFILE=ci names a profile the fixture does not have) and without colour
    // (CI's CARGO_TERM_COLOR=always splits "1 flaky" with escape codes)
    let workspace = fixture("flaky-test");

    // when
    let output = Command::new("cargo")
        .args(["nextest", "run", "--locked", "--color", "never"])
        .current_dir(&workspace)
        .env("CARGO_TARGET_DIR", fresh_target("flaky-plain")?)
        .env_remove("NEXTEST_RETRIES")
        .env_remove("NEXTEST_PROFILE")
        .output()?;

    // then
    let text = printed(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 flaky"), "{text}");
    Ok(())
}

#[test]
fn should_fail_the_clippy_stage_when_a_list_names_a_path_that_does_not_exist() -> TestResult {
    // given: a clean crate whose clippy.toml has a mistyped std path without allow-invalid
    let workspace = fixture("typo-in-list");

    // when
    let output = stage("clippy", &workspace, &fresh_target("typo-in-list")?)?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "clippy stage passed:\n{text}");
    assert!(
        text.contains("`std::thread::sleeep` does not refer to a"),
        "{text}"
    );
    assert!(
        text.contains("clippy: a clippy.toml entry names a path that does not exist"),
        "{text}"
    );
    Ok(())
}
