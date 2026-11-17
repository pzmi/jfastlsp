//! The `process_runner` contract (ver §5, `ARCH-child-environment`): what any child runner must
//! do, checked the same way against [`ScriptedRunner`] here and against `jfl-process`'s real
//! runner in `crates/jfl-process/tests/contract.rs`.
//!
//! `jfl-testkit` may not depend on `jfl-process` (plan §6.3), so the suite speaks its own small
//! vocabulary: an [`Invocation`] says what the child does ([`Behaviour`]) and how it is run, and
//! the implementation reports what it [`Observed`]. The real runner's factory maps each behaviour
//! to its test child's argv; the scripted runner interprets it. Byte encodings are part of the
//! contract: [`Behaviour::EchoArgs`] writes each argument followed by a NUL to stdout, and
//! [`Behaviour::PrintEnv`] writes `NAME=VALUE` followed by a NUL for each variable, sorted;
//! [`Behaviour::Grandchild`] writes its grandchild's id and a newline. The framed mode has its
//! own small script ([`FramedScript`]): frames are a 4-byte big-endian length and the payload.
//!
//! The suite's own tests run it against deliberately broken runners and assert each fails the
//! case it should (`process_runner::tests`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::time::Duration;

/// What the child does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Behaviour {
    /// Writes each of its arguments, NUL-terminated, to stdout and exits 0.
    EchoArgs,
    /// Writes its environment, `NAME=VALUE` NUL-terminated and sorted, to stdout and exits 0.
    PrintEnv,
    /// Writes `stdout` bytes `o` and `stderr` bytes `e` and exits 0.
    Write {
        /// Bytes to stdout.
        stdout: usize,
        /// Bytes to stderr.
        stderr: usize,
    },
    /// Exits with this code at once.
    Exit(u8),
    /// Never exits by itself.
    Hang,
    /// The program does not exist.
    Missing,
    /// The runner is handed an empty argv.
    EmptyArgv,
    /// An argument holds a NUL byte.
    NulInArgument,
    /// The program is a bare name and the child's environment has no `PATH`.
    BareWithoutPath,
    /// Starts a grandchild (in the child's process group), writes the grandchild's id and a
    /// newline to stdout, and never exits.
    Grandchild,
}

/// One run the suite asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What the child does.
    pub behaviour: Behaviour,
    /// The arguments after the ones that select the behaviour.
    pub args: Vec<OsString>,
    /// Variables set for the child.
    pub set_env: Vec<(OsString, OsString)>,
    /// Variables allowlisted from `parent_env`.
    pub inherit_env: Vec<OsString>,
    /// The parent's environment, which the runner must read inherited variables from.
    pub parent_env: BTreeMap<OsString, OsString>,
    /// The deadline.
    pub deadline: Duration,
    /// Bytes of stdout kept.
    pub stdout_limit: usize,
    /// Bytes of stderr kept.
    pub stderr_limit: usize,
}

impl Invocation {
    /// A run of `behaviour` with no arguments, no environment, a 10 s deadline and 64 KiB limits.
    pub fn of(behaviour: Behaviour) -> Self {
        Self {
            behaviour,
            args: Vec::new(),
            set_env: Vec::new(),
            inherit_env: Vec::new(),
            parent_env: BTreeMap::new(),
            deadline: Duration::from_secs(10),
            stdout_limit: 64 * 1024,
            stderr_limit: 64 * 1024,
        }
    }
}

/// One captured stream: the bytes kept and the count dropped past the limit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stream {
    /// The bytes kept.
    pub bytes: Vec<u8>,
    /// The bytes read past the limit and dropped.
    pub dropped: u64,
}

/// What the runner reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The child exited by itself.
    Exited {
        /// Its exit code, `None` when a signal ended it.
        code: Option<i32>,
        /// Its stdout.
        stdout: Stream,
        /// Its stderr.
        stderr: Stream,
    },
    /// The deadline passed and the child was killed.
    KilledOnDeadline {
        /// Its stdout.
        stdout: Stream,
        /// Its stderr.
        stderr: Stream,
    },
    /// The program could not be started.
    FailedToStart,
    /// The runner refused the invocation before starting anything.
    Refused,
}

