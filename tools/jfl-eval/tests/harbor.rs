//! `jfl-eval harbor` and `jfl-eval smoke` through the real binary: the Harbor 0.26.0 task layout
//! they write, and what they refuse.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_jfl-eval");

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("jfl-eval-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Result<(bool, String, String), Box<dyn Error>> {
    let out = Command::new(BIN).args(args).output()?;
    Ok((
        out.status.success(),
        String::from_utf8(out.stdout)?,
        String::from_utf8(out.stderr)?,
    ))
}

const TASKS: &str = r#"
schema = 1

[[task]]
id = "fix-rounding"
commit = "1111111111111111111111111111111111111111"
prompt = "Fix the rounding of Money.add."
test_command = "./gradlew :core:test --tests 'MoneyTest'"
known_good = "2222222222222222222222222222222222222222"
category = "bug-fix"

[[task]]
id = "rename-entry"
commit = "3333333333333333333333333333333333333333"
prompt = "Rename Entry to LedgerEntry."
test_command = "./gradlew test"
known_good = "4444444444444444444444444444444444444444"
category = "rename"
"#;

const ARM: &str = r#"
schema = 1
id = "jdtls-proxy"
image = "jfl-eval/jdtls-proxy:2026-11-17"
setup = ["git config --global --add safe.directory /app"]
plugins = ["jfl-jdtls@jfl-eval"]

[env]
ENABLE_LSP_TOOL = "1"
"#;

fn write_inputs(dir: &Dir) -> Result<(String, String), Box<dyn Error>> {
    let tasks = dir.path().join("tasks.toml");
    let arm = dir.path().join("arm.toml");
    std::fs::write(&tasks, TASKS)?;
    std::fs::write(&arm, ARM)?;
    Ok((
        tasks.to_string_lossy().into_owned(),
        arm.to_string_lossy().into_owned(),
    ))
}

