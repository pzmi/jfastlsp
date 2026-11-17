//! The end-to-end negative cases of fnd §3.4 rule 1: each fixture workspace under `fixtures/`
//! (path dependencies only, so it resolves offline) runs the real `cargo metadata` for every
//! target of ADR 0006 and the graph checks, and must fail exactly its rule. `pure-uses-std` is
//! built instead: a `compile_fail` doctest cannot show it, since rustdoc links each doctest as a
//! std binary.
//!
//! Every case runs a nested cargo; `.config/nextest.toml` puts this binary in the `nested-cargo`
//! test group.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use jfl_architecture::{Finding, Scope, check_repository};

type TestResult = Result<(), Box<dyn Error>>;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// The fixture's findings; each must be of `rules`, and one of `rules[0]` at `at`.
fn expect_only(name: &str, rules: &[&str], at: &str) -> Result<Vec<Finding>, Box<dyn Error>> {
    let findings = check_repository(&fixture(name), Scope::GRAPH);
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    let first = rules.first().ok_or("no rule given")?;
    assert!(
        findings.iter().any(|f| f.rule == *first && f.at == at),
        "no {first} at {at}:\n{}",
        report.join("\n")
    );
    assert!(
        findings.iter().all(|f| rules.contains(&f.rule)),
        "unexpected findings:\n{}",
        report.join("\n")
    );
    Ok(findings)
}

#[test]
fn should_fail_forbidden_edge_when_the_domain_depends_on_the_application() -> TestResult {
    // given
    let name = "domain-depends-on-application";

    // when
    let findings = expect_only(name, &["ARCH-forbidden-edge"], "domain/Cargo.toml")?;

    // then
    assert_eq!(findings.len(), 1, "{findings:?}");
    Ok(())
}

#[test]
fn should_fail_layer_isolation_when_an_adapter_depends_on_an_adapter() -> TestResult {
    // given
    let name = "adapter-depends-on-adapter";

    // when
    let findings = expect_only(
        name,
        &["ARCH-layer-isolation", "ARCH-forbidden-edge"],
        "adapter-a/Cargo.toml",
    )?;

    // then
    assert!(
        findings.iter().any(|f| f.message.contains("`adapter-b`")),
        "{findings:?}"
    );
    Ok(())
}

#[test]
fn should_fail_production_closure_when_the_testkit_is_reachable_from_production() -> TestResult {
    // given
    let name = "testkit-in-production-closure";

    // when
    // (the table's own widening is also an ARCH-forbidden-edge: the closure holds regardless)
    let findings = expect_only(
        name,
        &["ARCH-production-closure", "ARCH-forbidden-edge"],
        "app/Cargo.toml",
    )?;

    // then
    assert!(
        findings
            .iter()
            .any(|f| f.message.ends_with("app -> helper -> testkit")),
        "{findings:?}"
    );
    Ok(())
}

#[test]
fn should_fail_independent_oracle_when_the_oracle_imports_the_domain() -> TestResult {
    // given
    let name = "oracle-imports-domain";

    // when
    // (the table's own widening is also an ARCH-forbidden-edge: independence holds regardless)
    let findings = expect_only(
        name,
        &["ARCH-independent-oracle", "ARCH-forbidden-edge"],
        "oracle/Cargo.toml",
    )?;

    // then
    assert!(
        findings
            .iter()
            .any(|f| f.message.contains("oracle -> domain")),
        "{findings:?}"
    );
    Ok(())
}

#[test]
fn should_fail_unlisted_external_when_a_crate_names_an_unlisted_crate() -> TestResult {
    // given
    let name = "unlisted-external";

    // when
    let findings = expect_only(name, &["ARCH-unlisted-external"], "app/Cargo.toml")?;

    // then
    assert!(
        findings.iter().any(|f| f.message.contains("`third-party`")),
        "{findings:?}"
    );
    Ok(())
}

#[test]
fn should_fail_to_compile_when_a_no_std_crate_names_std_fs() -> TestResult {
    // given
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fixture-pure-uses-std");

    // when
    let output = Command::new(env!("CARGO"))
        .args(["build", "--locked", "--offline"])
        .current_dir(fixture("pure-uses-std"))
        .env("CARGO_TARGET_DIR", &target)
        .output()?;

    // then
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a no_std crate naming std::fs compiled:\n{text}"
    );
    assert!(
        text.contains("error[E0433]") && text.contains("`std`"),
        "{text}"
    );
    Ok(())
}
