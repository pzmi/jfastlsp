//! Task sets and arms into Harbor tasks (Harbor 0.26.0, task `schema_version = "1.4"`).
//!
//! Matched against Harbor 0.26.0 (PyPI sdist sha256
//! `ec75294b7e2a9f9c9419ff9e18033f4e587a8aa312a79e93a5958cf17ef746f8`, tag `v0.26.0` at commit
//! `3cb627394b95d3ae605f31e0d694edf508cf4b7b`): `models/task/paths.py`, `models/task/config.py`,
//! `models/trial/paths.py` and `agents/installed/claude_code.py`. A Harbor task is a directory:
//!
//! ```text
//! <arm>__<task>/
//! ├── task.toml              schema 1.4: [metadata], [agent], [verifier], [environment]
//! ├── instruction.md         the prompt, verbatim
//! ├── environment/Dockerfile FROM the arm's image; its setup; the starting commit, history stripped
//! ├── tests/test.sh          runs tests/test-command.sh; writes 1 or 0 to /logs/verifier/reward.txt
//! ├── tests/test-command.sh  the task's test command, verbatim
//! └── solution/solve.sh      the oracle: applies solution/known-good.patch (the owner writes it)
//! ```
//!
//! Harbor names a task without a `[task]` section after its directory, and records that name as
//! `task_name` in each trial's `result.json`; `<arm>__<task>` is how [`crate::run`] gets the arm
//! and task back. Pinned by: `tests/harbor.rs`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::arm::Arm;
use crate::error::{Context, EvalError, Result};
use crate::ids::{ARM_TASK_SEPARATOR, Id};
use crate::taskset::{Task, TaskSet};

/// The Harbor release whose formats this module writes and [`crate::run`] reads.
pub const HARBOR_VERSION: &str = "0.26.0";
/// The Harbor commit of that release (tag `v0.26.0`).
pub const HARBOR_COMMIT: &str = "3cb627394b95d3ae605f31e0d694edf508cf4b7b";
/// Harbor's task-config schema version written into `task.toml`.
pub const TASK_SCHEMA_VERSION: &str = "1.4";
/// Image builds for a large Gradle repository take a while.
const BUILD_TIMEOUT_SEC: u32 = 1800;
/// The id of the smoke task.
pub const SMOKE_TASK_ID: &str = "smoke-lsp";

/// The settings file of an arm, beside its tasks.
pub const SETTINGS_FILE: &str = "claude-settings.json";

/// Write every task of `set` for every arm under `out` (`out/<arm>/tasks/<arm>__<task>/`), plus
/// each arm's Claude Code settings (`out/<arm>/claude-settings.json`). Returns the Harbor command
/// lines, one per arm. `out` must be absent or empty, so nothing is overwritten.
pub fn write_tasks(set: &TaskSet, arms: &[Arm], out: &Path) -> Result<Vec<String>> {
    prepare_out(out)?;
    let mut commands = Vec::new();
    for arm in arms {
        let arm_dir = out.join(arm.id.as_str());
        write_settings(arm, &arm_dir)?;
        for task in &set.tasks {
            let dir = task_dir(&arm_dir, &arm.id, &task.id);
            write_task(set, arm, task, &dir)?;
        }
        commands.push(harbor_command(&arm_dir));
    }
    Ok(commands)
}

