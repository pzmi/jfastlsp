//! `scripts/xtask.sh`, the launcher every gate recipe, `land.sh` and the bootstrap call (XT-01):
//! the build of xtask, the one step before xtask's preflight can look, takes nothing from the
//! caller's environment beyond its allowlist and nothing from a cargo configuration of the working
//! directory or its ancestors, and it never runs xtask through `cargo run` (whose configured
//! runner would replace xtask, so the gate could pass without running). Each case is a nested
//! cargo build, in nextest's `nested-cargo` group; it builds into the caller's CARGO_TARGET_DIR
//! (or the repository's target directory, as the gate does), where the gate built xtask already.

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

/// What a gate run may inherit, passed through (CARGO_TARGET_DIR included, so the launcher finds
/// the xtask the gate built).
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

/// A fresh directory outside the repository, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("xtask-launcher-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    /// An executable script at `name` that records its call in `<name>.ran`.
    fn marker_script(&self, name: &str) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
        let script = self.0.join(name);
        let ran = self.0.join(format!("{name}.ran"));
        fs::write(
            &script,
            format!(
                "#!/usr/bin/env bash\ntouch '{}'\nexec \"$@\"\n",
                ran.display()
            ),
        )?;
        let chmod = Command::new("chmod").arg("+x").arg(&script).output()?;
        assert!(chmod.status.success(), "{}", printed(&chmod));
        Ok((script, ran))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `bash scripts/xtask.sh <args>` in `cwd` with a cleared environment plus [`PASSED_THROUGH`].
