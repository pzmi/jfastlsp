//! `ARCH-gate-config`'s negative cases (fnd §3.6, FND-08): each case is the repository's own gate
//! files (the justfile, the pin manifest, the pinned scripts, `.config/nextest.toml`, `mise.toml`,
//! `rust-toolchain.toml`) in memory with one small change that must produce the rule at its path
//! with its reason; the unchanged files must produce nothing. Several reproduce the FND-05
//! checkpoint's bypasses (N1, N3, N6).

use std::error::Error;
use std::path::Path;

use jfl_architecture::gate::{self, GATE_FILES, MISE, NEXTEST, PINS, RULE, TOOLCHAIN};
use jfl_architecture::{Finding, ci, tree::MemoryTree};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_file(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )?)
}

/// The gate's files as the repository has them.
fn baseline() -> Result<MemoryTree, Box<dyn Error>> {
    let mut tree = MemoryTree::new();
    for path in [ci::JUSTFILE, PINS, NEXTEST, MISE, TOOLCHAIN, "mise.lock"]
        .into_iter()
        .chain(GATE_FILES)
    {
        tree = tree.with(path, &repo_file(path)?);
    }
    Ok(tree)
}

/// The baseline with the first `from` in `path` replaced by `to`; an absent `from` is an error,
/// so a case can never pass by changing nothing.
fn edited(path: &str, from: &str, to: &str) -> Result<MemoryTree, Box<dyn Error>> {
    let text = repo_file(path)?;
    if !text.contains(from) {
        return Err(format!("{path} has no `{from}`").into());
    }
    Ok(baseline()?.with(path, &text.replacen(from, to, 1)))
}

/// Fails unless a finding of the rule is at `at` and its message contains `reason`.
fn assert_reported(findings: &[Finding], at: &str, reason: &str) {
    let hit = findings
        .iter()
        .any(|f| f.rule == RULE && f.at == at && f.message.contains(reason));
    let got: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        hit,
        "expected {RULE} at {at} with `{reason}`; got:\n  {}",
        got.join("\n  ")
    );
}

#[test]
fn should_find_nothing_when_the_gate_files_are_the_repository_s_own() -> TestResult {
    // given
    let tree = baseline()?;

    // when
    let found = gate::check(&tree);

    // then
    let report: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    Ok(())
}

#[test]
fn should_fail_gate_config_when_a_stage_recipe_changes_without_its_pin() -> TestResult {
    // given: the first review's probe, `check` running the quick stages only
    let tree = edited(
        ci::JUSTFILE,
        "check: (check-only check_stages)",
        "check: (check-only quick_stages)",
    )?;

    // when
    let found = gate::check(&tree);

    // then
    assert_reported(&found, PINS, "`justfile::check` changed");
    Ok(())
}

