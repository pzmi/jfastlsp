//! The `architecture` stage (fnd §3.2, `just check-only architecture`): every check of
//! `jfl-architecture` over the repository itself, through `cargo metadata --filter-platform` for
//! each target of ADR 0006, and `ARCH-ci-lockstep` over `ci.yml`, the justfile and
//! `doc/required-checks.md` (fnd §7.2). A finding prints as `ARCH-<id>: message (at path[:line])`.

use std::path::Path;

use jfl_architecture::tree::DiskTree;
use jfl_architecture::{Scope, check_repository, ci};

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

    // when
    let findings = ci::check(&DiskTree::new(&root));

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        report.is_empty(),
        "{} finding(s):\n{}",
        report.len(),
        report.join("\n")
    );
}
