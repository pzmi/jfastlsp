//! The preflight's negative cases (fnd §3.6, FND-08, FND-09) against `xtask preflight`, moved
//! here by XT-01 from tools/jfl-architecture/tests/gate_negative_cases.rs with their names: the
//! cases that need a scratch repository (whose root is then `--root`, as `_preflight` took the
//! justfile's directory) or a scratch CARGO_HOME (which scripts/xtask.sh would also hand to its
//! build of xtask). The cases that steer only variables or the working directory still run through
//! the justfile there. Each child gets a cleared environment plus [`PASSED_THROUGH`], so a case
//! sets exactly the steering it tests.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The variables a gate run may inherit (the preflight's allowlist), passed through to the child.
const PASSED_THROUGH: [&str; 7] = [
    "PATH",
    "HOME",
    "RUSTUP_HOME",
    "CARGO_HOME",
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
    "CARGO_HTTP_CAINFO",
];

/// A fresh directory for one case, outside the repository, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("xtask-preflight-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `xtask [--root ROOT] preflight` in `cwd`, with a cleared environment plus [`PASSED_THROUGH`].
fn preflight(root: Option<&Path>, cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    if let Some(root) = root {
        command.arg("--root").arg(root);
    }
    command.arg("preflight").current_dir(cwd).env_clear();
    for name in PASSED_THROUGH {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// A scratch git repository holding a copy of the repository's rust-toolchain.toml, mise.toml and
/// mise.lock, so a case can add the file the preflight must refuse without touching the real tree.
fn scratch_repository(scratch: &Scratch) -> Result<PathBuf, Box<dyn Error>> {
    let root = scratch.0.join("repo");
    fs::create_dir_all(&root)?;
    for file in ["rust-toolchain.toml", "mise.toml", "mise.lock"] {
        fs::copy(repo_root().join(file), root.join(file))?;
    }
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .output()?;
    assert!(init.status.success(), "{}", printed(&init));
    Ok(root)
}

fn scratch_preflight(root: &Path) -> Command {
    preflight(Some(root), root)
}

/// The reviewer's runner (the FND-05 checkpoint's N1): TOML unicode escapes spell `target` and
/// `runner`, which a line grep never matched; the runner passes every test without running it.
const ESCAPED_RUNNER: &str = "[\"t\\u0061rget\".x86_64-unknown-linux-gnu]\n\"r\\u0075nner\" = [\"bash\", \"-c\", \"exit 0\"]\n";

#[test]
fn should_pass_preflight_when_the_scratch_repository_holds_only_the_gate_files() -> TestResult {
    // given: the control for the scratch cases below
    let scratch = Scratch::new("scratch-clean")?;
    let root = scratch_repository(&scratch)?;

    // when
    let output = scratch_preflight(&root).output()?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_the_repository_holds_any_cargo_config() -> TestResult {
    // given: the escaped runner at the root, and an alias in a nested directory
    let mut accepted = Vec::new();
    let cases = [
        (".cargo/config.toml", ESCAPED_RUNNER),
        (
            "crates/x/.cargo/config",
            "[\"\\u0061lias\"]\nxyzzy = \"build\"\n",
        ),
        (".cargo/config.toml", "[net]\nretry = 2\n"),
    ];

    // when
    for (index, (path, contents)) in cases.iter().enumerate() {
        let scratch = Scratch::new(&format!("repo-cargo-config-{index}"))?;
        let root = scratch_repository(&scratch)?;
        let file = root.join(path);
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(&file, contents)?;
        let output = scratch_preflight(&root).output()?;
        if output.status.success() || !printed(&output).contains(path) {
            accepted.push(format!("{path}: {}", printed(&output)));
        }
    }

    // then: inside the repository the allowlist is empty, a harmless key included
    assert!(accepted.is_empty(), "preflight accepted {accepted:?}");
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_mise_toml_has_more_than_tools() -> TestResult {
    // given: the reviewer's N6, an [env] that would reach every tool through its shim
    let scratch = Scratch::new("mise-env")?;
    let root = scratch_repository(&scratch)?;
    let mise = root.join("mise.toml");
    let mut text = fs::read_to_string(&mise)?;
    text.push_str("\n[env]\nNEXTEST_PROFILE = \"evil\"\n");
    fs::write(&mise, text)?;

    // when
    let output = scratch_preflight(&root).output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    assert!(text.contains("mise.toml"), "{text}");
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_another_mise_config_sits_in_the_repository() -> TestResult {
    // given: a second configuration mise would load, tools only
    let scratch = Scratch::new("mise-local")?;
    let root = scratch_repository(&scratch)?;
    fs::write(root.join("mise.local.toml"), "[tools]\njust = \"1.59.0\"\n")?;

    // when
    let output = scratch_preflight(&root).output()?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    assert!(text.contains("mise.local.toml"), "{text}");
    Ok(())
}

#[test]
fn should_run_nothing_from_mise_toml_when_the_gate_meets_the_reviewer_s_payload() -> TestResult {
    // given: the FND-08 re-verification's F3, a mise.toml whose [env] sources a script and
    // expands an exec template, in a tree mise trusts (as land.sh used to make it); mise runs
    // both as it loads the file, so the preflight must refuse before asking mise anything
    let scratch = Scratch::new("mise-payload")?;
    let root = scratch_repository(&scratch)?;
    let sourced = root.join("ran-source");
    let executed = root.join("ran-exec");
    fs::write(
        root.join("payload.sh"),
        format!("touch '{}'\n", sourced.display()),
    )?;
    let mise = root.join("mise.toml");
    let mut text = fs::read_to_string(&mise)?;
    text.push_str(&format!(
        "\n[env]\n_.source = \"{{{{config_root}}}}/payload.sh\"\nX = \"{{{{exec(command='touch {}')}}}}\"\n",
        executed.display()
    ));
    fs::write(&mise, text)?;

    // when
    let output = scratch_preflight(&root)
        .env("MISE_TRUSTED_CONFIG_PATHS", &root)
        .output()?;
    let ran_through_the_gate = (sourced.try_exists()?, executed.try_exists()?);
    let control = Command::new("mise")
        .arg("bin-paths")
        .current_dir(&root)
        .env_clear()
        .envs(
            PASSED_THROUGH
                .iter()
                .filter_map(|n| std::env::var_os(n).map(|v| (*n, v))),
        )
        .env("MISE_TRUSTED_CONFIG_PATHS", &root)
        .output()?;

    // then: the gate refused before mise loaded the file; mise itself, the control, runs both
    let printed_text = printed(&output);
    assert!(!output.status.success(), "the gate passed:\n{printed_text}");
    assert!(printed_text.contains("mise.toml"), "{printed_text}");
    assert_eq!(ran_through_the_gate, (false, false), "{printed_text}");
    assert!(
        sourced.try_exists()? && executed.try_exists()?,
        "the control did not run the payload, so the case proves nothing:\n{}",
        printed(&control)
    );
    Ok(())
}

/// `xtask preflight` in the repository with CARGO_HOME set to a scratch directory holding `config`.
fn with_cargo_home(case: &str, config: &str) -> Result<(Scratch, Output), Box<dyn Error>> {
    let scratch = Scratch::new(case)?;
    fs::write(scratch.0.join("config.toml"), config)?;
    let output = preflight(None, &repo_root())
        .env("CARGO_HOME", &scratch.0)
        .output()?;
    Ok((scratch, output))
}

#[test]
fn should_refuse_the_gate_when_a_cargo_config_steers_in_another_toml_form() -> TestResult {
    // given: the forms FND-02's review found past the line grep
    let forms = [
        "profile = { dev = { overflow-checks = false } }\n",
        "build = { rustflags = [\"--cap-lints=allow\"] }\n",
        "\"build\".rustflags = [\"--cap-lints=allow\"]\n",
        "[ \"profile\".dev ]\noverflow-checks = false\n",
        "env = { RUSTC_BOOTSTRAP = \"1\" }\n",
        // FND-07's review: TOML literal (single-quoted) keys.
        "['profile'.dev]\noverflow-checks = false\n",
        "'profile' = { dev = { overflow-checks = false } }\n",
        "[build]\n'rustflags' = [\"--cap-lints=allow\"]\n",
    ];
    let mut accepted = Vec::new();

    // when
    for (index, form) in forms.iter().enumerate() {
        let (_scratch, output) = with_cargo_home(&format!("cargo-home-form-{index}"), form)?;
        if output.status.success() {
            accepted.push(*form);
        }
    }

    // then
    assert!(accepted.is_empty(), "preflight accepted {accepted:?}");
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_the_cargo_home_config_sets_a_rustc_wrapper() -> TestResult {
    // given
    let config = "[build]\nrustc-wrapper = \"sccache\"\n";

    // when
    let (_scratch, output) = with_cargo_home("cargo-home-config", config)?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    assert!(text.contains("config.toml"), "{text}");
    Ok(())
}

#[test]
fn should_pass_preflight_when_a_cargo_config_sets_only_network_and_job_options() -> TestResult {
    // given
    let config =
        "[net]\ngit-fetch-with-cli = true\n\n[build]\njobs = 2\ntarget-dir = \"elsewhere\"\n";

    // when
    let (_scratch, output) = with_cargo_home("cargo-home-network", config)?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_refuse_the_gate_when_a_cargo_home_config_leaves_the_plain_shape() -> TestResult {
    // given: the escaped runner, an escaped alias table and a dotted key in CARGO_HOME
    let forms = [
        ESCAPED_RUNNER,
        "[\"\\u0061lias\"]\nxyzzy = \"build\"\n",
        "net.retry = 2\n",
        "[net]\nretry = \"\\u0032\"\n",
        "[paths]\n",
        "[source.crates-io]\nreplace-with = \"vendored\"\n",
    ];
    let mut accepted = Vec::new();

    // when
    for (index, form) in forms.iter().enumerate() {
        let (_scratch, output) = with_cargo_home(&format!("cargo-home-shape-{index}"), form)?;
        if output.status.success() {
            accepted.push(*form);
        }
    }

    // then
    assert!(accepted.is_empty(), "preflight accepted {accepted:?}");
    Ok(())
}

#[test]
fn should_accept_cargo_run_s_own_variables_only_at_their_values_when_xtask_runs_under_cargo()
-> TestResult {
    // given: what `cargo run -p xtask` sets for this binary, once at its own values and once with
    // CARGO pointing elsewhere
    let own = [
        ("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR")),
        ("CARGO_PKG_NAME", env!("CARGO_PKG_NAME")),
        ("CARGO_PKG_VERSION", env!("CARGO_PKG_VERSION")),
        ("RUSTUP_TOOLCHAIN_SOURCE", "toolchain-file"),
    ];

    // when
    let accepted = preflight(None, &repo_root()).envs(own).output()?;
    let refused = preflight(None, &repo_root())
        .envs(own)
        .env("CARGO", "/elsewhere/cargo")
        .env("CARGO_PKG_NAME", "other")
        .output()?;

    // then
    assert!(accepted.status.success(), "{}", printed(&accepted));
    let text = printed(&refused);
    assert!(!refused.status.success(), "preflight passed:\n{text}");
    let names: Vec<&str> = text
        .lines()
        .find_map(|l| l.strip_prefix("preflight: refusing to run the gate: "))
        .and_then(|l| l.split(" is outside").next())
        .map(|l| l.split(' ').collect())
        .unwrap_or_default();
    assert_eq!(names, ["CARGO", "CARGO_PKG_NAME"], "{text}");
    Ok(())
}
