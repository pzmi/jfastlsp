//! The negative cases of the gate's tool-enforced rules (FND-02; fnd §3.3, §3.4 rule 2): each
//! fixture workspace under `fixtures/` must fail the tool that enforces its rule, and each has a
//! positive control showing that the failure is the rule's and not a broken fixture. The `just`
//! cases run the repository's own `justfile` on a child process, so no test steers its own
//! environment; since XT-01 its gate recipes delegate to xtask (scripts/xtask.sh), and the cases
//! that need a scratch repository, a scratch CARGO_HOME or a stage over a fixture workspace moved
//! to tools/xtask/tests/ (preflight.rs, stages.rs) with their names. FND-07 builds the rest of
//! `jfl-architecture` around these.
//!
//! Every case runs a nested cargo, clippy, cargo-deny or just; `.config/nextest.toml` puts them in
//! the `nested-cargo` test group. `just` and `cargo-deny` come from PATH, which the justfile's
//! stages extend with mise's shims: a missing tool fails the case, never skips it (fnd §8).

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// A fresh target directory per case under cargo's scratch directory for integration tests, so
/// a cached result of an earlier run never answers for this one.
fn fresh_target(case: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate-negative-cases")
        .join(case);
    if dir.try_exists()? {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// What a child printed, both streams, for assertions and failure messages.
fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// `cargo clippy` over a fixture with the clippy configuration of `conf_dir`, as the gate runs it.
fn clippy(fixture_name: &str, conf_dir: &Path, case: &str) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new("cargo")
        .args([
            "clippy",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(fixture(fixture_name))
        .env("CARGO_TARGET_DIR", fresh_target(case)?)
        .env("CLIPPY_CONF_DIR", conf_dir)
        .output()?)
}

/// The variables a gate run may inherit (the preflight's allowlist), passed through to a child
/// `just`; everything else, the test runner's own `CARGO_*` and `NEXTEST_*` included, is cleared,
/// so a case sets exactly the steering it tests. CARGO_TARGET_DIR is passed so scripts/xtask.sh
/// finds the xtask the gate built rather than building one from scratch.
const PASSED_THROUGH: [&str; 8] = [
    "PATH",
    "HOME",
    "RUSTUP_HOME",
    "CARGO_HOME",
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
    "CARGO_HTTP_CAINFO",
    "CARGO_TARGET_DIR",
];

/// The repository's justfile, run on a child process with the given working directory and a
/// cleared environment (plus [`PASSED_THROUGH`]).
fn just(working_dir: &Path) -> Command {
    let mut command = Command::new("just");
    command
        .arg("--justfile")
        .arg(repo_root().join("justfile"))
        .arg("--working-directory")
        .arg(working_dir)
        .env_clear();
    for name in PASSED_THROUGH {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

#[test]
fn should_fail_clippy_when_a_crate_without_the_process_grant_names_command() -> TestResult {
    // given
    let canonical = repo_root();

    // when
    let output = clippy("process-without-grant", &canonical, "process")?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "clippy passed:\n{text}");
    assert!(
        text.contains("disallowed type `std::process::Command`"),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_pass_clippy_when_the_crate_holds_the_process_grant() -> TestResult {
    // given: jfl-architecture's own list, which lifts the process grant
    let granted = Path::new(env!("CARGO_MANIFEST_DIR"));

    // when
    let output = clippy("process-without-grant", granted, "process-control")?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_fail_clippy_when_a_test_sleeps_outside_poll() -> TestResult {
    // given
    let canonical = repo_root();

    // when
    let output = clippy("sleep-in-test", &canonical, "sleep")?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "clippy passed:\n{text}");
    for path in [
        "std::thread::sleep",
        "std::sync::mpsc::Receiver::recv_timeout",
        "std::sync::Condvar::wait_timeout",
    ] {
        assert!(
            text.contains(&format!("disallowed method `{path}`")),
            "{path}:\n{text}"
        );
    }
    Ok(())
}

#[test]
fn should_pass_clippy_when_the_sleeping_crate_holds_the_sleep_grant() -> TestResult {
    // given: jfl-testkit's list, which lifts the sleep grant
    let granted = repo_root().join("crates/jfl-testkit");

    // when
    let output = clippy("sleep-in-test", &granted, "sleep-control")?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_fail_deny_bans_when_a_path_crate_is_wrapped_to_the_wrong_parent() -> TestResult {
    // given
    let workspace = fixture("wrapper-wrong-parent");

    // when
    let output = Command::new("cargo")
        .args(["deny", "--locked", "check", "bans"])
        .current_dir(&workspace)
        .output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "cargo deny passed:\n{text}");
    assert!(
        text.contains("direct parent 'wrong-parent = 0.0.0' of banned crate 'wrapped = 0.0.0'"),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_pass_deny_bans_when_every_parent_is_a_wrapper() -> TestResult {
    // given
    let workspace = fixture("wrapper-wrong-parent");

    // when
    let output = Command::new("cargo")
        .args([
            "deny",
            "--locked",
            "--config",
            "deny-control.toml",
            "check",
            "bans",
        ])
        .current_dir(&workspace)
        .output()?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_rustc_wrapper_is_set() -> TestResult {
    // given
    let mut preflight = just(&repo_root());
    preflight.arg("_preflight").env("RUSTC_WRAPPER", "sccache");

    // when
    let output = preflight.output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    assert!(text.contains("RUSTC_WRAPPER"), "{text}");
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_any_steering_variable_is_set() -> TestResult {
    // given: each variable alone, as a first review found them accepted
    let steering = [
        ("CLIPPY_CONF_DIR", "/elsewhere"),
        ("CLIPPY_ARGS", "--cap-lints=allow"),
        ("RUSTFLAGS", "--cap-lints=allow"),
        ("RUSTDOCFLAGS", "--cap-lints=allow"),
        ("RUSTC_WORKSPACE_WRAPPER", "wrapper"),
        ("RUSTC_BOOTSTRAP", "1"),
        ("RUSTC", "/elsewhere/rustc"),
        ("RUSTUP_TOOLCHAIN", "nightly"),
        ("CARGO", "/elsewhere/cargo"),
        ("CARGO_PROFILE_DEV_OVERFLOW_CHECKS", "false"),
        ("CARGO_BUILD_TARGET", "x86_64-unknown-none"),
        ("CARGO_BUILD_RUSTFLAGS", "--cap-lints=allow"),
        ("CARGO_ENCODED_RUSTFLAGS", "--cap-lints=allow"),
        (
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
            "/elsewhere/ld",
        ),
        ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER", "true"),
        ("CARGO_ALIAS_CLIPPY", "check"),
        ("CARGO_UNSTABLE_BUILD_STD", "std"),
        ("NEXTEST_RETRIES", "3"),
        ("NEXTEST_NO_TESTS", "pass"),
        ("NEXTEST_PROFILE", "lenient"),
        // FND-02's review, LOW: a lookalike toolchain, cargo's internal channel override, and
        // proptest's knobs (fnd §9: never inline PROPTEST_*).
        ("RUSTUP_TOOLCHAIN", "1.99.1-evil"),
        ("__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS", "nightly"),
        ("PROPTEST_CASES", "1"),
    ];
    let mut accepted = Vec::new();

    // when
    for (name, value) in steering {
        let output = just(&repo_root())
            .arg("_preflight")
            .env(name, value)
            .output()?;
        if output.status.success() || !printed(&output).contains(name) {
            accepted.push(name);
        }
    }

    // then
    assert!(accepted.is_empty(), "preflight accepted {accepted:?}");
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_an_ancestor_cargo_config_sets_rustflags() -> TestResult {
    // given: the reviewer's bypass, `--cap-lints=allow` from a config file above the tree
    let ancestor = fresh_target("ancestor-config")?;
    std::fs::create_dir_all(ancestor.join(".cargo"))?;
    std::fs::write(
        ancestor.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cap-lints=allow\"]\n",
    )?;
    let working = ancestor.join("work");
    std::fs::create_dir_all(&working)?;

    // when
    let output = just(&working).arg("_preflight").output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    assert!(text.contains(".cargo/config.toml"), "{text}");
    Ok(())
}

#[test]
fn should_pass_preflight_when_nothing_steers_the_build() -> TestResult {
    // given: the allowlisted values only
    let mut preflight = just(&repo_root());
    preflight
        .arg("_preflight")
        .env("RUSTFLAGS", "-D warnings")
        .env("NEXTEST_PROFILE", "ci")
        .env("RUSTUP_TOOLCHAIN", "1.99.1-x86_64-unknown-linux-gnu");

    // when
    let output = preflight.output()?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_fail_clippy_when_a_crate_without_grants_uses_the_paths_a_review_found_open() -> TestResult
{
    // given
    let canonical = repo_root();

    // when
    let output = clippy("authority-gaps", &canonical, "authority-gaps")?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "clippy passed:\n{text}");
    for path in [
        "std::thread::scope",
        "std::thread::Scope::spawn",
        "std::thread::park_timeout",
        "std::time::SystemTime::elapsed",
        "std::env::home_dir",
        "std::env::current_dir",
        "std::env::temp_dir",
    ] {
        assert!(
            text.contains(&format!("disallowed method `{path}`")),
            "{path}:\n{text}"
        );
    }
    Ok(())
}

#[test]
fn should_fail_deny_bans_when_a_workspace_crate_has_a_build_script() -> TestResult {
    // given: the repository's own deny.toml over a workspace crate with a build.rs
    let workspace = fixture("build-script-in-workspace");

    // when
    let output = Command::new("cargo")
        .args(["deny", "--locked", "--config"])
        .arg(repo_root().join("deny.toml"))
        .args(["check", "bans"])
        .current_dir(&workspace)
        .output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "cargo deny passed:\n{text}");
    assert!(text.contains("build-script-not-allowed"), "{text}");
    Ok(())
}

#[test]
fn should_fail_deny_bans_when_a_bypassed_build_script_changes() -> TestResult {
    // given: the repository's deny.toml with one reviewed checksum no longer matching
    let policy = std::fs::read_to_string(repo_root().join("deny.toml"))?;
    let reviewed =
        "build-script = \"e2ea6a7a82f5404c3e28cb055883c737ee7903367a10206fbb47b75c009f0eb3\"";
    assert!(policy.contains(reviewed), "serde's reviewed checksum moved");
    let changed = fresh_target("deny-build-script-checksum")?.join("deny.toml");
    std::fs::write(
        &changed,
        policy.replace(reviewed, &format!("build-script = \"{}\"", "0".repeat(64))),
    )?;

    // when
    let output = Command::new("cargo")
        .args(["deny", "--locked", "--config"])
        .arg(&changed)
        .args(["check", "bans"])
        .current_dir(repo_root())
        .output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "cargo deny passed:\n{text}");
    assert!(text.contains("crate 'serde = "), "{text}");
    Ok(())
}

#[test]
fn should_not_run_a_stage_name_as_a_command_when_check_only_gets_one() -> TestResult {
    // given
    let marker = fresh_target("check-only-injection")?.join("ran");
    let name = format!("$(touch {})", marker.display());

    // when
    let output = just(&repo_root()).args(["check-only", &name]).output()?;

    // then
    assert!(!output.status.success(), "{}", printed(&output));
    assert!(!marker.try_exists()?, "the stage name ran as a command");
    Ok(())
}

#[test]
fn should_name_the_valid_stages_when_check_only_gets_an_unknown_one() -> TestResult {
    // given
    let mut check_only = just(&repo_root());
    check_only.args(["check-only", "nope"]);

    // when
    let output = check_only.output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "check-only nope passed:\n{text}");
    assert!(text.contains("unknown stage 'nope'"), "{text}");
    assert!(
        text.contains("valid stages: fmt-check architecture clippy nostd"),
        "{text}"
    );
    Ok(())
}