/// What a long-lived, framed child does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramedScript {
    /// Echoes each frame it is sent; the runner sends these, one request each.
    Echo(Vec<Vec<u8>>),
    /// Declares a frame of `u32::MAX` bytes, over any limit.
    OversizedReply,
    /// Never answers; the runner waits 200 ms for a reply.
    Silent,
}

/// What a framed exchange reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramedObserved {
    /// The replies, in order.
    Replies(Vec<Vec<u8>>),
    /// A declared frame over the limit was refused.
    FrameTooLarge,
    /// The exchange passed its deadline.
    Deadline,
    /// Anything else, described.
    Other(String),
}

/// A runner under test.
pub trait RunnerUnderTest {
    /// Runs one invocation and reports what happened.
    fn run(&self, invocation: &Invocation) -> Observed;

    /// Whether the process with this id (as [`Behaviour::Grandchild`] wrote it) is still alive
    /// once the run returned; the real runner's factory waits a bounded time for it to go.
    fn survives(&self, id: &str) -> bool;

    /// Starts a long-lived framed child following `script` and reports the exchange.
    fn framed(&self, script: &FramedScript) -> FramedObserved;
}

/// A case the runner failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

/// The arguments [`argv_intact`] sends: spaces, both quotes, shell metacharacters, an empty one,
/// a leading dash, non-ASCII and a non-UTF-8 byte.
pub fn awkward_args() -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "plain",
        "with space",
        "\"double\" and 'single'",
        "",
        "$HOME `id` $(id) ; | & * ? > <",
        "-c",
        "zażółć",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(OsString::from_vec(vec![b'x', 0xff, b'y']));
    args
}

/// One case of the suite: `Err` says what the runner got wrong.
type Case = fn(&dyn RunnerUnderTest) -> Result<(), String>;