/// Write the smoke task for `arm` under `out`: a two-file synthetic Java project and a prompt
/// that asks for one `goToDefinition`; the verifier rewards 1 only when the transcript holds an
/// `LSP` tool call and the answer names the definition. Its trial, read with `jfl-eval inspect`,
/// says whether language servers start under `claude -p` in a Harbor container (ADR 0029).
pub fn write_smoke(arm: &Arm, out: &Path) -> Result<String> {
    prepare_out(out)?;
    let arm_dir = out.join(arm.id.as_str());
    write_settings(arm, &arm_dir)?;
    let task = Id::parse(SMOKE_TASK_ID).ok_or_else(|| EvalError::new("smoke task id"))?;
    let dir = task_dir(&arm_dir, &arm.id, &task);
    let mut toml = task_toml(&arm.id, &task, "smoke", 600, 300);
    let _ = writeln!(toml, "workdir = \"{SMOKE_DIR}\"");
    write(&dir.join("task.toml"), &toml)?;
    write(&dir.join("instruction.md"), SMOKE_INSTRUCTION)?;
    let mut dockerfile = dockerfile_head(arm);
    dockerfile.push_str(&format!(
        "COPY smoke {SMOKE_DIR}\n\
         RUN git -C {SMOKE_DIR} init -q \\\n \
         && git -C {SMOKE_DIR} add -A \\\n \
         && git -C {SMOKE_DIR} -c user.name=smoke -c user.email=smoke@example.invalid commit -qm smoke\n\
         WORKDIR {SMOKE_DIR}\n"
    ));
    write(&dir.join("environment/Dockerfile"), &dockerfile)?;
    for (path, text) in SMOKE_FILES {
        write(&dir.join("environment/smoke").join(path), text)?;
    }
    write(&dir.join("tests/test.sh"), SMOKE_TEST)?;
    write(&dir.join("solution/solve.sh"), SMOKE_SOLVE)?;
    Ok(harbor_command(&arm_dir))
}

fn prepare_out(out: &Path) -> Result<()> {
    if out.exists() {
        let mut entries = fs::read_dir(out).context(format!("read {}", out.display()))?;
        if entries.next().is_some() {
            return Err(EvalError::new(format!(
                "{}: not empty; jfl-eval writes into an empty directory only",
                out.display()
            )));
        }
    }
    fs::create_dir_all(out).context(format!("create {}", out.display()))
}

fn task_dir(arm_dir: &Path, arm: &Id, task: &Id) -> PathBuf {
    arm_dir
        .join("tasks")
        .join(format!("{arm}{ARM_TASK_SEPARATOR}{task}"))
}

fn write_settings(arm: &Arm, arm_dir: &Path) -> Result<()> {
    let mut text = serde_json::to_string_pretty(&arm.settings).context("settings")?;
    text.push('\n');
    write(&arm_dir.join(SETTINGS_FILE), &text)
}

fn harbor_command(arm_dir: &Path) -> String {
    let tasks = shell_quote(&arm_dir.join("tasks").to_string_lossy());
    let settings = shell_quote(&arm_dir.join(SETTINGS_FILE).to_string_lossy());
    format!(
        "harbor run -p {tasks} -a claude-code -m \"$MODEL\" -k 3 \
         --ak config={settings} --ak version=\"$CLAUDE_CODE_VERSION\" -o \"$JOBS\""
    )
}

