//! The architecture test over `cargo metadata`, the clippy lists and the source scan (fnd §3.2,
//! ADR 0030): the half of the dependency law no tool expresses. Role: test only.
//!
//! Every check is a function from its inputs to [`Finding`]s, so the table-driven cases in
//! `tests/rules.rs` feed it synthetic `cargo metadata` JSON and an in-memory [`tree::MemoryTree`],
//! and `tests/architecture.rs` feeds it the repository itself through
//! [`check_repository`]. A finding prints as `ARCH-<id>: message (at path[:line])`.
//!
//! Pinned by: tests/rules.rs (each rule's negative case, the clean baseline, the meta-test),
//! tests/fixture_workspaces.rs (end to end through `cargo metadata`) and tests/architecture.rs
//! (the tree).
//!
//! Rejected: the ported `syn` checker (ADR 0030; a line-oriented scan and the compiler cover what
//! it did, and the rest binds by review), and running `scripts/clippy-lists.sh` as the
//! `ARCH-clippy-lists` check (a generator bug would then agree with itself).

pub mod clippy_lists;
pub mod graph;
pub mod lexer;
pub mod manifests;
pub mod policy;
pub mod scan;
pub mod tree;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cargo_metadata::{Metadata, MetadataCommand};

/// Every rule id this crate can report, in fnd §3.3's order. The meta-test in `tests/rules.rs`
/// compares it with the rows fnd §3.3 assigns to FND-07 and with the negative cases.
pub const RULES: [&str; 25] = [
    "ARCH-policy-schema",
    "ARCH-metadata",
    "ARCH-missing-member",
    "ARCH-not-a-member",
    "ARCH-missing-crate",
    "ARCH-empty-selector",
    "ARCH-duplicate-category",
    "ARCH-policy-cycle",
    "ARCH-forbidden-edge",
    "ARCH-forbidden-dev-edge",
    "ARCH-undeclared-build-dep",
    "ARCH-production-closure",
    "ARCH-independent-oracle",
    "ARCH-layer-isolation",
    "ARCH-unlisted-external",
    "ARCH-external-feature",
    "ARCH-domain-purity",
    "ARCH-clippy-lists",
    "ARCH-lint-config",
    "ARCH-lint-suppression",
    "ARCH-ignored-test",
    "ARCH-test-name",
    "ARCH-test-structure",
    "ARCH-platform-branch",
    "ARCH-hidden-source",
];

/// The targets of ADR 0006: every edge is evaluated through `cargo metadata --filter-platform`
/// for each (fnd §2.2 rule 6).
pub const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];

/// One violation: the rule's id, what is wrong, and where (a path relative to the workspace
/// root, with a line when there is one, or a crate name for a graph finding).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The rule id, `ARCH-...`, one of [`RULES`].
    pub rule: &'static str,
    /// What is wrong, with the full dependency path for a transitive edge.
    pub message: String,
    /// Where: `path[:line]`.
    pub at: String,
}

impl Finding {
    /// A finding of `rule` at `at`.
    pub fn new(rule: &'static str, at: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
            at: at.into(),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} (at {})", self.rule, self.message, self.at)
    }
}

/// Which families of checks to run: the fixture workspaces exercise the graph alone.
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    /// Policy and graph rules over `cargo metadata`.
    pub graph: bool,
    /// Workspace lints, `publish = false`, members on disk.
    pub manifests: bool,
    /// `ARCH-clippy-lists`.
    pub clippy_lists: bool,
    /// The line-oriented source scan.
    pub scan: bool,
}

impl Scope {
    /// Every check: what the `architecture` stage runs on the repository.
    pub const ALL: Self = Self {
        graph: true,
        manifests: true,
        clippy_lists: true,
        scan: true,
    };
    /// The policy and graph checks only.
    pub const GRAPH: Self = Self {
        graph: true,
        manifests: false,
        clippy_lists: false,
        scan: false,
    };
}

/// Runs the checks of `scope` over a workspace: `tree` holds its files (relative paths), and
/// `metadata` one `cargo metadata` result per target name. A finding seen on only some targets
/// names them (fnd §2.2 rule 6).
pub fn check(tree: &dyn tree::Tree, metadata: &[(String, Metadata)], scope: Scope) -> Vec<Finding> {
    let mut findings = Vec::new();
    let policy = match tree.read("architecture.toml") {
        Some(text) => policy::parse(&text),
        None => Err(Finding::new(
            "ARCH-policy-schema",
            "architecture.toml",
            "the policy file is missing",
        )),
    };
    let policy = match policy {
        Ok(policy) => policy,
        Err(finding) => return vec![finding],
    };
    let mut members = BTreeMap::new();
    if scope.graph || scope.manifests || scope.scan {
        let mut per_target: BTreeMap<Finding, BTreeSet<&str>> = BTreeMap::new();
        for (target, data) in metadata {
            let workspace = graph::Workspace::from_metadata(data);
            for finding in graph::check(&policy, &workspace) {
                per_target
                    .entry(finding)
                    .or_default()
                    .insert(target.as_str());
            }
            for member in workspace.members.values() {
                members.insert(member.name.clone(), member.dir.clone());
            }
        }
        if metadata.is_empty() {
            findings.push(Finding::new(
                "ARCH-metadata",
                "Cargo.toml",
                "no `cargo metadata` result to check",
            ));
        }
        if scope.graph {
            findings.extend(per_target.into_iter().map(|(mut finding, targets)| {
                if targets.len() < metadata.len() {
                    let names: Vec<&str> = targets.into_iter().collect();
                    finding.message = format!("{} (on {})", finding.message, names.join(", "));
                }
                finding
            }));
        }
    }
    if scope.manifests {
        findings.extend(manifests::check(tree, &members));
    }
    if scope.clippy_lists {
        findings.extend(clippy_lists::check(tree, &policy, &members));
    }
    if scope.scan {
        findings.extend(scan::check(tree, &policy, &members));
    }
    for problem in tree.problems() {
        findings.push(Finding::new(
            "ARCH-metadata",
            problem.clone(),
            format!("the walk of the tree stopped: {problem}"),
        ));
    }
    findings.sort();
    findings.dedup();
    findings
}

/// Runs `cargo metadata --locked --filter-platform <target>` in `root` for each of [`TARGETS`]
/// and then [`check`] over the files under `root`. A metadata failure is an `ARCH-metadata`
/// finding, never a pass.
pub fn check_repository(root: &Path, scope: Scope) -> Vec<Finding> {
    let mut metadata = Vec::new();
    for target in TARGETS {
        let result = MetadataCommand::new()
            .current_dir(root)
            .other_options(vec![
                "--locked".to_owned(),
                "--filter-platform".to_owned(),
                target.to_owned(),
            ])
            .exec();
        match result {
            Ok(data) => metadata.push((target.to_owned(), data)),
            Err(error) => {
                return vec![Finding::new(
                    "ARCH-metadata",
                    "Cargo.toml",
                    format!("`cargo metadata --filter-platform {target}` failed: {error}"),
                )];
            }
        }
    }
    let mut tree = tree::DiskTree::new(root);
    for (_, data) in &metadata {
        tree = tree.skipping(data.target_directory.as_std_path());
    }
    check(&tree, &metadata, scope)
}

/// Parses `cargo metadata` JSON (for the synthetic cases); a parse error is `ARCH-metadata`.
pub fn parse_metadata(json: &str) -> Result<Metadata, Finding> {
    MetadataCommand::parse(json).map_err(|error| {
        Finding::new(
            "ARCH-metadata",
            "Cargo.toml",
            format!("`cargo metadata` output does not parse: {error}"),
        )
    })
}
