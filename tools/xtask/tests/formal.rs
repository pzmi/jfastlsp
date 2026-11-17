//! The formal stages (FORMAL-01, ver §6) through `xtask check-only` and `xtask formal-verify`, over
//! a scratch repository holding the repository's gate files, its `formal/` registry and model, and
//! a scratch bin directory whose fake `node` and `quint` stand in for the pinned tools (so no Node
//! or JVM runs in the test stages, ver §2): each case removes one thing and the stage must fail
//! for that reason, naming the fix; the control passes. The real tools run in the gate's own
//! `formal-typecheck` and `formal-test` stages over the real model, and in `just formal-verify`.

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

/// A fake `quint` that logs its arguments and, for `test`, prints the report `report` names:
/// `pass` (one test passing), `none` (no test) or `fail` (a failing test, exit 1).
const QUINT: &str = r#"#!/bin/sh
echo "quint $*" >> "$FAKE_LOG"
case "$1" in
  test)
    case "$(cat "$FAKE_REPORT")" in
      pass) printf '\n  smoke\n    ok countsUpTest passed 1 test(s)\n\n  1 passing (1ms)\n' ;;
      none) printf '\n  smoke\n\n' ;;
      fail) printf '\n  0 passing (1ms)\n  1 failed\n'; exit 1 ;;
    esac ;;
esac
exit 0
"#;

/// The scratch world of one case, removed when dropped.
struct World {
    dir: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("xtask-formal-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        let world = Self {
            repo: dir.join("repo"),
            bin: dir.join("bin"),
            dir,
        };
        fs::create_dir_all(world.repo.join("tools"))?;
        fs::create_dir_all(world.repo.join("formal"))?;
        for file in [
            "rust-toolchain.toml",
            "mise.toml",
            "mise.lock",
            "tools/versions.toml",
            "formal/models.toml",
            "formal/smoke.qnt",
        ] {
            fs::copy(repo_root().join(file), world.repo.join(file))?;
        }
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&world.repo)
            .output()?;
        assert!(init.status.success(), "{}", printed(&init));
        fs::create_dir_all(&world.bin)?;
        world.tool("node", "#!/bin/sh\necho v0\n")?;
        world.tool("quint", QUINT)?;
        world.report("pass")?;
        Ok(world)
    }

    /// Writes an executable `name` into the scratch bin directory.
    fn tool(&self, name: &str, body: &str) -> TestResult {
        let file = self.bin.join(name);
        fs::write(&file, body)?;
        let chmod = Command::new("chmod").arg("+x").arg(&file).output()?;
        assert!(chmod.status.success(), "{}", printed(&chmod));
        Ok(())
    }

    /// What the fake `quint test` reports.
    fn report(&self, kind: &str) -> TestResult {
        Ok(fs::write(self.dir.join("report"), kind)?)
    }

    /// The fake quint's calls so far.
    fn calls(&self) -> String {
        fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }

    /// `xtask --root <repo> <args>` in the repository, with the scratch bin directory and the
    /// system's PATH, a scratch HOME and tool cache, and nothing else.
    fn xtask(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut path = std::ffi::OsString::from(&self.bin);
        path.push(":/usr/bin:/bin");
        Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("--root")
            .arg(&self.repo)
            .args(args)
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.dir)
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            .env("FAKE_LOG", self.dir.join("log"))
            .env("FAKE_REPORT", self.dir.join("report"))
            .output()?)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn should_typecheck_and_test_each_registered_model_when_the_tools_are_present() -> TestResult {
    // given: the control of the cases below
    let world = World::new("control")?;

    // when
    let output = world.xtask(&["check-only", "formal-typecheck", "formal-test"])?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    assert_eq!(
        world.calls(),
        "quint typecheck formal/smoke.qnt\nquint test --backend typescript --main smoke formal/smoke.qnt\n"
    );
    Ok(())
}

#[test]
fn should_fail_formal_typecheck_when_a_qnt_file_is_not_registered() -> TestResult {
    // given
    let world = World::new("unregistered")?;
    fs::create_dir_all(world.repo.join("formal/more"))?;
    fs::write(
        world.repo.join("formal/more/stray.qnt"),
        "module stray {}\n",
    )?;

    // when
    let output = world.xtask(&["check-only", "formal-typecheck"])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("formal/more/stray.qnt: not registered in formal/models.toml"),
        "{text}"
    );
    assert_eq!(
        world.calls(),
        "",
        "quint ran on a tree with an unregistered model"
    );
    Ok(())
}

#[test]
fn should_fail_formal_typecheck_when_a_model_hides_behind_a_symlinked_directory() -> TestResult {
    // given: a directory of models outside formal/, linked into it
    let world = World::new("symlinked")?;
    let outside = world.dir.join("outside");
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("hidden.qnt"), "module hidden {}\n")?;
    let link = Command::new("ln")
        .arg("-s")
        .arg(&outside)
        .arg(world.repo.join("formal/linked"))
        .output()?;
    assert!(link.status.success(), "{}", printed(&link));

    // when
    let output = world.xtask(&["check-only", "formal-typecheck"])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("formal/linked: not registered"), "{text}");
    Ok(())
}

#[test]
fn should_fail_closed_with_the_install_command_when_node_or_quint_is_missing() -> TestResult {
    // given: each tool removed in turn
    let mut texts = Vec::new();
    for missing in ["node", "quint"] {
        let world = World::new(&format!("missing-{missing}"))?;
        fs::remove_file(world.bin.join(missing))?;

        // when
        let typecheck = world.xtask(&["check-only", "formal-typecheck"])?;
        let test = world.xtask(&["check-only", "formal-test"])?;

        // then
        for output in [typecheck, test] {
            let text = printed(&output);
            assert!(!output.status.success(), "{missing}: {text}");
            assert!(text.contains(&format!("{missing} missing")), "{text}");
            assert!(text.contains("mise install --locked"), "{text}");
            texts.push(text);
        }
        assert_eq!(world.calls(), "", "quint ran without {missing}");
    }
    assert_eq!(texts.len(), 4);
    Ok(())
}

#[test]
fn should_fail_formal_test_when_a_model_has_no_test() -> TestResult {
    // given
    let world = World::new("no-test")?;
    world.report("none")?;

    // when
    let output = world.xtask(&["check-only", "formal-test"])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("formal/smoke.qnt ran no test"), "{text}");
    Ok(())
}

#[test]
fn should_fail_formal_test_with_quint_s_code_when_a_test_fails() -> TestResult {
    // given
    let world = World::new("failing")?;
    world.report("fail")?;

    // when
    let output = world.xtask(&["check-only", "formal-test"])?;

    // then
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_fail_formal_verify_when_the_jvm_tools_are_missing() -> TestResult {
    // given: fake node and quint (whose versions are not the pinned ones), no apalache-mc on the
    // scratch PATH
    let world = World::new("verify-missing")?;

    // when
    let output = world.xtask(&["formal-verify", "smoke"])?;
    let unknown = world.xtask(&["formal-verify", "nope"])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("apalache-mc missing"), "{text}");
    assert!(text.contains("says 'v0', not 26.13.1"), "{text}");
    assert!(
        text.contains("bash scripts/bootstrap-tools.sh --formal-verify"),
        "{text}"
    );
    let calls = world.calls();
    assert!(
        !calls.contains("compile") && !calls.contains("quint run"),
        "quint ran a model without Apalache: {calls}"
    );
    assert_eq!(unknown.status.code(), Some(2), "{}", printed(&unknown));
    assert!(
        printed(&unknown).contains("no model nope; registered: smoke"),
        "{}",
        printed(&unknown)
    );
    Ok(())
}
