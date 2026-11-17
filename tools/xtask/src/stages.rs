//! The stages that are more than one call: each runs the commands its justfile recipe ran before
//! XT-01, in the same order, and reads the same files relative to the working directory (a stage
//! run with another working directory, a fixture workspace say, reads that workspace's).

use std::path::Path;

use regex::Regex;

use crate::catalog::cargo;
use crate::process::{Call, Context};

/// What the `clippy` stage stands for.
pub(crate) const CLIPPY: &str = "bash scripts/clippy-lists.sh --check, then cargo clippy --workspace --all-targets --locked -- -D warnings (failing on a `does not refer to a` warning)";

/// What the `nostd` stage stands for.
pub(crate) const NOSTD: &str =
    "cargo build --locked -p <each pure crate of architecture.toml> --target x86_64-unknown-none";

/// What the `script-tests` stage stands for.
pub(crate) const SCRIPT_TESTS: &str = "bash scripts/test-*.sh, each in name order";

/// What the `workflows` stage stands for.
pub(crate) const WORKFLOWS: &str =
    "bash scripts/workflows-check.sh workflows, then bash scripts/workflows-check.sh shell";

/// Clippy with the authority lists (fnd §2.4): the per-crate lists must match the root one first
/// (`scripts/clippy-lists.sh` of the repository root), and a list entry naming a path that does
/// not exist fails the stage (clippy only warns, and `-D warnings` does not reach configuration
/// warnings). Pinned by tools/xtask/tests/stages.rs
/// (`should_fail_the_clippy_stage_when_a_list_names_a_path_that_does_not_exist`).
pub(crate) fn clippy(context: &Context) -> i32 {
    let lists = context.root.join("scripts/clippy-lists.sh");
    let mut check = Call::new("bash", &[]);
    check.args = vec![lists.to_string_lossy().into_owned(), "--check".to_owned()];
    let code = check.run(context);
    if code != 0 {
        return code;
    }
    let mut lint = cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    ]);
    lint.deny_stderr = Some((
        "does not refer to a",
        "clippy: a clippy.toml entry names a path that does not exist (above); fix it or, for a crate absent from the graph, add allow-invalid",
    ));
    lint.run(context)
}

/// The pure crates of the `architecture.toml` in the working directory: every `pure = [...]` line
/// of it, quotes and spaces dropped, split at commas (the recipe's `sed`).
fn pure_crates(policy: &str) -> Vec<String> {
    let Ok(line) = Regex::new(r"^pure[[:space:]]*=[[:space:]]*\[(.*)\][[:space:]]*$") else {
        return Vec::new();
    };
    policy
        .lines()
        .filter_map(|l| line.captures(l))
        .filter_map(|c| c.get(1))
        .flat_map(|m| {
            m.as_str()
                .replace(['"', ' '], "")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Builds every pure crate of the working directory's `architecture.toml` for a target with no
/// std (fnd §2.3, ARCH-domain-purity). Pinned by tools/xtask/tests/stages.rs
/// (`should_fail_the_nostd_stage_when_a_pure_crate_depends_on_a_std_crate`).
pub(crate) fn nostd(context: &Context) -> i32 {
    let policy = match std::fs::read_to_string("architecture.toml") {
        Ok(text) => text,
        Err(err) => {
            eprintln!("nostd: cannot read architecture.toml: {err}");
            return 1;
        }
    };
    let crates = pure_crates(&policy);
    if crates.is_empty() {
        eprintln!("nostd: architecture.toml names no pure crate");
        return 1;
    }
    let mut args = vec!["build".to_owned(), "--locked".to_owned()];
    for name in crates {
        args.push("-p".to_owned());
        args.push(name);
    }
    args.push("--target".to_owned());
    args.push("x86_64-unknown-none".to_owned());
    let mut build = cargo(&[]);
    build.args = args;
    build.run(context)
}

/// The `scripts/test-*.sh` of the working directory, in byte order of their names.
fn test_scripts(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("test-") && n.ends_with(".sh"))
        .map(|n| format!("scripts/{n}"))
        .collect();
    found.sort();
    found
}

/// Runs each `scripts/test-*.sh` of the working directory with bash, stopping at the first
/// failure; none at all fails the stage.
pub(crate) fn script_tests(context: &Context) -> i32 {
    let tests = test_scripts(Path::new("scripts"));
    if tests.is_empty() {
        eprintln!("script-tests: no scripts/test-*.sh");
        return 1;
    }
    for test in tests {
        eprintln!("script-tests: {test}");
        let code = Call::new("bash", &[&test]).run(context);
        if code != 0 {
            return code;
        }
    }
    0
}

/// actionlint with shellcheck, zizmor and pinact over the workflows, then shellcheck over the
/// scripts and the hooks (`scripts/workflows-check.sh`, FND-04).
pub(crate) fn workflows(context: &Context) -> i32 {
    for mode in ["workflows", "shell"] {
        let code = Call::new("bash", &["scripts/workflows-check.sh", mode]).run(context);
        if code != 0 {
            return code;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_every_pure_crate_when_the_policy_lists_them() {
        // given
        let policy = "[roles]\npure = [\"jfl-domain\", \"jfl-protocol\"]\napplication = [\"a\"]\n";

        // when
        let crates = pure_crates(policy);

        // then
        assert_eq!(crates, ["jfl-domain", "jfl-protocol"]);
    }

    #[test]
    fn should_find_no_pure_crate_when_the_list_is_empty_or_absent() {
        // given
        let empty = "pure = []\n";
        let absent = "[roles]\napplication = [\"a\"]\n  pure = [\"indented\"]\n";

        // when
        let found = (pure_crates(empty), pure_crates(absent));

        // then
        assert!(found.0.is_empty() && found.1.is_empty(), "{found:?}");
    }
}