#[test]
fn should_write_a_harbor_task_per_task_and_arm_when_given_a_task_set() {
    // given
    let dir = Dir::new("harbor").unwrap();
    let (tasks, arm) = write_inputs(&dir).unwrap();
    let out = dir.path().join("out");
    // when
    let (ok, stdout, stderr) = run(&[
        "harbor",
        "--tasks",
        &tasks,
        "--arm",
        &arm,
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    // then
    assert!(ok, "{stderr}");
    let task = out.join("jdtls-proxy/tasks/jdtls-proxy__fix-rounding");
    let toml = std::fs::read_to_string(task.join("task.toml")).unwrap();
    assert!(toml.contains("schema_version = \"1.4\""), "{toml}");
    assert!(toml.contains("jfl_eval_task = \"fix-rounding\""), "{toml}");
    assert!(toml.contains("category = \"bug-fix\""), "{toml}");
    assert!(toml.contains("workdir = \"/app\""), "{toml}");
    let parsed: toml::Table = toml::from_str(&toml).unwrap();
    assert_eq!(parsed["agent"]["timeout_sec"].as_float(), Some(3600.0));
    assert_eq!(
        std::fs::read_to_string(task.join("instruction.md")).unwrap(),
        "Fix the rounding of Money.add.\n"
    );
    let dockerfile = std::fs::read_to_string(task.join("environment/Dockerfile")).unwrap();
    assert!(
        dockerfile.contains("FROM jfl-eval/jdtls-proxy:2026-11-17\n"),
        "{dockerfile}"
    );
    assert!(dockerfile.contains("RUN git config --global --add safe.directory /app\n"));
    assert!(dockerfile.contains("--detach 1111111111111111111111111111111111111111"));
    assert_eq!(
        std::fs::read_to_string(task.join("tests/test-command.sh")).unwrap(),
        "./gradlew :core:test --tests 'MoneyTest'\n"
    );
    let test = std::fs::read_to_string(task.join("tests/test.sh")).unwrap();
    assert!(test.contains("/logs/verifier/reward.txt"), "{test}");
    let solve = std::fs::read_to_string(task.join("solution/solve.sh")).unwrap();
    assert!(
        solve.contains("apply --whitespace=nowarn \"$patch\""),
        "{solve}"
    );
    assert!(solve.contains("/solution/known-good.patch"), "{solve}");
    assert!(
        out.join("jdtls-proxy/tasks/jdtls-proxy__rename-entry/task.toml")
            .is_file()
    );
    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("jdtls-proxy/claude-settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["enabledPlugins"]["jfl-jdtls@jfl-eval"], true);
    assert_eq!(settings["env"]["ENABLE_LSP_TOOL"], "1");
    assert!(stdout.contains("harbor run -p "), "{stdout}");
    assert!(stdout.contains("--ak config="), "{stdout}");
}

#[test]
fn should_strip_history_past_the_starting_commit_when_writing_the_dockerfile() {
    // given
    let dir = Dir::new("harbor-history").unwrap();
    let (tasks, arm) = write_inputs(&dir).unwrap();
    let out = dir.path().join("out");
    // when
    let (ok, _, stderr) = run(&[
        "harbor",
        "--tasks",
        &tasks,
        "--arm",
        &arm,
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    // then: refs deleted, reflogs expired, unreachable objects pruned, and the build fails if
    // the known-good commit survived
    assert!(ok, "{stderr}");
    let dockerfile = std::fs::read_to_string(
        out.join("jdtls-proxy/tasks/jdtls-proxy__fix-rounding/environment/Dockerfile"),
    )
    .unwrap();
    let run_line = &dockerfile[dockerfile.find("RUN git -C /app checkout").unwrap()..];
    for step in [
        "checkout --quiet --force --detach 1111111111111111111111111111111111111111",
        "for-each-ref --format='delete %(refname)' | git -C /app update-ref --stdin",
        "/ORIG_HEAD",
        "/FETCH_HEAD",
        "reflog expire --expire=now --all",
        "gc --quiet --prune=now",
        "test \"$(git -C /app for-each-ref | wc -l)\" -eq 0",
        "if git -C /app cat-file -e 2222222222222222222222222222222222222222^{commit} 2>/dev/null; then",
        ">&2; exit 1; fi",
    ] {
        assert!(run_line.contains(step), "missing {step:?} in:\n{run_line}");
    }
    // one RUN instruction: every line but the last continues it
    let lines: Vec<&str> = run_line.lines().collect();
    assert!(
        lines[..lines.len() - 1].iter().all(|l| l.ends_with(" \\")),
        "{run_line}"
    );
}

#[test]
fn should_refuse_to_write_when_the_output_directory_is_not_empty() {
    // given
    let dir = Dir::new("harbor-nonempty").unwrap();
    let (tasks, arm) = write_inputs(&dir).unwrap();
    // when: the output is the directory holding the inputs
    let (ok, _, stderr) = run(&[
        "harbor",
        "--tasks",
        &tasks,
        "--arm",
        &arm,
        "--out",
        dir.path().to_str().unwrap(),
    ])
    .unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("not empty"), "{stderr}");
}

#[test]
fn should_refuse_two_arms_when_they_share_an_id() {
    // given
    let dir = Dir::new("harbor-twice").unwrap();
    let (tasks, arm) = write_inputs(&dir).unwrap();
    let out = dir.path().join("out");
    // when
    let (ok, _, stderr) = run(&[
        "harbor",
        "--tasks",
        &tasks,
        "--arm",
        &arm,
        "--arm",
        &arm,
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("twice"), "{stderr}");
}

#[test]
fn should_write_the_smoke_task_when_given_an_arm() {
    // given
    let dir = Dir::new("smoke").unwrap();
    let (_, arm) = write_inputs(&dir).unwrap();
    let out = dir.path().join("out");
    // when
    let (ok, stdout, stderr) =
        run(&["smoke", "--arm", &arm, "--out", out.to_str().unwrap()]).unwrap();
    // then
    assert!(ok, "{stderr}");
    let task = out.join("jdtls-proxy/tasks/jdtls-proxy__smoke-lsp");
    let ledger =
        std::fs::read_to_string(task.join("environment/smoke/src/main/java/smoke/Ledger.java"))
            .unwrap();
    // The verifier's expected answer, Ledger.java:11, is where `total` is declared.
    assert_eq!(
        ledger.lines().nth(10),
        Some("    public long total() {"),
        "{ledger}"
    );
    let app = std::fs::read_to_string(task.join("environment/smoke/src/main/java/smoke/App.java"))
        .unwrap();
    assert!(
        app.lines().nth(6).unwrap().contains("ledger.total()"),
        "{app}"
    );
    let test = std::fs::read_to_string(task.join("tests/test.sh")).unwrap();
    assert!(
        test.contains("Ledger.java:11") && test.contains("\"name\":\"LSP\""),
        "{test}"
    );
    let dockerfile = std::fs::read_to_string(task.join("environment/Dockerfile")).unwrap();
    assert!(dockerfile.contains("COPY smoke /smoke"), "{dockerfile}");
    assert!(stdout.contains("harbor run"), "{stdout}");
}

#[test]
fn should_print_the_usage_when_called_without_arguments() {
    // given
    // when
    let (ok, stdout, _) = run(&[]).unwrap();
    // then
    assert!(ok);
    assert!(
        stdout.contains("usage: jfl-eval") && stdout.contains("0.26.0"),
        "{stdout}"
    );
}

#[test]
fn should_fail_and_say_why_when_the_command_is_unknown() {
    // given
    // when
    let (ok, _, stderr) = run(&["frobnicate"]).unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("unknown command"), "{stderr}");
}
