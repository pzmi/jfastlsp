//! The one-shot mode: start, capture capped output, end by exit or by the deadline.

use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::process::Stdio;
use std::time::{Duration, Instant};

use rustix::event::PollFlags;

use crate::command::{Checked, Command};
use crate::detached::Detached;
use crate::env::{EnvSource, ProcessEnv};
use crate::error::RunError;
use crate::framed::{FramedChild, FramedOptions};
use crate::sys::{self, Leader};

/// The size of one read from a pipe: a Linux pipe's default capacity.
const READ_CHUNK: usize = 64 * 1024;

/// The most reads a drain makes after the group is killed: 1 MiB, Linux's largest unprivileged
/// pipe buffer. What a killed group left in the pipe fits; a descendant that escaped the group
/// (`setsid`) and keeps writing cannot hold the run past it.
const DRAIN_READS: usize = 16;

/// How a child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// It exited with this code.
    Code(i32),
    /// A signal ended it.
    Signal(i32),
    /// Neither, as the raw wait status (not expected from an exited child).
    Other(i32),
}

impl Exit {
    /// Whether it exited with code 0.
    pub fn success(self) -> bool {
        self == Self::Code(0)
    }
}

/// How a one-shot run ended: by itself, or killed with its group at the deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The leader ended before the deadline (its group was then killed).
    Exited(Exit),
    /// The deadline passed first; the group was killed with `SIGKILL`.
    KilledOnDeadline,
}

/// What a run kept of one output stream: at most its limit, and how many bytes past it were read
/// and dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    bytes: Vec<u8>,
    dropped: u64,
}

impl Captured {
    /// The bytes kept, the first ones the child wrote.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The bytes kept, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// How many bytes past the limit were read and dropped.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether anything was dropped.
    pub fn is_truncated(&self) -> bool {
        self.dropped > 0
    }
}

/// The result of a one-shot run that started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How it ended.
    pub ending: Ending,
    /// Its stdout, capped.
    pub stdout: Captured,
    /// Its stderr, capped.
    pub stderr: Captured,
    /// From the start to the leader being reaped.
    pub elapsed: Duration,
}

/// Starts children (the crate's only entry point), reading inherited variables from `E`.
#[derive(Debug, Clone, Default)]
pub struct Runner<E = ProcessEnv> {
    env: E,
}

impl Runner<ProcessEnv> {
    /// A runner over the process's own environment.
    pub fn new() -> Self {
        Self { env: ProcessEnv }
    }
}

impl<E: EnvSource> Runner<E> {
    /// A runner reading inherited variables from `env`.
    pub fn with_env(env: E) -> Self {
        Self { env }
    }

    /// Runs `command` to its end or to `deadline` from the call (the spawn counts against it),
    /// whichever comes first, keeping each output stream up to its limit.
    ///
    /// When the leader exits, the rest of its group is killed before it is reaped, so no
    /// grandchild outlives the run; what the pipes already hold is still read. At the deadline
    /// the group is killed with `SIGKILL` (a child that ignores `SIGTERM` cannot hold it off).
    /// Pinned by `tests/run.rs` and the `process_runner` contract suite
    /// (`tests/contract.rs::should_pass_the_process_runner_contract_when_running_real_children`).
    pub fn run(&self, command: &Command, deadline: Duration) -> Result<Outcome, RunError> {
        let start = Instant::now();
        let checked = command.check(&self.env)?;
        let mut child = std_command(&checked, Group::Own)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| failed_to_start(&checked, source))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut leader = Leader::new(child);
        let mut out = Stream::new(stdout, checked.limits.stdout);
        let mut err = Stream::new(stderr, checked.limits.stderr);
        let ending =
            collect(&mut leader, &mut out, &mut err, start, deadline).map_err(RunError::Wait)?;
        Ok(Outcome {
            ending,
            stdout: out.captured,
            stderr: err.captured,
            elapsed: start.elapsed(),
        })
    }

    /// Starts `command` as a long-lived child spoken to in frames (see [`FramedChild`]).
    pub fn start_framed(
        &self,
        command: &Command,
        options: FramedOptions,
    ) -> Result<FramedChild, RunError> {
        let checked = command.check(&self.env)?;
        FramedChild::start(&checked, options)
    }

    /// Starts `command` detached: stdin from `/dev/null`, stdout and stderr to `log` (a file the
    /// caller opened), no deadline and no kill on drop. It is left in the caller's process group
    /// so that it can call `setsid` itself (see [`Detached`]).
    pub fn spawn_detached(&self, command: &Command, log: OwnedFd) -> Result<Detached, RunError> {
        let checked = command.check(&self.env)?;
        Detached::spawn(&checked, log)
    }
}