/// `s` as one POSIX shell word: bare when it holds only safe characters, else single-quoted.
fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:@%+,-=".contains(&b));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn write_task(set: &TaskSet, arm: &Arm, task: &Task, dir: &Path) -> Result<()> {
    let mut toml = task_toml(
        &arm.id,
        &task.id,
        task.category.as_str(),
        set.agent_timeout_sec,
        set.verifier_timeout_sec,
    );
    let _ = writeln!(toml, "workdir = \"{}\"", arm.workdir);
    write(&dir.join("task.toml"), &toml)?;
    let mut instruction = task.prompt.clone();
    if !instruction.ends_with('\n') {
        instruction.push('\n');
    }
    write(&dir.join("instruction.md"), &instruction)?;
    let mut dockerfile = dockerfile_head(arm);
    let w = &arm.workdir;
    dockerfile.push_str(&checkout_without_history(w, &task.commit, &task.known_good));
    write(&dir.join("environment/Dockerfile"), &dockerfile)?;
    let mut command = task.test_command.clone();
    if !command.ends_with('\n') {
        command.push('\n');
    }
    write(&dir.join("tests/test-command.sh"), &command)?;
    write(
        &dir.join("tests/test.sh"),
        &format!(
            "#!/bin/bash\n\
             # Generated by jfl-eval (EVAL-01): the task's test command from the work directory;\n\
             # reward 1 when it exits 0, else 0 (Harbor reads /logs/verifier/reward.txt).\n\
             mkdir -p /logs/verifier\n\
             cd {w} || {{ echo 0 > /logs/verifier/reward.txt; exit 0; }}\n\
             bash /tests/test-command.sh\n\
             status=$?\n\
             if [ \"$status\" -eq 0 ]; then echo 1; else echo 0; fi > /logs/verifier/reward.txt\n"
        ),
    )?;
    write(
        &dir.join("solution/solve.sh"),
        &format!(
            "#!/bin/bash\n\
             # Generated by jfl-eval (EVAL-01): Harbor's oracle agent applies the known-good diff.\n\
             # The image holds no history past the starting commit, so the diff comes with the\n\
             # task: git diff --binary {commit} {good} > solution/{SOLUTION_PATCH}\n\
             set -euo pipefail\n\
             patch=/solution/{SOLUTION_PATCH}\n\
             if [ ! -s \"$patch\" ]; then\n\
             \x20 echo 'jfl-eval: solution/{SOLUTION_PATCH} is missing (doc/eval/harness.md)' >&2\n\
             \x20 exit 1\n\
             fi\n\
             git -C {w} apply --whitespace=nowarn \"$patch\"\n",
            commit = task.commit,
            good = task.known_good
        ),
    )
}

/// The oracle's diff, which the owner writes beside `solve.sh` (the image cannot hold it).
pub const SOLUTION_PATCH: &str = "known-good.patch";

/// Dockerfile lines that check out the starting commit and strip every way back to a later one.
///
/// The arm image holds the history of every task, so its branches, tags, reflogs and
/// `ORIG_HEAD`/`FETCH_HEAD` reach past the starting commit, and `git log --all` or `git show
/// <known_good>` would hand the agent the fix (the SWE-bench leak). After the checkout every ref
/// is deleted, the reflogs expire and `gc --prune=now` drops what HEAD cannot reach; the build
/// then fails if the known-good commit is still in the object store. Pinned by:
/// `tests/harbor.rs::should_strip_history_past_the_starting_commit_when_writing_the_dockerfile`.
fn checkout_without_history(w: &str, commit: &str, good: &str) -> String {
    format!(
        "WORKDIR {w}\n\
         RUN git -C {w} checkout --quiet --force --detach {commit} \\\n \
         && git -C {w} clean -fdq \\\n \
         && git -C {w} for-each-ref --format='delete %(refname)' | git -C {w} update-ref --stdin \\\n \
         && rm -f \"$(git -C {w} rev-parse --absolute-git-dir)/ORIG_HEAD\" \
         \"$(git -C {w} rev-parse --absolute-git-dir)/FETCH_HEAD\" \\\n \
         && git -C {w} reflog expire --expire=now --all \\\n \
         && git -C {w} gc --quiet --prune=now \\\n \
         && test \"$(git -C {w} for-each-ref | wc -l)\" -eq 0 \\\n \
         && if git -C {w} cat-file -e {good}^{{commit}} 2>/dev/null; then \
         echo 'jfl-eval: the known-good commit is still reachable in the image' >&2; exit 1; fi\n"
    )
}

/// `task.toml` up to the `[environment]` table's `workdir` line, which the caller adds. Every
/// value is an id, a fixed word or a number, so nothing here needs TOML escaping.
fn task_toml(arm: &Id, task: &Id, category: &str, agent_sec: u32, verifier_sec: u32) -> String {
    format!(
        "# Generated by jfl-eval (EVAL-01) for Harbor {HARBOR_VERSION}.\n\
         schema_version = \"{TASK_SCHEMA_VERSION}\"\n\
         \n\
         [metadata]\n\
         jfl_eval_arm = \"{arm}\"\n\
         jfl_eval_task = \"{task}\"\n\
         category = \"{category}\"\n\
         \n\
         [agent]\n\
         timeout_sec = {agent_sec}.0\n\
         \n\
         [verifier]\n\
         timeout_sec = {verifier_sec}.0\n\
         \n\
         [environment]\n\
         build_timeout_sec = {BUILD_TIMEOUT_SEC}.0\n"
    )
}