#[test]
fn should_fail_gate_config_when_any_pinned_part_of_the_gate_changes() -> TestResult {
    // given: a stage body, the preflight, a top-level variable and a pinned script, each neutered
    let cases = [
        (
            ci::JUSTFILE,
            "--retries 0 --no-tests fail --lib --bins",
            "--retries 0 --no-tests pass --lib --bins",
            "`justfile::unit-tests` changed",
        ),
        (
            ci::JUSTFILE,
            "    refused=()\n",
            "    refused=()\n    exit 0\n",
            "`justfile::_preflight` changed",
        ),
        (
            ci::JUSTFILE,
            "RUSTFLAGS='-D warnings'",
            "RUSTFLAGS='--cap-lints=allow'",
            "`justfile::(top-level)` changed",
        ),
        (
            "scripts/secrets-check.sh",
            "set -euo pipefail\n",
            "set -euo pipefail\nexit 0\n",
            "`scripts/secrets-check.sh` changed",
        ),
    ];

    // when
    let mut found = Vec::new();
    for (path, from, to, reason) in cases {
        found.push((gate::check(&edited(path, from, to)?), reason));
    }

    // then
    for (findings, reason) in found {
        assert_reported(&findings, PINS, reason);
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_the_manifest_lists_too_much_or_too_little() -> TestResult {
    // given
    let pins = repo_file(PINS)?;
    let extra = baseline()?.with(
        PINS,
        &format!("{pins}{}  scripts/land.sh\n", "0".repeat(64)),
    );
    let first = pins
        .lines()
        .find(|l| l.contains("justfile::check-only"))
        .ok_or("no check-only pin")?;
    let missing = baseline()?.with(PINS, &pins.replacen(&format!("{first}\n"), "", 1));
    let gone = baseline()?.without(PINS);

    // when
    let extra_found = gate::check(&extra);
    let missing_found = gate::check(&missing);
    let gone_found = gate::check(&gone);

    // then
    assert_reported(
        &extra_found,
        PINS,
        "`scripts/land.sh` is pinned but is no part",
    );
    assert_reported(
        &missing_found,
        PINS,
        "`justfile::check-only` is part of the gate but not pinned",
    );
    assert_reported(&gone_found, PINS, "the pin manifest is missing");
    Ok(())
}

#[test]
fn should_fail_gate_config_when_a_gate_recipe_runs_an_unpinned_script() -> TestResult {
    // given
    let tree = edited(
        ci::JUSTFILE,
        "    bash scripts/secrets-check.sh\n",
        "    bash scripts/secrets-check.sh\n    bash scripts/land.sh --no-push\n",
    )?;

    // when
    let found = gate::check(&tree);

    // then
    assert_reported(&found, ci::JUSTFILE, "runs `scripts/land.sh`");
    Ok(())
}

#[test]
fn should_fail_gate_config_when_nextest_toml_leaves_its_shape() -> TestResult {
    // given: the checkpoint's N3 (a default-filter skipping the failing test) and its relatives
    let cases = [
        (
            "[profile.default]\n",
            "[profile.default]\ndefault-filter = \"not test(/four/)\"\n",
        ),
        ("[profile.ci]\nretries = 0\n", "[profile.ci]\nretries = 2\n"),
        (
            "[profile.serial]\n",
            "[profile.lenient]\nretries = 0\n\n[profile.serial]\n",
        ),
        (
            "test-group = 'daemon-or-jvm'\n",
            "test-group = 'daemon-or-jvm'\nretries = 3\n",
        ),
        (
            "[test-groups]\n",
            "[scripts.setup.evil]\ncommand = \"true\"\n\n[test-groups]\n",
        ),
    ];

    // when
    let mut found = Vec::new();
    for (from, to) in cases {
        found.push(gate::check(&edited(NEXTEST, from, to)?));
    }

    // then
    for findings in found {
        assert_reported(&findings, NEXTEST, "");
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_mise_toml_holds_more_than_tools() -> TestResult {
    // given: the checkpoint's N6, an [env] that reaches the tools through their shims
    let cases = [
        "\n[env]\nNEXTEST_PROFILE = \"evil\"\n",
        "\n[tasks.x]\nrun = \"true\"\n",
        "\n[settings]\nexperimental = true\n",
    ];

    // when
    let mut found = Vec::new();
    for extra in cases {
        let text = format!("{}{extra}", repo_file(MISE)?);
        found.push(gate::check(&baseline()?.with(MISE, &text)));
    }

    // then
    for findings in found {
        assert_reported(&findings, MISE, "mise.toml holds only [tools]");
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_a_toolchain_file_leaves_its_shape() -> TestResult {
    // given
    let path = edited(
        TOOLCHAIN,
        "channel = \"1.97.0\"",
        "channel = \"1.97.0\"\npath = \"/elsewhere/toolchain\"",
    )?;
    let nested = baseline()?.with(
        "crates/x/rust-toolchain.toml",
        "[toolchain]\nchannel = \"nightly\"\n",
    );

    // when
    let path_found = gate::check(&path);
    let nested_found = gate::check(&nested);

    // then
    assert_reported(&path_found, TOOLCHAIN, "`toolchain.path` is not allowed");
    assert_reported(
        &nested_found,
        "crates/x/rust-toolchain.toml",
        "a toolchain file",
    );
    Ok(())
}

#[test]
fn should_fail_gate_config_when_the_tree_holds_a_cargo_or_mise_config() -> TestResult {
    // given: the checkpoint's N1 (an escaped runner) and other configuration mise or cargo loads
    let files = [
        (
            ".cargo/config.toml",
            "[\"t\\u0061rget\".x86_64-unknown-linux-gnu]\n\"r\\u0075nner\" = \"true\"\n",
            "a cargo configuration file",
        ),
        (
            "tools/x/.cargo/config",
            "[net]\nretry = 2\n",
            "a cargo configuration file",
        ),
        ("mise.local.toml", "[tools]\n", "a mise configuration file"),
        (
            ".config/mise/conf.d/x.toml",
            "[env]\n",
            "a mise configuration file",
        ),
        (
            ".tool-versions",
            "just 1.59.0\n",
            "a mise configuration file",
        ),
    ];

    // when
    let mut found = Vec::new();
    for (path, contents, reason) in files {
        found.push((gate::check(&baseline()?.with(path, contents)), path, reason));
    }

    // then
    for (findings, path, reason) in found {
        assert_reported(&findings, path, reason);
    }
    Ok(())
}
