//! The stage catalog (fnd §9): every stage of `check`, in its order, which of them `quick` runs,
//! and the command each runs. Each stage runs the same tool command its justfile recipe ran
//! before XT-01; `list --json` prints the catalog, which `ARCH-ci-lockstep` and `ARCH-gate-config`
//! read (tools/jfl-architecture/src/ci.rs). A stage enters the catalog in the commit of the task
//! that introduces it; one that does not exist yet is absent, never a skipped row.
//!
//! Started from mayfly's `tools/xtask/src/catalog.rs`, `check.rs` and `quick.rs` (one table
//! instead of a catalog plus two stage lists, since every stage here is a stage of `check`).
//! Pinned by tools/xtask/tests/catalog.rs (the frozen catalog of XT-01) and the unit tests below.

use std::fmt::Write as _;

use crate::conformance;
use crate::formal;
use crate::process::{Call, Stage};
use crate::stages;

/// One row: a stage of `check`, and whether `quick` runs it too.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    /// The stage.
    pub(crate) stage: Stage,
    /// Whether `quick` runs it.
    pub(crate) quick: bool,
}

/// The variables every cargo of the gate runs with, set here rather than taken from the
/// environment or a cargo configuration file: RUSTFLAGS and an empty RUSTC_WRAPPER beat
/// `build.rustflags` and `build.rustc-wrapper` in any `.cargo/config.toml` (fnd §3.6); the
/// preflight still refuses such a file and such variables, so a steered environment fails loudly
/// rather than being overridden silently. The justfile's `cargo` variable, moved.
pub(crate) const CARGO_ENV: [(&str, &str); 4] = [
    ("RUSTFLAGS", "-D warnings"),
    ("RUSTDOCFLAGS", ""),
    ("RUSTC_WRAPPER", ""),
    ("RUSTC_WORKSPACE_WRAPPER", ""),
];

/// `cargo` with `args` and [`CARGO_ENV`].
pub(crate) fn cargo(args: &[&str]) -> Call {
    CARGO_ENV
        .iter()
        .fold(Call::new("cargo", args), |call, (k, v)| call.env(k, v))
}

/// `cargo nextest run` with `args`: each stage passes `--retries 0` and `--no-tests fail` on the
/// command line, which beat any configuration or variable.
fn nextest(args: &[&str]) -> Call {
    let all: Vec<&str> = ["nextest", "run"].iter().chain(args).copied().collect();
    cargo(&all)
}

