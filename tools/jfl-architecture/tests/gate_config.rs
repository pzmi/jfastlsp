//! `ARCH-gate-config`'s negative cases (fnd §3.6, FND-08): each case is the repository's own gate
//! files (the justfile, the pin manifest, the pinned scripts, `.config/nextest.toml`, `mise.toml`,
//! `rust-toolchain.toml`) in memory with one small change that must produce the rule at its path
//! with its reason; the unchanged files must produce nothing. Several reproduce the FND-05
//! checkpoint's bypasses (N1, N3, N6).

use std::error::Error;
use std::path::Path;

use jfl_architecture::gate::{self, GATE_FILES, MISE, MISE_LOCK, NEXTEST, PINS, RULE, TOOLCHAIN};
use jfl_architecture::{Finding, ci, tree::MemoryTree};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_file(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )?)
}

/// The repository's `scripts/test-*.sh`, which the pin manifest covers by glob.
fn test_scripts() -> Result<Vec<String>, Box<dyn Error>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with("test-") && name.ends_with(".sh") {
            found.push(format!("scripts/{name}"));
        }
    }
    found.sort();
    Ok(found)
}

/// The gate's files as the repository has them.
fn baseline() -> Result<MemoryTree, Box<dyn Error>> {
    let mut tree = MemoryTree::new();
    let fixed = [ci::JUSTFILE, PINS, NEXTEST, MISE, TOOLCHAIN, MISE_LOCK]
        .into_iter()
        .chain(GATE_FILES)
        .map(str::to_owned);
    for path in fixed.chain(test_scripts()?) {
        tree = tree.with(&path, &repo_file(&path)?);
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
        &format!("{pins}{}  scripts/bench-corpus.sh\n", "0".repeat(64)),
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
        "`scripts/bench-corpus.sh` is pinned but is no part",
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
        "    bash scripts/secrets-check.sh\n    bash scripts/bench-corpus.sh fetch small\n",
    )?;

    // when
    let found = gate::check(&tree);

    // then
    assert_reported(&found, ci::JUSTFILE, "runs `scripts/bench-corpus.sh`");
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
        "channel = \"1.99.1\"",
        "channel = \"1.99.1\"\npath = \"/elsewhere/toolchain\"",
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

#[test]
fn should_fail_gate_config_when_a_tool_reads_an_unlisted_configuration_file() -> TestResult {
    // given: the FND-08 re-verification's F1, configuration files the gate's linters (and mise)
    // read without a command line naming them, at the root, under .github/ and under .config/
    let files = [
        (
            ".shellcheckrc",
            "disable=all\n",
            ".shellcheckrc",
            "not an allowed entry at the root",
        ),
        (
            ".yamllint",
            "rules: {}\n",
            ".yamllint",
            "not an allowed entry at the root",
        ),
        (
            ".mise.toml",
            "[tools]\n",
            ".mise.toml",
            "not an allowed entry at the root",
        ),
        (
            "mise.dev.toml",
            "[tools]\n",
            "mise.dev.toml",
            "not an allowed entry at the root",
        ),
        (
            ".github/actionlint.yaml",
            "paths: {}\n",
            ".github/actionlint.yaml",
            "not an allowed entry of .github/",
        ),
        (
            ".github/pinact.yaml",
            "ignore_actions: []\n",
            ".github/pinact.yaml",
            "not an allowed entry of .github/",
        ),
        (
            ".config/mise/config.toml",
            "[env]\n",
            ".config/mise",
            "not an allowed entry of .config/",
        ),
        (
            "crates/x/rustfmt.toml",
            "ignore = [\"*\"]\n",
            "crates/x/rustfmt.toml",
            "a rustfmt configuration file",
        ),
    ];

    // when
    let mut found = Vec::new();
    for (path, contents, at, reason) in files {
        found.push((gate::check(&baseline()?.with(path, contents)), at, reason));
    }

    // then
    for (findings, at, reason) in found {
        assert_reported(&findings, at, reason);
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_mise_names_a_plugin_backend_or_a_template() -> TestResult {
    // given: a backend whose plugin is code, and a version that is a template
    let cases = [
        (
            "\"asdf:someone/plugin\" = \"1.0.0\"\n",
            "is not `just`, an aqua or a github backend",
        ),
        (
            "\"vfox:someone/plugin\" = \"1.0.0\"\n",
            "is not `just`, an aqua or a github backend",
        ),
        (
            "\"aqua:a/b\" = \"{{exec(command='true')}}\"\n",
            "must be pinned by a plain version string",
        ),
    ];

    // when
    let mut found = Vec::new();
    for (line, reason) in cases {
        let text = format!("{}{line}", repo_file(MISE)?);
        found.push((gate::check(&baseline()?.with(MISE, &text)), reason));
    }

    // then
    for (findings, reason) in found {
        assert_reported(&findings, MISE, reason);
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_mise_lock_leaves_its_shape() -> TestResult {
    // given: an asset from elsewhere, a checksum that is not SHA-256, a key mise.lock does not
    // allow, and an entry for a tool mise.toml does not pin
    let lock = repo_file(MISE_LOCK)?;
    let first_url = lock
        .lines()
        .find(|l| l.starts_with("url = "))
        .ok_or("no url line")?;
    let first_checksum = lock
        .lines()
        .find(|l| l.starts_with("checksum = "))
        .ok_or("no checksum line")?;
    let cases = [
        lock.replacen(
            first_url,
            "url = \"https://example.invalid/tool.tar.gz\"",
            1,
        ),
        lock.replacen(first_checksum, "checksum = \"blake3:00\"", 1),
        lock.replacen(
            "lockfile_version = 3",
            "lockfile_version = 3\n[settings]\nx = 1",
            1,
        ),
        format!("{lock}\n[[tools.\"aqua:a/b\"]]\nversion = \"1.0.0\"\nbackend = \"aqua:a/b\"\n"),
    ];

    // when
    let mut found = Vec::new();
    for text in &cases {
        found.push(gate::check(&baseline()?.with(MISE_LOCK, text)));
    }

    // then
    for findings in found {
        assert_reported(&findings, MISE_LOCK, "");
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_a_test_script_or_a_policy_changes_without_its_pin() -> TestResult {
    // given: the FND-08 re-verification's F4, files that define the gate besides its recipes
    let new_test = baseline()?.with("scripts/test-anything.sh", "exit 0\n");
    let cases = [
        (
            "scripts/test-secrets.sh",
            "set -euo pipefail\n",
            "set -euo pipefail\nexit 0\n",
            "`scripts/test-secrets.sh` changed",
        ),
        (
            ".github/zizmor.yml",
            "\"*\": hash-pin",
            "\"*\": ref-pin",
            "`.github/zizmor.yml` changed",
        ),
        (
            "deny.toml",
            "[advisories]",
            "[advisories]\nignore = [\"RUSTSEC-0000-0000\"]",
            "`deny.toml` changed",
        ),
    ];

    // when
    let new_found = gate::check(&new_test);
    let mut found = Vec::new();
    for (path, from, to, reason) in cases {
        found.push((gate::check(&edited(path, from, to)?), reason));
    }

    // then
    assert_reported(
        &new_found,
        PINS,
        "`scripts/test-anything.sh` is part of the gate but not pinned",
    );
    for (findings, reason) in found {
        assert_reported(&findings, PINS, reason);
    }
    Ok(())
}

#[test]
fn should_fail_gate_config_when_what_decides_landing_changes_without_its_pin() -> TestResult {
    // given: the FND-09 review's G5, the landing script without its tree check, the DCO check
    // and the hooks neutered
    let cases = [
        (
            "scripts/land.sh",
            "    check_tree \"$tree\" \"$sha\"\n",
            "",
            "`scripts/land.sh` changed",
        ),
        (
            "scripts/dco-check.sh",
            "set -euo pipefail\n",
            "set -euo pipefail\nexit 0\n",
            "`scripts/dco-check.sh` changed",
        ),
        (
            ".claude/hooks/stop-gate.sh",
            "set -uo pipefail\n",
            "set -uo pipefail\nexit 0\n",
            "`.claude/hooks/stop-gate.sh` changed",
        ),
        (
            ".claude/hooks/session-start.sh",
            "set -uo pipefail\n",
            "set -uo pipefail\nexit 0\n",
            "`.claude/hooks/session-start.sh` changed",
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
fn should_fail_gate_config_when_a_cargo_dir_or_a_nested_gitleaks_policy_exists() -> TestResult {
    // given: the FND-09 review's G2 (any `.cargo` entry at the root) and G4 (a policy file whose
    // self-allowlist would cover it, deeper down)
    let cargo_dir = baseline()?.with(".cargo/notes.txt", "x\n");
    let nested = baseline()?.with("crates/x/.gitleaks.toml", "title = \"x\"\n");

    // when
    let cargo_found = gate::check(&cargo_dir);
    let nested_found = gate::check(&nested);

    // then
    assert_reported(&cargo_found, ".cargo", "not an allowed entry at the root");
    assert_reported(
        &nested_found,
        "crates/x/.gitleaks.toml",
        "a gitleaks policy",
    );
    Ok(())
}

#[test]
fn should_fail_gate_config_when_a_cargo_or_toolchain_file_differs_only_by_case() -> TestResult {
    // given: the FND-09 review's H6, names a case-insensitive file system (macOS) opens as the
    // ones cargo and rustup read
    let cargo = baseline()?.with("crates/x/.Cargo/Config.toml", "[build]\n");
    let toolchain = baseline()?.with("crates/x/Rust-Toolchain.toml", "[toolchain]\n");

    // when
    let cargo_found = gate::check(&cargo);
    let toolchain_found = gate::check(&toolchain);

    // then
    assert_reported(
        &cargo_found,
        "crates/x/.Cargo/Config.toml",
        "a cargo configuration file",
    );
    assert_reported(
        &toolchain_found,
        "crates/x/Rust-Toolchain.toml",
        "a toolchain file",
    );
    Ok(())
}