/// Runs every case and returns the violations; an empty list is a pass.
pub fn check(runner: &impl RunnerUnderTest) -> Vec<Violation> {
    let cases: [(&'static str, Case); 15] = [
        ("argv_intact", argv_intact),
        ("env_only_allowlist", env_only_allowlist),
        ("exit_code_kept", exit_code_kept),
        ("stdout_truncated_and_counted", stdout_truncated_and_counted),
        ("stderr_truncated_and_counted", stderr_truncated_and_counted),
        ("under_limit_not_truncated", under_limit_not_truncated),
        ("deadline_kills", deadline_kills),
        (
            "missing_program_fails_to_start",
            missing_program_fails_to_start,
        ),
        ("empty_argv_refused", empty_argv_refused),
        ("nul_refused", nul_refused),
        ("bare_without_path_refused", bare_without_path_refused),
        ("group_killed_with_grandchild", group_killed_with_grandchild),
        ("framed_round_trip", framed_round_trip),
        ("framed_oversized_refused", framed_oversized_refused),
        ("framed_deadline", framed_deadline),
    ];
    cases
        .into_iter()
        .filter_map(|(case, run)| run(runner).err().map(|detail| Violation { case, detail }))
        .collect()
}

fn exited(observed: Observed) -> Result<(Option<i32>, Stream, Stream), String> {
    match observed {
        Observed::Exited {
            code,
            stdout,
            stderr,
        } => Ok((code, stdout, stderr)),
        other => Err(format!("expected an exit, got {other:?}")),
    }
}

fn split_terminated(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let Some(body) = bytes.strip_suffix(b"\0") else {
        return if bytes.is_empty() {
            Ok(Vec::new())
        } else {
            Err(format!("output not NUL-terminated: {bytes:?}"))
        };
    };
    Ok(body.split(|b| *b == 0).map(<[u8]>::to_vec).collect())
}

fn argv_intact(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let mut invocation = Invocation::of(Behaviour::EchoArgs);
    invocation.args = awkward_args();
    let (code, stdout, _) = exited(runner.run(&invocation))?;
    let got = split_terminated(&stdout.bytes)?;
    let want: Vec<Vec<u8>> = invocation
        .args
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    if code != Some(0) || got != want {
        return Err(format!("code {code:?}, arguments {got:?}, want {want:?}"));
    }
    Ok(())
}

fn env_only_allowlist(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let mut invocation = Invocation::of(Behaviour::PrintEnv);
    invocation.parent_env = BTreeMap::from([
        (OsString::from("KEEP"), OsString::from("kept value")),
        (
            OsString::from("SECRET_TOKEN"),
            OsString::from("must not leak"),
        ),
        (OsString::from("HOME"), OsString::from("/home/parent")),
    ]);
    invocation.inherit_env = vec![OsString::from("KEEP"), OsString::from("ABSENT")];
    invocation.set_env = vec![(OsString::from("SET"), OsString::from("a=b c"))];
    let (code, stdout, _) = exited(runner.run(&invocation))?;
    let got = split_terminated(&stdout.bytes)?;
    let want = vec![b"KEEP=kept value".to_vec(), b"SET=a=b c".to_vec()];
    if code != Some(0) || got != want {
        return Err(format!(
            "code {code:?}, environment {:?}, want {:?}",
            lossy(&got),
            lossy(&want)
        ));
    }
    Ok(())
}

fn lossy(lines: &[Vec<u8>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect()
}

fn exit_code_kept(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let (code, _, _) = exited(runner.run(&Invocation::of(Behaviour::Exit(3))))?;
    if code != Some(3) {
        return Err(format!("code {code:?}, want Some(3)"));
    }
    Ok(())
}

fn volume(
    runner: &dyn RunnerUnderTest,
    stdout: usize,
    stderr: usize,
) -> Result<(Stream, Stream), String> {
    let mut invocation = Invocation::of(Behaviour::Write { stdout, stderr });
    invocation.stdout_limit = 1000;
    invocation.stderr_limit = 500;
    let (code, out, err) = exited(runner.run(&invocation))?;
    if code != Some(0) {
        return Err(format!("code {code:?}, want Some(0)"));
    }
    Ok((out, err))
}

fn expect_stream(
    name: &str,
    stream: &Stream,
    byte: u8,
    kept: usize,
    dropped: u64,
) -> Result<(), String> {
    if stream.bytes.len() != kept
        || stream.bytes.iter().any(|b| *b != byte)
        || stream.dropped != dropped
    {
        return Err(format!(
            "{name}: kept {} bytes, dropped {}; want {kept} bytes of {:?}, {dropped} dropped",
            stream.bytes.len(),
            stream.dropped,
            char::from(byte)
        ));
    }
    Ok(())
}

fn stdout_truncated_and_counted(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let (out, err) = volume(runner, 300_000, 10)?;
    expect_stream("stdout", &out, b'o', 1000, 299_000)?;
    expect_stream("stderr", &err, b'e', 10, 0)
}

fn stderr_truncated_and_counted(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let (out, err) = volume(runner, 10, 300_000)?;
    expect_stream("stdout", &out, b'o', 10, 0)?;
    expect_stream("stderr", &err, b'e', 500, 299_500)
}

fn under_limit_not_truncated(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let (out, err) = volume(runner, 1000, 500)?;
    expect_stream("stdout", &out, b'o', 1000, 0)?;
    expect_stream("stderr", &err, b'e', 500, 0)
}

fn deadline_kills(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let mut invocation = Invocation::of(Behaviour::Hang);
    invocation.deadline = Duration::from_millis(200);
    match runner.run(&invocation) {
        Observed::KilledOnDeadline { .. } => Ok(()),
        other => Err(format!("expected a kill at the deadline, got {other:?}")),
    }
}

fn missing_program_fails_to_start(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    match runner.run(&Invocation::of(Behaviour::Missing)) {
        Observed::FailedToStart => Ok(()),
        other => Err(format!("expected a failure to start, got {other:?}")),
    }
}

fn empty_argv_refused(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    match runner.run(&Invocation::of(Behaviour::EmptyArgv)) {
        Observed::Refused => Ok(()),
        other => Err(format!("expected a refusal, got {other:?}")),
    }
}

fn refused(runner: &dyn RunnerUnderTest, behaviour: Behaviour) -> Result<(), String> {
    match runner.run(&Invocation::of(behaviour)) {
        Observed::Refused => Ok(()),
        other => Err(format!("expected a refusal, got {other:?}")),
    }
}

fn nul_refused(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    refused(runner, Behaviour::NulInArgument)
}

fn bare_without_path_refused(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    refused(runner, Behaviour::BareWithoutPath)
}

fn group_killed_with_grandchild(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let mut invocation = Invocation::of(Behaviour::Grandchild);
    invocation.deadline = Duration::from_millis(300);
    let stdout = match runner.run(&invocation) {
        Observed::KilledOnDeadline { stdout, .. } => stdout,
        other => return Err(format!("expected a kill at the deadline, got {other:?}")),
    };
    let text = String::from_utf8_lossy(&stdout.bytes).into_owned();
    let id = text.lines().next().unwrap_or_default().trim().to_owned();
    if id.is_empty() {
        return Err(format!("no grandchild id in {text:?}"));
    }
    if runner.survives(&id) {
        return Err(format!("grandchild {id} survived the kill"));
    }
    Ok(())
}

fn framed_round_trip(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    let frames = vec![b"hello".to_vec(), Vec::new(), vec![7_u8; 200_000]];
    match runner.framed(&FramedScript::Echo(frames.clone())) {
        FramedObserved::Replies(replies) if replies == frames => Ok(()),
        other => Err(format!("expected the frames echoed, got {other:?}")),
    }
}

fn framed_oversized_refused(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    match runner.framed(&FramedScript::OversizedReply) {
        FramedObserved::FrameTooLarge => Ok(()),
        other => Err(format!(
            "expected an oversized frame refused, got {other:?}"
        )),
    }
}

fn framed_deadline(runner: &dyn RunnerUnderTest) -> Result<(), String> {
    match runner.framed(&FramedScript::Silent) {
        FramedObserved::Deadline => Ok(()),
        other => Err(format!("expected the deadline, got {other:?}")),
    }
}

/// The scripted fake: interprets each [`Behaviour`] and [`FramedScript`] with the rules a real
/// runner must keep (the allowlist, the limits, the deadline, the refusals). It speaks this
/// suite's vocabulary, not `jfl_process`'s types, and cannot be told to fail: it exists so the
/// suite itself can be checked against a known-good runner and the broken ones of its tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptedRunner;

impl ScriptedRunner {
    fn capture(bytes: Vec<u8>, limit: usize) -> Stream {
        let kept = bytes.len().min(limit);
        let dropped = u64::try_from(bytes.len().saturating_sub(kept)).unwrap_or(u64::MAX);
        let mut bytes = bytes;
        bytes.truncate(kept);
        Stream { bytes, dropped }
    }

    fn environment(invocation: &Invocation) -> BTreeMap<OsString, OsString> {
        let mut env = BTreeMap::new();
        for name in &invocation.inherit_env {
            if let Some(value) = invocation.parent_env.get(name) {
                env.insert(name.clone(), value.clone());
            }
        }
        for (name, value) in &invocation.set_env {
            env.insert(name.clone(), value.clone());
        }
        env
    }

    fn exited(invocation: &Invocation, stdout: Vec<u8>, stderr: Vec<u8>) -> Observed {
        Observed::Exited {
            code: Some(0),
            stdout: Self::capture(stdout, invocation.stdout_limit),
            stderr: Self::capture(stderr, invocation.stderr_limit),
        }
    }
}

impl RunnerUnderTest for ScriptedRunner {
    fn run(&self, invocation: &Invocation) -> Observed {
        match &invocation.behaviour {
            Behaviour::EchoArgs => {
                let mut out = Vec::new();
                for arg in &invocation.args {
                    out.extend_from_slice(arg.as_bytes());
                    out.push(0);
                }
                Self::exited(invocation, out, Vec::new())
            }
            Behaviour::PrintEnv => {
                let mut out = Vec::new();
                for (name, value) in Self::environment(invocation) {
                    out.extend_from_slice(name.as_bytes());
                    out.push(b'=');
                    out.extend_from_slice(value.as_bytes());
                    out.push(0);
                }
                Self::exited(invocation, out, Vec::new())
            }
            Behaviour::Write { stdout, stderr } => {
                Self::exited(invocation, vec![b'o'; *stdout], vec![b'e'; *stderr])
            }
            Behaviour::Exit(code) => Observed::Exited {
                code: Some(i32::from(*code)),
                stdout: Stream::default(),
                stderr: Stream::default(),
            },
            Behaviour::Hang => Observed::KilledOnDeadline {
                stdout: Stream::default(),
                stderr: Stream::default(),
            },
            Behaviour::Missing => Observed::FailedToStart,
            Behaviour::EmptyArgv | Behaviour::NulInArgument | Behaviour::BareWithoutPath => {
                Observed::Refused
            }
            Behaviour::Grandchild => Observed::KilledOnDeadline {
                stdout: Stream {
                    bytes: b"4242\n".to_vec(),
                    dropped: 0,
                },
                stderr: Stream::default(),
            },
        }
    }

    fn survives(&self, _id: &str) -> bool {
        false
    }

    fn framed(&self, script: &FramedScript) -> FramedObserved {
        match script {
            FramedScript::Echo(frames) => FramedObserved::Replies(frames.clone()),
            FramedScript::OversizedReply => FramedObserved::FrameTooLarge,
            FramedScript::Silent => FramedObserved::Deadline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts of [`RunnerUnderTest`] a broken runner leaves to the scripted fake.
    macro_rules! scripted_rest {
        () => {
            fn survives(&self, id: &str) -> bool {
                ScriptedRunner.survives(id)
            }
            fn framed(&self, script: &FramedScript) -> FramedObserved {
                ScriptedRunner.framed(script)
            }
        };
    }

    /// Passes the parent's whole environment, as `Command` does without `env_clear`.
    struct LeakyEnv;
    impl RunnerUnderTest for LeakyEnv {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            let mut leaky = invocation.clone();
            leaky.inherit_env = leaky.parent_env.keys().cloned().collect();
            ScriptedRunner.run(&leaky)
        }
    }

    /// Joins the argv and splits it on spaces, as a shell would.
    struct ShellSplit;
    impl RunnerUnderTest for ShellSplit {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            let joined = invocation
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let mut split = invocation.clone();
            split.args = joined.split(' ').map(OsString::from).collect();
            ScriptedRunner.run(&split)
        }
    }

    /// Keeps the cap but forgets to count what it dropped.
    struct Uncounted;
    impl RunnerUnderTest for Uncounted {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            match ScriptedRunner.run(invocation) {
                Observed::Exited {
                    code,
                    mut stdout,
                    mut stderr,
                } => {
                    stdout.dropped = 0;
                    stderr.dropped = 0;
                    Observed::Exited {
                        code,
                        stdout,
                        stderr,
                    }
                }
                other => other,
            }
        }
    }

    /// Lets a child outlive its deadline and reports it as an exit.
    struct NoDeadline;
    impl RunnerUnderTest for NoDeadline {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            match &invocation.behaviour {
                Behaviour::Hang => Observed::Exited {
                    code: None,
                    stdout: Stream::default(),
                    stderr: Stream::default(),
                },
                _ => ScriptedRunner.run(invocation),
            }
        }
    }

    /// Runs an empty argv as if it were an empty program, which then fails to start.
    struct AcceptsEmptyArgv;
    impl RunnerUnderTest for AcceptsEmptyArgv {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            match &invocation.behaviour {
                Behaviour::EmptyArgv => Observed::FailedToStart,
                _ => ScriptedRunner.run(invocation),
            }
        }
    }

    /// Kills the leader but lets its grandchild live, as a kill by pid alone would.
    struct KeepsGrandchild;
    impl RunnerUnderTest for KeepsGrandchild {
        fn run(&self, invocation: &Invocation) -> Observed {
            ScriptedRunner.run(invocation)
        }
        fn survives(&self, _id: &str) -> bool {
            true
        }
        fn framed(&self, script: &FramedScript) -> FramedObserved {
            ScriptedRunner.framed(script)
        }
    }

    /// Hands a NUL to the OS, which then fails to start the program.
    struct AcceptsNul;
    impl RunnerUnderTest for AcceptsNul {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            match &invocation.behaviour {
                Behaviour::NulInArgument => Observed::FailedToStart,
                _ => ScriptedRunner.run(invocation),
            }
        }
    }

    /// Searches a default path when the child has none.
    struct SearchesDefaultPath;
    impl RunnerUnderTest for SearchesDefaultPath {
        scripted_rest!();
        fn run(&self, invocation: &Invocation) -> Observed {
            match &invocation.behaviour {
                Behaviour::BareWithoutPath => Observed::FailedToStart,
                _ => ScriptedRunner.run(invocation),
            }
        }
    }

    /// Allocates whatever length the child declares.
    struct NoFrameLimit;
    impl RunnerUnderTest for NoFrameLimit {
        fn run(&self, invocation: &Invocation) -> Observed {
            ScriptedRunner.run(invocation)
        }
        fn survives(&self, id: &str) -> bool {
            ScriptedRunner.survives(id)
        }
        fn framed(&self, script: &FramedScript) -> FramedObserved {
            match script {
                FramedScript::OversizedReply => FramedObserved::Other("out of memory".to_owned()),
                other => ScriptedRunner.framed(other),
            }
        }
    }

    /// Waits for a framed reply for ever.
    struct NoFramedDeadline;
    impl RunnerUnderTest for NoFramedDeadline {
        fn run(&self, invocation: &Invocation) -> Observed {
            ScriptedRunner.run(invocation)
        }
        fn survives(&self, id: &str) -> bool {
            ScriptedRunner.survives(id)
        }
        fn framed(&self, script: &FramedScript) -> FramedObserved {
            match script {
                FramedScript::Silent => FramedObserved::Other("still waiting".to_owned()),
                other => ScriptedRunner.framed(other),
            }
        }
    }

    /// Splits a framed reply in two.
    struct SplitsFrames;
    impl RunnerUnderTest for SplitsFrames {
        fn run(&self, invocation: &Invocation) -> Observed {
            ScriptedRunner.run(invocation)
        }
        fn survives(&self, id: &str) -> bool {
            ScriptedRunner.survives(id)
        }
        fn framed(&self, script: &FramedScript) -> FramedObserved {
            match script {
                FramedScript::Echo(frames) => FramedObserved::Replies(
                    frames
                        .iter()
                        .flat_map(|f| f.chunks(100_000).map(<[u8]>::to_vec))
                        .collect(),
                ),
                other => ScriptedRunner.framed(other),
            }
        }
    }

    fn failed_cases(runner: &impl RunnerUnderTest) -> Vec<&'static str> {
        check(runner).into_iter().map(|v| v.case).collect()
    }

    #[test]
    fn should_pass_every_case_when_the_runner_is_the_scripted_fake() {
        // given
        let runner = ScriptedRunner;

        // when
        let violations = check(&runner);

        // then
        assert_eq!(violations, []);
    }

    #[test]
    fn should_fail_the_allowlist_case_when_the_runner_leaks_the_environment() {
        // given
        let runner = LeakyEnv;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["env_only_allowlist"]);
    }

    #[test]
    fn should_fail_the_argv_case_when_the_runner_splits_like_a_shell() {
        // given
        let runner = ShellSplit;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["argv_intact"]);
    }

    #[test]
    fn should_fail_the_truncation_cases_when_the_runner_does_not_count_drops() {
        // given
        let runner = Uncounted;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(
            failed,
            [
                "stdout_truncated_and_counted",
                "stderr_truncated_and_counted"
            ]
        );
    }

    #[test]
    fn should_fail_the_deadline_case_when_the_runner_lets_a_child_outlive_it() {
        // given
        let runner = NoDeadline;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["deadline_kills"]);
    }

    #[test]
    fn should_fail_the_refusal_case_when_the_runner_accepts_an_empty_argv() {
        // given
        let runner = AcceptsEmptyArgv;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["empty_argv_refused"]);
    }

    #[test]
    fn should_fail_the_cleanup_case_when_the_grandchild_survives() {
        // given
        let runner = KeepsGrandchild;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["group_killed_with_grandchild"]);
    }

    #[test]
    fn should_fail_the_nul_case_when_the_runner_passes_a_nul_on() {
        // given
        let runner = AcceptsNul;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["nul_refused"]);
    }

    #[test]
    fn should_fail_the_path_case_when_the_runner_searches_a_default_path() {
        // given
        let runner = SearchesDefaultPath;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["bare_without_path_refused"]);
    }

    #[test]
    fn should_fail_the_frame_limit_case_when_the_runner_allocates_any_length() {
        // given
        let runner = NoFrameLimit;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["framed_oversized_refused"]);
    }

    #[test]
    fn should_fail_the_framed_deadline_case_when_the_runner_waits_for_ever() {
        // given
        let runner = NoFramedDeadline;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["framed_deadline"]);
    }

    #[test]
    fn should_fail_the_round_trip_case_when_the_runner_splits_frames() {
        // given
        let runner = SplitsFrames;

        // when
        let failed = failed_cases(&runner);

        // then
        assert_eq!(failed, ["framed_round_trip"]);
    }
}
