//! Running the gate's stages: each an external program with its arguments (argv only, never
//! `sh -c`; fnd §2.4) or a function of this binary.
//!
//! Started from mayfly's `tools/xtask/src/process.rs` (MIT OR Apache-2.0, same owner) and
//! adapted: the stages run in order and the first failure stops the run with its exit code, as
//! the justfile's `check` did (fnd §9), rather than mayfly's run-everything report; a pinned tool
//! a stage needs is checked right before that stage, as the justfile's `_require` was; and every
//! child gets [`Stage::removed`] taken out of its environment.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One gate stage.
#[derive(Debug, Clone)]
pub(crate) struct Stage {
    /// The catalog id, printed in the progress line.
    pub(crate) name: &'static str,
    /// What runs.
    pub(crate) action: Action,
    /// The pinned tool that must be on PATH before the stage runs.
    pub(crate) tool: Option<&'static str>,
}

/// How a stage runs.
#[derive(Debug, Clone)]
pub(crate) enum Action {
    /// One program and its arguments.
    Command(Call),
    /// A function of this binary, given the run's context, shown as `display`.
    InProcess {
        /// The command line it stands for, for the progress line and the catalog.
        display: &'static str,
        /// The function; it returns the stage's exit code.
        run: fn(&Context) -> i32,
    },
}

/// A program, its arguments, the variables it gets and the stderr text that fails it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Call {
    /// The program, found on PATH.
    pub(crate) program: String,
    /// Its arguments, passed as they are.
    pub(crate) args: Vec<String>,
    /// Variables set for this child only.
    pub(crate) env: Vec<(String, String)>,
    /// Stderr text that fails the call even when it exits 0, with the message printed then.
    pub(crate) deny_stderr: Option<(&'static str, &'static str)>,
}

/// What every stage of one run shares: the repository root (where the gate's own scripts are) and
/// the variables taken out of each child's environment.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    /// The checkout whose gate runs (the justfile's `justfile_directory()`).
    pub(crate) root: PathBuf,
    /// Variables removed from every child (those `cargo run` sets for this binary).
    pub(crate) removed: Vec<OsString>,
    /// The PATH every child gets.
    pub(crate) path: Option<OsString>,
}

impl Call {
    /// A call of `program` with `args`.
    pub(crate) fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: Vec::new(),
            deny_stderr: None,
        }
    }

    /// Sets a variable for this child only.
    #[must_use]
    pub(crate) fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// The command line, for the progress line and the catalog.
    pub(crate) fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Runs the call in the current directory and returns its exit code (1 when it could not
    /// start or ended by a signal).
    pub(crate) fn run(&self, context: &Context) -> i32 {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        for name in &context.removed {
            command.env_remove(name);
        }
        if let Some(path) = &context.path {
            command.env("PATH", path);
        }
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        let Some((needle, message)) = self.deny_stderr else {
            return match command.status() {
                Ok(status) => status.code().unwrap_or(1),
                Err(err) => {
                    eprintln!("xtask: cannot run {}: {err}", self.program);
                    1
                }
            };
        };
        command.stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("xtask: cannot run {}: {err}", self.program);
                return 1;
            }
        };
        let denied = child
            .stderr
            .take()
            .is_some_and(|stderr| scan_stderr(BufReader::new(stderr), needle));
        let code = match child.wait() {
            Ok(status) => status.code().unwrap_or(1),
            Err(_) => 1,
        };
        if code != 0 {
            return code;
        }
        if denied {
            eprintln!("{message}");
            return 1;
        }
        0
    }
}

impl Stage {
    /// A stage that runs one call.
    pub(crate) fn call(name: &'static str, call: Call) -> Self {
        Self {
            name,
            action: Action::Command(call),
            tool: None,
        }
    }

    /// A stage that runs `run` in this process.
    pub(crate) fn in_process(
        name: &'static str,
        display: &'static str,
        run: fn(&Context) -> i32,
    ) -> Self {
        Self {
            name,
            action: Action::InProcess { display, run },
            tool: None,
        }
    }

    /// Declares the pinned tool the stage needs.
    #[must_use]
    pub(crate) fn needs(mut self, tool: &'static str) -> Self {
        self.tool = Some(tool);
        self
    }

    /// The command line the stage stands for.
    pub(crate) fn display(&self) -> String {
        match &self.action {
            Action::Command(call) => call.display(),
            Action::InProcess { display, .. } => (*display).to_owned(),
        }
    }

    fn execute(&self, context: &Context) -> i32 {
        if let Some(tool) = self.tool
            && !on_path(tool, context.path.as_deref())
        {
            eprintln!(
                "{tool} is missing: {}; just doctor says what else is",
                crate::tools::INSTALL_HINT
            );
            return 1;
        }
        eprintln!("xtask: stage {}: {}", self.name, self.display());
        match &self.action {
            Action::Command(call) => call.run(context),
            Action::InProcess { run, .. } => run(context),
        }
    }
}

/// Echoes `stderr` line by line and says whether a line contained `needle`. Reads to the end
/// whatever the stream holds (a line that is not UTF-8 is decoded lossily), so the child never
/// blocks on a full pipe. Pinned by `should_fail_when_denied_text_follows_invalid_utf8_on_stderr`.
fn scan_stderr(mut stderr: impl BufRead, needle: &str) -> bool {
    let mut found = false;
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match stderr.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => return found,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&raw);
        let line = line.trim_end_matches(['\n', '\r']);
        eprintln!("{line}");
        found = found || line.contains(needle);
    }
}