fn launcher(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts/xtask.sh"))
        .args(args)
        .current_dir(cwd)
        .env_clear();
    for name in PASSED_THROUGH {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// The channel rust-toolchain.toml pins.
fn channel() -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(repo_root().join("rust-toolchain.toml"))?;
    text.lines()
        .find_map(|l| l.strip_prefix("channel = \"")?.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| "no channel".into())
}

#[test]
fn should_build_xtask_without_the_caller_s_steering_when_the_launcher_runs() -> TestResult {
    // given: a wrapper and a compiler that leave a mark, and flags that would force a rebuild
    // through them if they reached the build
    let scratch = Scratch::new("env")?;
    let (wrapper, wrapper_ran) = scratch.marker_script("wrapper")?;
    let (rustc, rustc_ran) = scratch.marker_script("rustc")?;

    // when
    let output = launcher(&repo_root(), &["preflight"])
        .env("RUSTC_WRAPPER", &wrapper)
        .env("RUSTC", &rustc)
        .env("CARGO_ENCODED_RUSTFLAGS", "--cfg=xtask_probe")
        .env("CARGO_BUILD_TARGET", "x86_64-unknown-none")
        .output()?;

    // then: xtask was built as pinned and then refused the caller's steering
    let text = printed(&output);
    assert!(!output.status.success(), "preflight passed:\n{text}");
    for name in [
        "RUSTC_WRAPPER",
        "RUSTC",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
    ] {
        assert!(text.contains(name), "{name} not refused:\n{text}");
    }
    assert!(
        !wrapper_ran.try_exists()? && !rustc_ran.try_exists()?,
        "{text}"
    );
    Ok(())
}

#[test]
fn should_not_apply_the_working_directory_s_cargo_config_when_the_launcher_builds() -> TestResult {
    // given: a working directory whose cargo configuration sets a runner (which would replace
    // xtask under `cargo run`), a rustc-wrapper and flags (which would rebuild xtask through it)
    let scratch = Scratch::new("config")?;
    let (runner, runner_ran) = scratch.marker_script("runner")?;
    let (wrapper, wrapper_ran) = scratch.marker_script("wrapper")?;
    let work = scratch.0.join("work");
    fs::create_dir_all(work.join(".cargo"))?;
    fs::write(
        work.join(".cargo/config.toml"),
        format!(
            "[build]\nrustc-wrapper = \"{}\"\nrustflags = [\"--cfg\", \"xtask_probe\"]\n\n[target.'cfg(all())']\nrunner = [\"{}\", \"true\"]\n",
            wrapper.display(),
            runner.display()
        ),
    )?;
    let control_config = scratch.0.join("control");
    fs::create_dir_all(control_config.join(".cargo"))?;
    fs::write(
        control_config.join(".cargo/config.toml"),
        format!(
            "[target.'cfg(all())']\nrunner = [\"{}\", \"true\"]\n",
            runner.display()
        ),
    )?;

    // when
    let listed = launcher(&work, &["list"]).output()?;
    let refused = launcher(&work, &["preflight"]).output()?;
    let ran_through_the_launcher = (runner_ran.try_exists()?, wrapper_ran.try_exists()?);
    let mut control = Command::new("cargo");
    control
        .args(["run", "--locked", "-q", "--manifest-path"])
        .arg(repo_root().join("Cargo.toml"))
        .args(["-p", "xtask", "--", "list"])
        .current_dir(&control_config)
        .env_clear()
        .env("RUSTUP_TOOLCHAIN", channel()?)
        .env("RUSTFLAGS", "-D warnings");
    for name in PASSED_THROUGH {
        if let Some(value) = std::env::var_os(name) {
            control.env(name, value);
        }
    }
    let control = control.output()?;

    // then: xtask ran (it listed the stages) and refused the ancestor's file; nothing of the file
    // ran. The control, `cargo run` from a directory with only the runner, ran the runner instead
    // of xtask, which is why the launcher builds and execs.
    let text = printed(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.lines().any(|l| l == "private-names"), "{text}");
    let refusal = printed(&refused);
    assert!(!refused.status.success(), "{refusal}");
    assert!(refusal.contains(".cargo/config.toml"), "{refusal}");
    assert_eq!(ran_through_the_launcher, (false, false), "{text}{refusal}");
    assert!(
        runner_ran.try_exists()? && !printed(&control).contains("private-names"),
        "the control did not run the runner, so the case proves nothing:\n{}",
        printed(&control)
    );
    Ok(())
}

/// A fake `cargo` that, like cargo with a fresh unit, links `debug/xtask` only when nothing is
/// there yet; the binary it links prints the checkout it was "built" from.
const FRESH_CARGO: &str = r#"#!/usr/bin/env bash
manifest=""
while [ $# -gt 0 ]; do
  if [ "$1" = --manifest-path ]; then manifest="$2"; fi
  shift
done
bin="$CARGO_TARGET_DIR/debug/xtask"
if [ ! -e "$bin" ]; then
  mkdir -p "$(dirname "$bin")"
  printf '#!/usr/bin/env bash\necho "built from %s"\n' "$(dirname "$manifest")" >"$bin"
  chmod +x "$bin"
fi
"#;

#[test]
fn should_run_each_checkout_s_own_xtask_when_two_checkouts_share_a_target_dir() -> TestResult {
    // given: two checkouts with the launcher, one shared target directory, and a cargo that
    // relinks nothing it considers fresh (the land worktree and the landing checkout share
    // CARGO_TARGET_DIR; land.sh's retry runs the landing checkout's xtask after the worktree's)
    let scratch = Scratch::new("shared-target")?;
    let bin = scratch.0.join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("cargo"), FRESH_CARGO)?;
    let chmod = Command::new("chmod")
        .arg("+x")
        .arg(bin.join("cargo"))
        .output()?;
    assert!(chmod.status.success(), "{}", printed(&chmod));
    let mut checkouts = Vec::new();
    for name in ["a", "b"] {
        let root = scratch.0.join(name);
        fs::create_dir_all(root.join("scripts"))?;
        for file in ["scripts/xtask.sh", "rust-toolchain.toml"] {
            fs::copy(repo_root().join(file), root.join(file))?;
        }
        checkouts.push(fs::canonicalize(&root)?);
    }
    let mut path = std::ffi::OsString::from(&bin);
    if let Some(rest) = std::env::var_os("PATH") {
        path.push(":");
        path.push(rest);
    }
    let target = scratch.0.join("target");

    // when: a, then b, then a again
    let mut said = Vec::new();
    for root in [&checkouts[0], &checkouts[1], &checkouts[0]] {
        let output = Command::new("bash")
            .arg(root.join("scripts/xtask.sh"))
            .arg("list")
            .current_dir(root)
            .env("PATH", &path)
            .env("CARGO_TARGET_DIR", &target)
            .output()?;
        assert!(output.status.success(), "{}", printed(&output));
        said.push(String::from_utf8(output.stdout)?.trim().to_owned());
    }

    // then: each run ran its own checkout's binary
    let expected: Vec<String> = [&checkouts[0], &checkouts[1], &checkouts[0]]
        .iter()
        .map(|root| format!("built from {}", root.display()))
        .collect();
    assert_eq!(said, expected);
    Ok(())
}

/// A scratch CARGO_HOME holding `config` and links to the real registry and git caches (so the
/// build itself needs no download), as the review's probe had it.
fn cargo_home(scratch: &Scratch, config: &str) -> Result<PathBuf, Box<dyn Error>> {
    let home = scratch.0.join("cargo-home");
    fs::create_dir_all(&home)?;
    let real = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .ok_or("no CARGO_HOME or HOME")?;
    for cache in ["registry", "git"] {
        fs::create_dir_all(real.join(cache))?;
        std::os::unix::fs::symlink(real.join(cache), home.join(cache))?;
    }
    fs::write(home.join("config.toml"), config)?;
    Ok(home)
}

/// Fails unless `output` is a refusal naming `file` before any build ran.
fn assert_refused_before_build(output: &Output, file: &Path) {
    let text = printed(output);
    assert!(!output.status.success(), "the launcher ran xtask:\n{text}");
    assert!(text.contains("refusing to build xtask"), "{text}");
    assert!(text.contains(&file.display().to_string()), "{text}");
}

#[test]
fn should_refuse_to_build_when_the_cargo_home_config_sets_a_linker() -> TestResult {
    // given: the review's X1, a linker that writes a binary exiting 0, so the gate would pass
    // having run nothing; and the control, a CARGO_HOME configuration of the plain shape
    let scratch = Scratch::new("cargo-home-linker")?;
    let (linker, linker_ran) = scratch.marker_script("ld.sh")?;
    let home = cargo_home(
        &scratch,
        &format!(
            "[target.x86_64-unknown-linux-gnu]\nlinker = \"{0}\"\n\n[target.aarch64-unknown-linux-gnu]\nlinker = \"{0}\"\n\n[target.aarch64-apple-darwin]\nlinker = \"{0}\"\n",
            linker.display()
        ),
    )?;
    let plain_scratch = Scratch::new("cargo-home-plain")?;
    let plain = cargo_home(&plain_scratch, "[net]\nretry = 2\n")?;

    // when
    let refused = launcher(&repo_root(), &["list"])
        .env("CARGO_HOME", &home)
        .output()?;
    let accepted = launcher(&repo_root(), &["list"])
        .env("CARGO_HOME", &plain)
        .output()?;

    // then
    assert_refused_before_build(&refused, &home.join("config.toml"));
    assert!(!linker_ran.try_exists()?, "{}", printed(&refused));
    assert!(accepted.status.success(), "{}", printed(&accepted));
    assert!(printed(&accepted).lines().any(|l| l == "private-names"));
    Ok(())
}

#[test]
fn should_refuse_to_build_when_the_cargo_home_config_overrides_rustc() -> TestResult {
    // given: `build.rustc`, which cargo would run for every unit and for its own `rustc -vV`
    let scratch = Scratch::new("cargo-home-rustc")?;
    let (rustc, rustc_ran) = scratch.marker_script("rustc")?;
    let home = cargo_home(
        &scratch,
        &format!("[build]\nrustc = \"{}\"\n", rustc.display()),
    )?;

    // when
    let output = launcher(&repo_root(), &["list"])
        .env("CARGO_HOME", &home)
        .output()?;

    // then
    assert_refused_before_build(&output, &home.join("config.toml"));
    assert!(!rustc_ran.try_exists()?, "{}", printed(&output));
    Ok(())
}

#[test]
fn should_refuse_to_build_when_an_ancestor_of_the_build_holds_a_cargo_config() -> TestResult {
    // given: a cargo configuration above the directory the launcher builds from, which cargo
    // reads as it would `/.cargo/config.toml` (the build directory sits under CARGO_TARGET_DIR,
    // so a target directory under this one stands for `/`, which a test cannot write)
    let scratch = Scratch::new("ancestor")?;
    let (linker, linker_ran) = scratch.marker_script("ld.sh")?;
    fs::create_dir_all(scratch.0.join(".cargo"))?;
    let config = scratch.0.join(".cargo/config.toml");
    fs::write(
        &config,
        format!("[target.'cfg(all())']\nlinker = \"{}\"\n", linker.display()),
    )?;

    // when
    let output = launcher(&repo_root(), &["list"])
        .env("CARGO_TARGET_DIR", scratch.0.join("target"))
        .output()?;

    // then
    assert_refused_before_build(&output, &config);
    assert!(!linker_ran.try_exists()?, "{}", printed(&output));
    Ok(())
}
