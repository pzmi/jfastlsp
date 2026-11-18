//! The `git` CLI, hardened: every invocation goes through `jfl-process` (argv only, a cleared
//! environment plus an allowlist, a deadline, its own process group, capped output) and through
//! [`Git::command`], which fixes what the user's and the repository's configuration could
//! otherwise change about the output or run as a side effect.
//!
//! The indexed repository is untrusted input (plan §11), its `.git/config` included: a repository
//! unpacked from an archive, copied, synced or mounted belongs to the user, so Git's ownership check
//! passes it, though its configuration came from elsewhere, and an `[include]` can pull committed
//! content into it. So nothing the configuration names may run. What [`Git::command`] sets, and
//! why:
//!
//! - **No system or user configuration:** `GIT_CONFIG_NOSYSTEM=1` and
//!   `GIT_CONFIG_GLOBAL=/dev/null` (git ≥ 2.32) leave out `/etc/gitconfig`, `~/.gitconfig` and
//!   `~/.config/git/config`; `core.excludesFile=/dev/null` and `core.attributesFile=/dev/null`
//!   leave out the user's global ignore and attributes files, which Git reads by default from
//!   `~/.config/git/` whatever `GIT_CONFIG_GLOBAL` says. A user's global `safe.directory` entries
//!   no longer apply either: a repository owned by another user, which the user had allowed
//!   globally, is refused (`NotAWorktree`). The repository's own configuration (and what it
//!   includes) is still read.
//! - **Nothing the configuration names is run:**
//!   - `core.fsmonitor=false`: a `status` would run the configured monitor;
//!   - `core.hooksPath=/dev/null`: no hook;
//!   - filter drivers: before `diff` and `status`, the reconcile lists the repository's
//!     `filter.<driver>.*` keys (`git config --get-regexp`, which runs nothing) and passes each
//!     driver with empty `clean`, `smudge` and `process` and `required=false`
//!     ([`Git::run_overriding`], through `GIT_CONFIG_COUNT` so a driver name holding `=` cannot be
//!     misread). A `status` that rehashes a file would otherwise run its clean or process filter,
//!     on every reconcile, since the index is never refreshed;
//!     one window remains: a driver added to the configuration (or an include) between the
//!     listing and the `status` it guards, up to about 40 ms on the medium corpus, would run once.
//!     That needs a process writing the repository's configuration at that moment, not a
//!     repository at rest; closing it would take Git's own switch to ignore all filters, which it
//!     does not have;
//!   - `--no-ext-diff` and `--no-textconv` on `diff`: no `diff.external`, no
//!     `diff.<driver>.command`, no textconv; `--name-only` prints no patch in any case;
//!   - `merge.<driver>.driver` is not reachable: no command run here merges;
//!   - `protocol.allow=never` and `GIT_NO_LAZY_FETCH=1`: a partial clone never fetches a missing
//!     object (no transport, no credential helper, no `core.sshCommand`);
//!   - `safe.bareRepository=explicit`: a bare repository planted in a working tree is never
//!     discovered;
//!   - no pager (`--no-pager`, `GIT_PAGER=cat`).
//! - **Nothing is written:** `GIT_OPTIONAL_LOCKS=0`, so `status` does not refresh and rewrite the
//!   index (which would move the very stat a view compares, and race the user's own `git`).
//! - **The commit's own objects:** `GIT_NO_REPLACE_OBJECTS=1`, so `git replace` refs and grafts,
//!   which move no file of the stat set, cannot change the tree a view diffs from.
//! - **Fixed output:** `LC_ALL=C`, `core.quotepath=off`, `color.ui=false`, `-z` on every command
//!   whose output names paths, `--no-renames` and `--no-relative`; `GIT_TERMINAL_PROMPT=0`.
//!
//! Only `PATH` (to find `git`) and `HOME` (to expand a `~` in the repository's own configuration)
//! are inherited. Pinned by `tests/hardening.rs`.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use jfl_application::{GitStep, RepoProbeError};
use jfl_process::{Command, Ending, EnvSource, Exit, OutputLimits, RunError, Runner};

/// The configuration every invocation carries on its command line, before the subcommand.
pub const PROTECTIONS: [&str; 16] = [
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.quotepath=off",
    "-c",
    "color.ui=false",
    "-c",
    "protocol.allow=never",
    "-c",
    "safe.bareRepository=explicit",
    "-c",
    "core.excludesFile=/dev/null",
    "-c",
    "core.attributesFile=/dev/null",
];