/// Whether a child leads its own process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    /// `process_group(0)`: the one-shot and framed modes, whose group is killed as a whole.
    Own,
    /// The parent's group: the detached mode, whose child calls `setsid` itself, which a group
    /// leader cannot (`EPERM`).
    Inherit,
}

/// The std command for a checked [`Command`]: its argv and an environment cleared and then given
/// only the allowlist, in its own process group or not.
pub(crate) fn std_command(checked: &Checked, group: Group) -> std::process::Command {
    use std::os::unix::process::CommandExt;

    let mut command = std::process::Command::new(&checked.program);
    command
        .args(&checked.args)
        .env_clear()
        .envs(checked.env.iter().map(|(name, value)| (name, value)));
    if group == Group::Own {
        command.process_group(0);
    }
    if let Some(cwd) = &checked.cwd {
        command.current_dir(cwd);
    }
    command
}

pub(crate) fn failed_to_start(checked: &Checked, source: io::Error) -> RunError {
    RunError::FailedToStart {
        program: checked.program.to_string_lossy().into_owned(),
        source,
    }
}

/// One output pipe and what was kept of it.
struct Stream<R> {
    pipe: Option<R>,
    limit: usize,
    captured: Captured,
}

impl<R: Read> Stream<R> {
    fn new(pipe: Option<R>, limit: usize) -> Self {
        Self {
            pipe,
            limit,
            captured: Captured::default(),
        }
    }

    /// One read after `poll` said the pipe is ready; end of file closes it.
    fn read_once(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let Some(pipe) = self.pipe.as_mut() else {
            return Ok(());
        };
        match pipe.read(buf) {
            Ok(0) => {
                self.pipe = None;
                Ok(())
            }
            Ok(n) => {
                self.absorb(buf.get(..n).unwrap_or_default());
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn absorb(&mut self, chunk: &[u8]) {
        let room = self.limit.saturating_sub(self.captured.bytes.len());
        let (kept, rest) = chunk.split_at(chunk.len().min(room));
        self.captured.bytes.extend_from_slice(kept);
        let rest = u64::try_from(rest.len()).unwrap_or(u64::MAX);
        self.captured.dropped = self.captured.dropped.saturating_add(rest);
    }
}

impl<R: Read + std::os::fd::AsFd> Stream<R> {
    /// Reads what the pipe holds right now, without waiting, in at most [`DRAIN_READS`] reads.
    fn drain(&mut self, buf: &mut [u8]) -> io::Result<()> {
        for _ in 0..DRAIN_READS {
            let Some(pipe) = self.pipe.as_ref() else {
                break;
            };
            let [ready] = sys::wait_ready([Some(pipe.as_fd())], [PollFlags::IN], Duration::ZERO)?;
            if !ready {
                break;
            }
            self.read_once(buf)?;
        }
        Ok(())
    }
}

/// Reads both pipes until the leader exits or the deadline passes, then kills the group and
/// reaps the leader. On an error the leader's drop kills and reaps.
fn collect<O, E>(
    leader: &mut Leader,
    out: &mut Stream<O>,
    err: &mut Stream<E>,
    start: Instant,
    deadline: Duration,
) -> io::Result<Ending>
where
    O: Read + std::os::fd::AsFd,
    E: Read + std::os::fd::AsFd,
{
    let exit = sys::exit_descriptor(leader.child_mut());
    let mut buf = vec![0_u8; READ_CHUNK];
    loop {
        let remaining = deadline.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            let exit_status = leader.kill_and_reap()?;
            out.drain(&mut buf)?;
            err.drain(&mut buf)?;
            return Ok(match exit_status {
                Exit::Signal(signal) if signal == rustix::process::Signal::KILL.as_raw() => {
                    Ending::KilledOnDeadline
                }
                ended => Ending::Exited(ended),
            });
        }
        let [out_ready, err_ready, _exited] = sys::wait_ready(
            [
                sys::borrow(out.pipe.as_ref()),
                sys::borrow(err.pipe.as_ref()),
                sys::borrow(exit.as_ref()),
            ],
            [PollFlags::IN; 3],
            sys::slice(remaining, exit.as_ref()),
        )?;
        if out_ready {
            out.read_once(&mut buf)?;
        }
        if err_ready {
            err.read_once(&mut buf)?;
        }
        if leader.has_exited()? {
            leader.kill_group();
            out.drain(&mut buf)?;
            err.drain(&mut buf)?;
            return leader.kill_and_reap().map(Ending::Exited);
        }
    }
}