/// The catalog, in the order `check` runs it (fnd §9's order).
pub(crate) fn rows() -> Vec<Row> {
    let row = |stage, quick| Row { stage, quick };
    vec![
        // Formatting, checked, never applied.
        row(
            Stage::call("fmt-check", cargo(&["fmt", "--all", "--check"])),
            true,
        ),
        // The architecture test (fnd §3.2, FND-07): the dependency law, the clippy lists and the
        // source scan, the checker's negative cases and meta-test, ARCH-ci-lockstep,
        // ARCH-gate-config and ARCH-workflow-yaml. The fixture workspaces run in
        // `integration-tests`.
        row(
            Stage::call(
                "architecture",
                nextest(&[
                    "--locked",
                    "--retries",
                    "0",
                    "--no-tests",
                    "fail",
                    "-p",
                    "jfl-architecture",
                    "--test",
                    "architecture",
                    "--test",
                    "rules",
                    "--test",
                    "ci_lockstep",
                    "--test",
                    "gate_config",
                    "--test",
                    "workflow_yaml",
                ]),
            )
            .needs("cargo-nextest"),
            true,
        ),
        // Clippy with the authority lists (fnd §2.4).
        row(
            Stage::in_process("clippy", stages::CLIPPY, stages::clippy),
            true,
        ),
        // Every pure crate of architecture.toml for a target with no std (fnd §2.3).
        row(
            Stage::in_process("nostd", stages::NOSTD, stages::nostd),
            false,
        ),
        // The ADR index agrees with the records (FND-03).
        row(
            Stage::call(
                "docs",
                Call::new("bash", &["scripts/adr-index.sh", "check", "doc/adr"]),
            ),
            false,
        ),
        // Unit and property tests: the lib and bin targets.
        row(
            Stage::call(
                "unit-tests",
                nextest(&[
                    "--workspace",
                    "--locked",
                    "--retries",
                    "0",
                    "--no-tests",
                    "fail",
                    "--lib",
                    "--bins",
                ]),
            )
            .needs("cargo-nextest"),
            true,
        ),
        // Integration, component and contract tests: every tests/ target but `jfl lsp`'s (its
        // transcripts and live sessions), which `lsp-conformance` runs (LSP-01, LSP-02), so each
        // runs in one stage.
        row(
            Stage::call(
                "integration-tests",
                nextest(&[
                    "--workspace",
                    "--locked",
                    "--retries",
                    "0",
                    "--no-tests",
                    "fail",
                    "--test",
                    "*",
                    "-E",
                    "not (binary_id(jfl-cli::lsp_transcripts) | binary_id(jfl-cli::lsp_daemon))",
                ]),
            )
            .needs("cargo-nextest"),
            false,
        ),
        // Doctests (nextest does not run them).
        row(
            Stage::call(
                "doctests",
                cargo(&["test", "--workspace", "--locked", "--doc"]),
            ),
            false,
        ),
        // The scripts' own offline self-tests (scripts/test-*.sh).
        row(
            Stage::in_process("script-tests", stages::SCRIPT_TESTS, stages::script_tests),
            true,
        ),
        // The LSP conformance level (ver §2, LSP-01, LSP-02): the transcripts of fixtures/lsp/ and
        // the live sessions against the built `jfl lsp` over stdio.
        row(
            Stage::call(
                "lsp-conformance",
                nextest(&[
                    "--locked",
                    "--retries",
                    "0",
                    "--no-tests",
                    "fail",
                    "-p",
                    "jfl-cli",
                    "--test",
                    "lsp_transcripts",
                    "--test",
                    "lsp_daemon",
                ]),
            )
            .needs("cargo-nextest"),
            false,
        ),
        // Every Quint model of the MODELS registry typechecks, and none is unregistered (FORMAL-01).
        row(
            Stage::in_process("formal-typecheck", formal::TYPECHECK, formal::typecheck),
            false,
        ),
        // Every model's Quint tests pass, on the TypeScript backend; a model without one fails.
        row(
            Stage::in_process("formal-test", formal::TEST, formal::test),
            false,
        ),
        // Every committed regression trace regenerates byte for byte at its seed (FORMAL-02).
        row(
            Stage::in_process(
                "formal-freshness",
                conformance::FRESHNESS,
                conformance::freshness,
            ),
            false,
        ),
        // Seeded traces of every model, then the committed ones, replayed through the drivers
        // against the real code, with each driver's negative control (FORMAL-02, ver §6.2).
        row(
            Stage::in_process(
                "conformance-replay",
                conformance::REPLAY,
                conformance::replay_stage,
            ),
            false,
        ),
        // Advisories, licences, sources and bans (fnd §2.5, §7.4).
        row(
            Stage::call("deny", cargo(&["deny", "--locked", "check"])).needs("cargo-deny"),
            false,
        ),
        // gitleaks over the full history (FND-04, FND-08, fnd §7.4).
        row(
            Stage::call("secrets", Call::new("bash", &["scripts/secrets-check.sh"])),
            false,
        ),
        // actionlint, zizmor, pinact and shellcheck, offline (FND-04, fnd §7.3).
        row(
            Stage::in_process("workflows", stages::WORKFLOWS, stages::workflows),
            false,
        ),
    ]
}

/// The stages of `check`, in order.
pub(crate) fn check() -> Vec<Stage> {
    rows().into_iter().map(|r| r.stage).collect()
}

/// The stages of `quick`, in `check`'s order.
pub(crate) fn quick() -> Vec<Stage> {
    rows()
        .into_iter()
        .filter(|r| r.quick)
        .map(|r| r.stage)
        .collect()
}