fn dockerfile_head(arm: &Arm) -> String {
    let mut out = format!(
        "# Generated by jfl-eval (EVAL-01) for arm {}.\nFROM {}\n",
        arm.id, arm.image
    );
    for line in &arm.setup {
        let _ = writeln!(out, "RUN {line}");
    }
    out
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context(format!("create {}", parent.display()))?;
    }
    fs::write(path, text).context(format!("write {}", path.display()))
}

const SMOKE_DIR: &str = "/smoke";

const SMOKE_INSTRUCTION: &str = "\
This is a smoke test of the code-intelligence setup, not a coding task.

Use the LSP tool's goToDefinition on the call `ledger.total()` in
`src/main/java/smoke/App.java` (line 7) to find where `total` is defined. Do not open
`Ledger.java` and do not search with grep: the point is to see whether the language server answers.

Write the definition as `<file name>:<line>` (for example `Foo.java:3`) into `/smoke/answer.txt`,
then stop. If the LSP tool is not available or fails, write `unavailable` instead.
";

const SMOKE_FILES: [(&str, &str); 2] = [
    (
        "src/main/java/smoke/Ledger.java",
        "package smoke;\n\
         \n\
         /** A synthetic ledger for the smoke task. */\n\
         public final class Ledger {\n\
         \x20   private long cents;\n\
         \n\
         \x20   public void add(long amount) {\n\
         \x20       cents += amount;\n\
         \x20   }\n\
         \n\
         \x20   public long total() {\n\
         \x20       return cents;\n\
         \x20   }\n\
         }\n",
    ),
    (
        "src/main/java/smoke/App.java",
        "package smoke;\n\
         \n\
         public final class App {\n\
         \x20   public static void main(String[] args) {\n\
         \x20       Ledger ledger = new Ledger();\n\
         \x20       ledger.add(42);\n\
         \x20       System.out.println(ledger.total());\n\
         \x20   }\n\
         }\n",
    ),
];

/// Claude Code writes its transcript as compact JSON (`"name":"LSP"`) under `$CLAUDE_CONFIG_DIR`,
/// which Harbor 0.26.0 sets to `/logs/agent/sessions`.
const SMOKE_TEST: &str = "\
#!/bin/bash
# Generated by jfl-eval (EVAL-01): reward 1 only when the transcript holds an LSP tool call and
# the answer names the definition (Ledger.java:11). `jfl-eval inspect` on the trial says more.
mkdir -p /logs/verifier
lsp=0
if grep -rqs --include='*.jsonl' '\"name\":\"LSP\"' /logs/agent/sessions; then lsp=1; fi
answer=0
if grep -qs 'Ledger.java:11' /smoke/answer.txt; then answer=1; fi
if [ \"$lsp\" -eq 1 ] && [ \"$answer\" -eq 1 ]; then echo 1; else echo 0; fi > /logs/verifier/reward.txt
";

const SMOKE_SOLVE: &str = "\
#!/bin/bash
# Generated by jfl-eval (EVAL-01): the oracle writes the answer (its trial has no LSP call, so 0).
echo 'Ledger.java:11' > /smoke/answer.txt
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_quote_a_path_when_it_holds_shell_characters() {
        // given
        let paths = ["/eval/out/arm", "/eval/my out/it's"];
        // when
        let quoted: Vec<String> = paths.iter().map(|p| shell_quote(p)).collect();
        // then
        assert_eq!(quoted, vec!["/eval/out/arm", "'/eval/my out/it'\\''s'"]);
    }
}
