//! The `architecture` stage (fnd §3.2, `just check-only architecture`): every check of
//! `jfl-architecture` over the repository itself, through `cargo metadata --filter-platform` for
//! each target of ADR 0006, `ARCH-ci-lockstep` over `ci.yml`, the justfile, xtask's stage catalog
//! (`bash scripts/xtask.sh list --json`) and `doc/required-checks.md` (fnd §7.2), and
//! `ARCH-gate-config` over the gate's own configuration (fnd §3.6), `ARCH-workflow-yaml` over the decoded YAML under `.github/` (fnd §7.3), and `ARCH-fixtures-lock` over the golden fixtures (ver §4). A
//! finding prints as `ARCH-<id>: message (at path[:line])`.

use std::path::Path;

use jfl_architecture::tree::DiskTree;
use jfl_architecture::{Scope, check_repository, ci, fixtures_lock, gate, workflow_yaml};

#[test]
fn should_find_nothing_when_the_repository_keeps_the_dependency_law() {
    // given
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // when
    let findings = check_repository(&root, Scope::ALL);

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn should_find_nothing_when_ci_yml_the_justfile_and_required_checks_agree() {
    // given
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = ci::xtask_catalog(&root);

    // when
    let findings = match &catalog {
        Ok(catalog) => ci::check(&DiskTree::new(&root), catalog),
        Err(finding) => vec![finding.clone()],
    };

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn should_find_nothing_when_the_gate_configuration_matches_its_pins() {
    // given
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let catalog = ci::xtask_catalog(&root);

    // when
    let findings = match &catalog {
        Ok(catalog) => gate::check(&DiskTree::new(&root), catalog),
        Err(finding) => vec![finding.clone()],
    };

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn should_find_nothing_when_the_github_yaml_decodes_within_its_allowlist() {
    // given
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // when
    let findings = workflow_yaml::check(&DiskTree::new(&root));

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn should_find_nothing_when_every_golden_fixture_matches_fixtures_lock() {
    // given
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // when
    let findings = fixtures_lock::check(&DiskTree::new(&root));

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}