/// The variables every invocation sets.
pub const ENVIRONMENT: [(&str, &str); 10] = [
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_PAGER", "cat"),
    ("PAGER", "cat"),
    ("GIT_NO_LAZY_FETCH", "1"),
    ("GIT_ADVICE", "0"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
];

/// The variables every invocation inherits from the daemon's environment.
pub const INHERITED: [&str; 2] = ["PATH", "HOME"];

/// How much stderr is kept: only its length is ever used.
const STDERR_LIMIT: usize = 4096;

/// The `git` program and the runner that starts it.
#[derive(Debug, Clone)]
pub struct Git<E> {
    runner: Runner<E>,
    program: std::ffi::OsString,
}

/// One invocation: what [`Git::call`] runs and what it accepts.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    /// The step it serves, for errors.
    pub step: GitStep,
    /// The subcommand and its arguments.
    pub args: &'a [&'a str],
    /// The stdout kept; more is refused.
    pub stdout_limit: usize,
    /// When it is killed.
    pub deadline: Duration,
    /// Exit codes besides 0 that are a success.
    pub accepted: &'a [i32],
    /// Command-line configuration, passed through the environment.
    pub overrides: &'a [(OsString, OsString)],
}

/// What a successful run printed.
#[derive(Debug)]
pub struct Printed {
    /// Its exit (code 0, or one the caller said it accepts).
    pub exit: Exit,
    /// Its stdout, whole.
    pub stdout: Vec<u8>,
}

impl<E: EnvSource> Git<E> {
    /// `program` (a bare name is looked up on the inherited `PATH`), started by `runner`.
    pub fn new(runner: Runner<E>, program: impl Into<std::ffi::OsString>) -> Self {
        Self {
            runner,
            program: program.into(),
        }
    }

    /// `git --no-pager -C dir <protections> args...` with the fixed environment.
    pub fn command(&self, dir: &Path, args: &[&str], stdout_limit: usize) -> Command {
        self.command_overriding(dir, args, stdout_limit, &[])
    }

    /// [`Git::command`], with `overrides` as command-line configuration through
    /// `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>` (git ≥ 2.31): a key
    /// passes whole, whatever its subsection holds.
    pub fn command_overriding(
        &self,
        dir: &Path,
        args: &[&str],
        stdout_limit: usize,
        overrides: &[(OsString, OsString)],
    ) -> Command {
        let mut command = Command::new(self.program.clone())
            .arg("--no-pager")
            .arg("-C")
            .arg(dir.as_os_str())
            .args(PROTECTIONS)
            .args(args)
            .output_limits(OutputLimits {
                stdout: stdout_limit,
                stderr: STDERR_LIMIT,
            });
        for (name, value) in ENVIRONMENT {
            command = command.env(name, value);
        }
        for name in INHERITED {
            command = command.inherit_env(name);
        }
        if !overrides.is_empty() {
            command = command.env("GIT_CONFIG_COUNT", overrides.len().to_string());
            for (n, (key, value)) in overrides.iter().enumerate() {
                command = command
                    .env(format!("GIT_CONFIG_KEY_{n}"), key.clone())
                    .env(format!("GIT_CONFIG_VALUE_{n}"), value.clone());
            }
        }
        command
    }

    /// Runs [`Git::command`] to its end or `deadline`. Exit code 0, and any code in `accepted`,
    /// is a success; output past `stdout_limit` is refused, never truncated into an answer.
    pub fn run(
        &self,
        step: GitStep,
        dir: &Path,
        args: &[&str],
        stdout_limit: usize,
        deadline: Duration,
        accepted: &[i32],
    ) -> Result<Printed, RepoProbeError> {
        self.call(
            dir,
            &Call {
                step,
                args,
                stdout_limit,
                deadline,
                accepted,
                overrides: &[],
            },
        )
    }

    /// Runs `call` in `dir`, as [`Git::run`] does, with its configuration overrides
    /// ([`Git::command_overriding`]).
    pub fn call(&self, dir: &Path, call: &Call<'_>) -> Result<Printed, RepoProbeError> {
        let Call {
            step,
            args,
            stdout_limit,
            deadline,
            accepted,
            overrides,
        } = *call;
        let command = self.command_overriding(dir, args, stdout_limit, overrides);
        let outcome = self
            .runner
            .run(&command, deadline)
            .map_err(|error| match error {
                RunError::Refused(_) | RunError::FailedToStart { .. } => RepoProbeError::GitMissing,
                RunError::Wait(source) => RepoProbeError::Io(source.kind()),
            })?;
        let exit = match outcome.ending {
            Ending::KilledOnDeadline => return Err(RepoProbeError::Deadline(step)),
            Ending::Exited(exit) => exit,
        };
        let code = match exit {
            Exit::Code(code) => Some(code),
            Exit::Signal(_) | Exit::Other(_) => None,
        };
        if !code.is_some_and(|code| code == 0 || accepted.contains(&code)) {
            return Err(RepoProbeError::GitFailed { step, code });
        }
        if outcome.stdout.is_truncated() {
            return Err(RepoProbeError::OutputTooLarge(step));
        }
        Ok(Printed {
            exit,
            stdout: outcome.stdout.into_bytes(),
        })
    }
}
