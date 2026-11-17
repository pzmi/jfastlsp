//! What to run: a program, its arguments, the environment allowlist, a working directory and the
//! output limits. Built freely, checked once when a mode starts it ([`Command::check`]).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::env::EnvSource;
use crate::error::Refusal;

/// The default cap on each of stdout and stderr: 1 MiB, more than any `git` or Gradle answer the
/// adapters parse, small enough that a runaway child cannot grow the daemon.
pub const DEFAULT_OUTPUT_LIMIT: usize = 1 << 20;

/// How many bytes of stdout and stderr a one-shot run keeps; the rest is counted and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLimits {
    /// The bytes of stdout kept.
    pub stdout: usize,
    /// The bytes of stderr kept.
    pub stderr: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            stdout: DEFAULT_OUTPUT_LIMIT,
            stderr: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

/// One variable of the child's allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvEntry {
    /// Set to this value.
    Set(OsString),
    /// Copied from the [`EnvSource`] when the child starts; left out when the source has none.
    Inherit,
}

/// A child to start: the program, its arguments as separate strings, the variables it may see,
/// its working directory and its output limits.
///
/// The environment starts empty; [`Command::env`] and [`Command::inherit_env`] are the only way a
/// variable reaches the child. A program without a `/` is looked up on the `PATH` of the child's
/// own environment, so such a command must allowlist `PATH`; otherwise it is refused
/// ([`Refusal::NoPathForLookup`]) rather than searched for on a path nobody chose, and a `PATH`
/// with an empty or relative entry is refused ([`Refusal::RelativePathEntry`]), since that entry
/// would search the child's working directory. Pinned by
/// `command::tests::should_refuse_when_a_bare_program_has_no_path_to_search`,
/// `tests/run.rs::should_find_a_bare_program_when_path_is_allowlisted` and
/// `tests/run.rs::should_refuse_a_planted_binary_when_path_has_a_relative_entry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: BTreeMap<OsString, EnvEntry>,
    cwd: Option<PathBuf>,
    limits: OutputLimits,
}

/// A command after its checks: what the modes hand to the OS.
#[derive(Debug, Clone)]
pub(crate) struct Checked {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: Vec<(OsString, OsString)>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) limits: OutputLimits,
}

impl Command {
    /// A command running `program` with no arguments, no environment, the parent's working
    /// directory and the default output limits.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            limits: OutputLimits::default(),
        }
    }

    /// A command from a whole argv: its first element is the program, the rest its arguments.
    ///
    /// An empty argv is refused with [`Refusal::EmptyArgv`]. Pinned by
    /// `command::tests::should_refuse_when_the_argv_is_empty`.
    pub fn from_argv<I, S>(argv: I) -> Result<Self, Refusal>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut argv = argv.into_iter().map(Into::into);
        let program = argv.next().ok_or(Refusal::EmptyArgv)?;
        Ok(Self::new(program).args(argv))
    }

    /// Appends one argument, passed to the child as it is.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Appends arguments, each passed to the child as it is.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets a variable in the child's environment (the last setting of a name wins).
    #[must_use]
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.insert(name.into(), EnvEntry::Set(value.into()));
        self
    }

    /// Allowlists a variable of the parent: its value is read from the runner's [`EnvSource`]
    /// when the child starts, and it is left out when the source has none.
    #[must_use]
    pub fn inherit_env(mut self, name: impl Into<OsString>) -> Self {
        self.env.insert(name.into(), EnvEntry::Inherit);
        self
    }

    /// The child's working directory.
    #[must_use]
    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// The output limits of a one-shot run.
    #[must_use]
    pub fn output_limits(mut self, limits: OutputLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The program, as given.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Checks the command and resolves its environment against `source`: an empty program, a NUL
    /// anywhere, a variable name that is empty or holds `=`, or a bare program name with no `PATH`
    /// in the child's environment is refused. Pinned by `command::tests`.
    pub(crate) fn check(&self, source: &impl EnvSource) -> Result<Checked, Refusal> {
        if self.program.is_empty() {
            return Err(Refusal::EmptyProgram);
        }
        refuse_nul(&self.program, "the program")?;
        for arg in &self.args {
            refuse_nul(arg, "an argument")?;
        }
        if let Some(cwd) = &self.cwd {
            refuse_nul(cwd.as_os_str(), "the working directory")?;
        }
        let mut env = Vec::with_capacity(self.env.len());
        for (name, entry) in &self.env {
            if name.is_empty() || name.as_bytes().contains(&b'=') {
                return Err(Refusal::InvalidEnvName {
                    name: name.to_string_lossy().into_owned(),
                });
            }
            refuse_nul(name, "a variable name")?;
            let value = match entry {
                EnvEntry::Set(value) => Some(value.clone()),
                EnvEntry::Inherit => source.var_os(name),
            };
            if let Some(value) = value {
                refuse_nul(&value, "a variable value")?;
                env.push((name.clone(), value));
            }
        }
        if !self.program.as_bytes().contains(&b'/') {
            let Some((_, path)) = env.iter().find(|(name, _)| name == "PATH") else {
                return Err(Refusal::NoPathForLookup {
                    program: self.program.to_string_lossy().into_owned(),
                });
            };
            // An empty or relative entry (a trailing `:` is common) searches the child's working
            // directory, which is the repository being indexed: a planted binary would run.
            if let Some(entry) = path
                .as_bytes()
                .split(|b| *b == b':')
                .find(|entry| !entry.starts_with(b"/"))
            {
                return Err(Refusal::RelativePathEntry {
                    entry: String::from_utf8_lossy(entry).into_owned(),
                });
            }
        }
        Ok(Checked {
            program: self.program.clone(),
            args: self.args.clone(),
            env,
            cwd: self.cwd.clone(),
            limits: self.limits,
        })
    }
}