/// Whether an executable file named `tool` is in a directory of `path` (the shell's
/// `command -v`).
pub(crate) fn on_path(tool: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let Some(path) = path else {
        return false;
    };
    std::env::split_paths(path).any(|dir| executable(&dir.join(tool)))
}

#[cfg(unix)]
fn executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Runs the stages in order, streaming their output, and stops at the first failure: 0 when every
/// stage passed, otherwise the failing stage's exit code.
pub(crate) fn run_stages(stages: &[Stage], context: &Context) -> i32 {
    for stage in stages {
        let code = stage.execute(context);
        if code != 0 {
            eprintln!("xtask: stage {} FAILED with exit code {code}", stage.name);
            return code;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of scripts for one test, removed when dropped. The stages run each script as
    /// `bash <file>` (argv only, never `-c`, as every spawn of this crate).
    struct Scripts(PathBuf);

    impl Scripts {
        fn new(case: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("xtask-process-{}-{case}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            Self(dir)
        }

        /// `bash <dir>/<name>.sh`, the file holding `body`.
        fn call(&self, name: &str, body: &str) -> Call {
            let file = self.0.join(format!("{name}.sh"));
            let _ = std::fs::write(&file, body);
            Call::new("bash", &[&file.to_string_lossy()])
        }
    }

    impl Drop for Scripts {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn context() -> Context {
        Context {
            root: PathBuf::from("."),
            removed: Vec::new(),
            path: std::env::var_os("PATH"),
        }
    }

    #[test]
    fn should_return_the_failing_exit_code_and_stop_when_a_stage_fails() {
        // given
        let scripts = Scripts::new("stop");
        let marker = scripts.0.join("ran");
        let stages = [
            Stage::call("ok", scripts.call("ok", "exit 0\n")),
            Stage::call("bad", scripts.call("bad", "exit 7\n")),
            Stage::call(
                "later",
                scripts.call("later", &format!("touch '{}'\n", marker.display())),
            ),
        ];

        // when
        let code = run_stages(&stages, &context());

        // then
        assert_eq!(code, 7);
        assert!(!marker.exists(), "the stage after the failure ran");
    }

    #[test]
    fn should_return_zero_when_every_stage_passes() {
        // given
        let scripts = Scripts::new("pass");
        let stages = [
            Stage::call("a", scripts.call("a", "exit 0\n")),
            Stage::call("b", Call::new("true", &[])),
        ];

        // when
        let code = run_stages(&stages, &context());

        // then
        assert_eq!(code, 0);
    }

    #[test]
    fn should_return_the_function_s_code_when_an_in_process_stage_fails() {
        // given
        let stages = [Stage::in_process("inner", "inner", |_| 5)];

        // when
        let code = run_stages(&stages, &context());

        // then
        assert_eq!(code, 5);
    }

    #[test]
    fn should_fail_when_the_program_does_not_exist() {
        // given
        let stages = [Stage::call(
            "missing",
            Call::new("xtask-no-such-program", &[]),
        )];

        // when
        let code = run_stages(&stages, &context());

        // then
        assert_eq!(code, 1);
    }

    #[test]
    fn should_fail_without_running_when_the_stage_s_tool_is_not_on_path() {
        // given
        let scripts = Scripts::new("tool");
        let marker = scripts.0.join("ran");
        let stages = [Stage::call(
            "needs",
            scripts.call("needs", &format!("touch '{}'\n", marker.display())),
        )
        .needs("xtask-no-such-tool")];

        // when
        let code = run_stages(&stages, &context());

        // then
        assert_eq!(code, 1);
        assert!(!marker.exists(), "the stage ran without its tool");
    }

    #[test]
    fn should_pass_the_variable_and_remove_the_others_when_a_call_sets_env() {
        // given
        let scripts = Scripts::new("env");
        let mut ctx = context();
        ctx.removed = vec![OsString::from("HOME")];
        let call = scripts
            .call(
                "env",
                "test \"$XTASK_PROBE\" = 1024 && test -z \"${HOME:-}\"\n",
            )
            .env("XTASK_PROBE", "1024");

        // when
        let code = call.run(&ctx);

        // then
        assert_eq!(code, 0);
    }

    #[test]
    fn should_fail_when_denied_text_is_on_stderr_despite_exit_zero() {
        // given
        let scripts = Scripts::new("deny");
        let mut call = scripts.call("deny", "echo 'x does not refer to a y' >&2\n");
        call.deny_stderr = Some(("does not refer to a", "denied"));

        // when
        let code = call.run(&context());

        // then
        assert_eq!(code, 1);
    }

    #[test]
    fn should_fail_when_denied_text_follows_invalid_utf8_on_stderr() {
        // given: a line that is not UTF-8, then the denied text
        let scripts = Scripts::new("utf8");
        let mut call = scripts.call(
            "utf8",
            "printf 'bad \\377\\376 bytes\\n' >&2\necho 'does not refer to a' >&2\n",
        );
        call.deny_stderr = Some(("does not refer to a", "denied"));

        // when
        let code = call.run(&context());

        // then
        assert_eq!(code, 1);
    }

    #[test]
    fn should_keep_the_exit_code_when_no_denied_text_is_on_stderr() {
        // given
        let scripts = Scripts::new("quiet");
        let mut call = scripts.call("quiet", "echo fine >&2\nexit 3\n");
        call.deny_stderr = Some(("does not refer to a", "denied"));

        // when
        let code = call.run(&context());

        // then
        assert_eq!(code, 3);
    }
}