/// The stages named by `names`, each argument split at whitespace (as the justfile's `check-only`
/// did, so `check-only "a b"` is `check-only a b`), in the order given, each once (just ran a
/// recipe once per invocation). An unknown name is an error naming it and the valid ones; a name
/// is only ever compared, never run.
pub(crate) fn select(names: &[String]) -> Result<Vec<Stage>, String> {
    let all = check();
    let mut chosen = Vec::new();
    for name in names.iter().flat_map(|arg| arg.split_whitespace()) {
        match all.iter().find(|s| s.name == name) {
            Some(stage) if chosen.iter().any(|c: &Stage| c.name == stage.name) => {}
            Some(stage) => chosen.push(stage.clone()),
            None => {
                let valid: Vec<&str> = all.iter().map(|s| s.name).collect();
                return Err(format!(
                    "check-only: unknown stage '{name}'; valid stages: {}",
                    valid.join(" ")
                ));
            }
        }
    }
    if chosen.is_empty() {
        return Err("check-only: name at least one stage".to_owned());
    }
    Ok(chosen)
}

/// `list`: one stage id per line, in order.
pub(crate) fn to_lines(rows: &[Row]) -> String {
    rows.iter().fold(String::new(), |mut out, r| {
        let _ = writeln!(out, "{}", r.stage.name);
        out
    })
}

/// `list --json`: a JSON array, one object per stage in order, with its id, the command it runs,
/// and whether `quick` and `check` run it.
pub(crate) fn to_json(rows: &[Row]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "  {{\"id\": {}, \"command\": {}, \"quick\": {}, \"check\": true}}",
                json_str(r.stage.name),
                json_str(&r.stage.display()),
                r.quick
            )
        })
        .collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A context for the unit tests and `list`, which run nothing.
#[cfg(test)]
pub(crate) fn idle() -> crate::process::Context {
    crate::process::Context {
        root: std::path::PathBuf::from("."),
        removed: Vec::new(),
        path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn should_have_unique_ids_when_the_catalog_is_listed() {
        // given
        let rows = rows();

        // when
        let ids: BTreeSet<&str> = rows.iter().map(|r| r.stage.name).collect();

        // then
        assert_eq!(ids.len(), rows.len());
        assert!(idle().removed.is_empty());
    }

    #[test]
    fn should_keep_check_s_order_when_quick_is_built() {
        // given
        let check: Vec<&str> = check().iter().map(|s| s.name).collect();

        // when
        let quick: Vec<&str> = quick().iter().map(|s| s.name).collect();

        // then
        assert_eq!(
            quick,
            [
                "fmt-check",
                "architecture",
                "clippy",
                "unit-tests",
                "script-tests"
            ]
        );
        let positions: Vec<usize> = quick
            .iter()
            .filter_map(|q| check.iter().position(|c| c == q))
            .collect();
        assert!(
            positions.windows(2).all(|w| w.first() < w.get(1)),
            "{positions:?}"
        );
    }

    #[test]
    fn should_name_the_valid_stages_when_check_only_gets_an_unknown_one() {
        // given
        let names = vec!["fmt-check".to_owned(), "nope".to_owned()];

        // when
        let got = select(&names);

        // then
        let err = got.err().unwrap_or_default();
        assert!(err.contains("unknown stage 'nope'"), "{err}");
        assert!(
            err.contains("valid stages: fmt-check architecture clippy nostd"),
            "{err}"
        );
    }

    #[test]
    fn should_split_each_argument_when_check_only_gets_a_list_in_one_word() {
        // given
        let names = vec!["docs fmt-check".to_owned(), "nostd".to_owned()];

        // when
        let got = select(&names).unwrap_or_default();

        // then
        let ids: Vec<&str> = got.iter().map(|s| s.name).collect();
        assert_eq!(ids, ["docs", "fmt-check", "nostd"]);
        assert!(select(&[]).is_err());
    }

    #[test]
    fn should_escape_quotes_and_control_characters_when_rendering_json() {
        // given
        let text = "say \"hi\"\\\n";

        // when
        let json = json_str(text);

        // then
        assert_eq!(json, r#""say \"hi\"\\\u000a""#);
    }
}