fn refuse_nul(value: &OsStr, what: &'static str) -> Result<(), Refusal> {
    if value.as_bytes().contains(&0) {
        Err(Refusal::Nul { what })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env() -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    #[test]
    fn should_refuse_when_the_argv_is_empty() {
        // given
        let argv: Vec<OsString> = Vec::new();

        // when
        let command = Command::from_argv(argv);

        // then
        assert_eq!(command, Err(Refusal::EmptyArgv));
    }

    #[test]
    fn should_split_program_and_arguments_when_built_from_an_argv() {
        // given
        let argv = ["/usr/bin/git", "status", "-z"];

        // when
        let command = Command::from_argv(argv).unwrap();

        // then
        let checked = command.check(&no_env()).unwrap();
        assert_eq!(checked.program, "/usr/bin/git");
        assert_eq!(checked.args, ["status", "-z"]);
    }

    #[test]
    fn should_refuse_when_the_program_is_empty() {
        // given
        let command = Command::new("");

        // when
        let checked = command.check(&no_env());

        // then
        assert_eq!(checked.unwrap_err(), Refusal::EmptyProgram);
    }

    #[test]
    fn should_refuse_when_an_argument_holds_a_nul() {
        // given
        let command = Command::new("/bin/true").arg("a\0b");

        // when
        let checked = command.check(&no_env());

        // then
        assert_eq!(
            checked.unwrap_err(),
            Refusal::Nul {
                what: "an argument"
            }
        );
    }

    #[test]
    fn should_refuse_when_a_variable_name_holds_an_equals_sign() {
        // given
        let command = Command::new("/bin/true").env("A=B", "c");

        // when
        let checked = command.check(&no_env());

        // then
        assert_eq!(
            checked.unwrap_err(),
            Refusal::InvalidEnvName {
                name: "A=B".to_owned()
            }
        );
    }

    #[test]
    fn should_refuse_when_a_bare_program_has_no_path_to_search() {
        // given
        let command = Command::new("git").arg("status");

        // when
        let checked = command.check(&no_env());

        // then
        assert_eq!(
            checked.unwrap_err(),
            Refusal::NoPathForLookup {
                program: "git".to_owned()
            }
        );
    }

    #[test]
    fn should_accept_a_bare_program_when_path_is_allowlisted() {
        // given
        let parent = BTreeMap::from([(OsString::from("PATH"), OsString::from("/usr/bin"))]);
        let command = Command::new("git").inherit_env("PATH");

        // when
        let checked = command.check(&parent).unwrap();

        // then
        assert_eq!(
            checked.env,
            [(OsString::from("PATH"), OsString::from("/usr/bin"))]
        );
    }

    #[test]
    fn should_refuse_when_path_has_an_empty_entry_for_a_bare_program() {
        // given
        let command = Command::new("git").env("PATH", "/usr/bin:");

        // when
        let checked = command.check(&no_env());

        // then
        assert_eq!(
            checked.unwrap_err(),
            Refusal::RelativePathEntry {
                entry: String::new()
            }
        );
    }

    #[test]
    fn should_accept_a_relative_path_entry_when_the_program_has_a_slash() {
        // given
        let command = Command::new("/usr/bin/git").env("PATH", "bin:");

        // when
        let checked = command.check(&no_env());

        // then
        assert!(checked.is_ok());
    }

    #[test]
    fn should_resolve_only_the_allowlist_when_the_source_has_more() {
        // given
        let parent = BTreeMap::from([
            (OsString::from("HOME"), OsString::from("/home/x")),
            (OsString::from("SECRET"), OsString::from("s")),
        ]);
        let command = Command::new("/bin/true")
            .inherit_env("HOME")
            .inherit_env("ABSENT")
            .env("LANG", "C");

        // when
        let checked = command.check(&parent).unwrap();

        // then
        assert_eq!(
            checked.env,
            [
                (OsString::from("HOME"), OsString::from("/home/x")),
                (OsString::from("LANG"), OsString::from("C")),
            ]
        );
    }
}
