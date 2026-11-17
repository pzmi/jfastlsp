//! The long-lived mode: a child spoken to in length-prefixed frames over its stdin and stdout
//! (the javac worker and the Gradle model helper, plan §3.1, ADR 0036).
//!
//! A frame is a 4-byte big-endian length followed by that many bytes. The length a child
//! declares is checked against the limit before anything is allocated for it (plan §11). Every
//! exchange has a deadline; any failure (deadline, oversized frame, end of stream, I/O error)
//! kills the child's group and reaps it, because a stream cut mid-frame cannot be resynchronised.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::process::{ChildStdin, ChildStdout, Stdio};
use std::time::{Duration, Instant};

use rustix::event::PollFlags;

use crate::command::Checked;
use crate::error::{FramedError, RunError};
use crate::run::{Exit, Group, failed_to_start, std_command};
use crate::sys::{self, Leader};

/// The default frame limit: 16 MiB, the daemon protocol's own frame limit (plan §2.5).
pub const DEFAULT_MAX_FRAME: u32 = 16 << 20;

/// The length prefix's size.
const HEADER: usize = 4;

/// Where a long-lived child's stderr goes. It is never a pipe: nobody would drain it between
/// exchanges, and a full pipe would stall the child.
#[derive(Debug)]
pub enum StderrSink {
    /// `/dev/null`.
    Null,
    /// A descriptor the caller opened (a log file).
    Fd(OwnedFd),
}

/// How a long-lived child is started.
#[derive(Debug)]
pub struct FramedOptions {
    /// The longest frame either side may send.
    pub max_frame: u32,
    /// Where its stderr goes.
    pub stderr: StderrSink,
}

impl Default for FramedOptions {
    fn default() -> Self {
        Self {
            max_frame: DEFAULT_MAX_FRAME,
            stderr: StderrSink::Null,
        }
    }
}

/// A long-lived child in its own process group. Dropping it kills the group and reaps the leader.
///
/// Pinned by `tests/framed.rs`: `should_round_trip_frames_when_the_child_echoes_them`,
/// `should_refuse_before_allocating_when_the_child_declares_an_oversized_frame`,
/// `should_refuse_a_payload_over_the_limit_when_sending_and_keep_the_child`,
/// `should_kill_the_child_when_a_reply_misses_its_deadline`,
/// `should_kill_the_child_when_a_send_misses_its_deadline`,
/// `should_report_the_exit_when_the_child_ends_the_stream` and
/// `should_kill_the_group_when_the_handle_is_dropped`.
#[derive(Debug)]
pub struct FramedChild {
    leader: Leader,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    max_frame: u32,
    dead: bool,
}

impl FramedChild {
    pub(crate) fn start(checked: &Checked, options: FramedOptions) -> Result<Self, RunError> {
        let stderr = match options.stderr {
            StderrSink::Null => Stdio::null(),
            StderrSink::Fd(fd) => Stdio::from(fd),
        };
        let mut child = std_command(checked, Group::Own)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .map_err(|source| failed_to_start(checked, source))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let leader = Leader::new(child);
        for fd in [sys::borrow(stdin.as_ref()), sys::borrow(stdout.as_ref())]
            .into_iter()
            .flatten()
        {
            rustix::io::ioctl_fionbio(fd, true).map_err(|error| RunError::Wait(error.into()))?;
        }
        Ok(Self {
            leader,
            stdin,
            stdout,
            max_frame: options.max_frame,
            dead: false,
        })
    }

    /// The child's pid, which is also its process group id.
    pub fn id(&self) -> u32 {
        self.leader.id()
    }

    /// Sends one frame, within `timeout`.
    pub fn send(&mut self, payload: &[u8], timeout: Duration) -> Result<(), FramedError> {
        self.send_by(payload, Instant::now(), timeout)
    }

    /// Receives one frame, within `timeout`.
    pub fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>, FramedError> {
        self.recv_by(Instant::now(), timeout)
    }

    /// Sends one frame and receives the reply, both within `timeout`.
    pub fn request(&mut self, payload: &[u8], timeout: Duration) -> Result<Vec<u8>, FramedError> {
        let start = Instant::now();
        self.send_by(payload, start, timeout)?;
        self.recv_by(start, timeout)
    }

    /// Ends the child: closes its stdin, gives it until `timeout` to close its stdout (its own
    /// clean exit), then kills its group and reaps it.
    pub fn close(mut self, timeout: Duration) -> Result<Exit, FramedError> {
        if self.dead {
            return Err(FramedError::Dead);
        }
        let start = Instant::now();
        self.stdin = None;
        let mut sink = [0_u8; 4096];
        while let Some(stdout) = self.stdout.as_mut() {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            let [ready] = sys::wait_ready([Some(stdout.as_fd())], [PollFlags::IN], remaining)
                .map_err(FramedError::Io)?;
            if ready {
                match stdout.read(&mut sink) {
                    Ok(0) => self.stdout = None,
                    Ok(_) => {}
                    Err(error) if retryable(&error) => {}
                    Err(error) => return Err(FramedError::Io(error)),
                }
            }
        }
        self.dead = true;
        self.leader.kill_and_reap().map_err(FramedError::Io)
    }

    fn send_by(
        &mut self,
        payload: &[u8],
        start: Instant,
        timeout: Duration,
    ) -> Result<(), FramedError> {
        if self.dead {
            return Err(FramedError::Dead);
        }
        let max = self.max_frame;
        let len = u32::try_from(payload.len())
            .ok()
            .filter(|len| *len <= max)
            .ok_or(FramedError::PayloadTooLarge {
                len: payload.len(),
                max,
            })?;
        let mut frame = Vec::with_capacity(payload.len().saturating_add(HEADER));
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(payload);
        let written = self.write_all_by(&frame, start, timeout);
        written.map_err(|error| self.fail(error))
    }

    fn recv_by(&mut self, start: Instant, timeout: Duration) -> Result<Vec<u8>, FramedError> {
        if self.dead {
            return Err(FramedError::Dead);
        }
        let received = self.read_frame_by(start, timeout);
        received.map_err(|error| self.fail(error))
    }

    fn read_frame_by(&mut self, start: Instant, timeout: Duration) -> Result<Vec<u8>, Failure> {
        let mut header = [0_u8; HEADER];
        self.read_exact_by(&mut header, start, timeout)?;
        let declared = u32::from_be_bytes(header);
        if declared > self.max_frame {
            return Err(Failure::Refused(FramedError::FrameTooLarge {
                declared: u64::from(declared),
                max: self.max_frame,
            }));
        }
        let len = usize::try_from(declared).map_err(|_| {
            Failure::Refused(FramedError::FrameTooLarge {
                declared: u64::from(declared),
                max: self.max_frame,
            })
        })?;
        let mut payload = vec![0_u8; len];
        self.read_exact_by(&mut payload, start, timeout)?;
        Ok(payload)
    }

    fn read_exact_by(
        &mut self,
        out: &mut [u8],
        start: Instant,
        timeout: Duration,
    ) -> Result<(), Failure> {
        let mut filled = 0_usize;
        while let Some(rest) = out.get_mut(filled..).filter(|rest| !rest.is_empty()) {
            let stdout = self.stdout.as_mut().ok_or(Failure::Ended)?;
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(Failure::Refused(FramedError::Deadline));
            }
            let [ready] = sys::wait_ready([Some(stdout.as_fd())], [PollFlags::IN], remaining)
                .map_err(Failure::Io)?;
            if !ready {
                continue;
            }
            match stdout.read(rest) {
                Ok(0) => return Err(Failure::Ended),
                Ok(n) => filled = filled.saturating_add(n),
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(Failure::Io(error)),
            }
        }
        Ok(())
    }

    fn write_all_by(
        &mut self,
        mut bytes: &[u8],
        start: Instant,
        timeout: Duration,
    ) -> Result<(), Failure> {
        while !bytes.is_empty() {
            let stdin = self.stdin.as_mut().ok_or(Failure::Ended)?;
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(Failure::Refused(FramedError::Deadline));
            }
            let [ready] = sys::wait_ready([Some(stdin.as_fd())], [PollFlags::OUT], remaining)
                .map_err(Failure::Io)?;
            if !ready {
                continue;
            }
            match stdin.write(bytes) {
                Ok(0) => return Err(Failure::Ended),
                Ok(n) => bytes = bytes.get(n..).unwrap_or_default(),
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                    return Err(Failure::Ended);
                }
                Err(error) if retryable(&error) => {}
                Err(error) => return Err(Failure::Io(error)),
            }
        }
        Ok(())
    }

    /// Kills the group, reaps the leader, and says why.
    fn fail(&mut self, failure: Failure) -> FramedError {
        self.dead = true;
        self.stdin = None;
        self.stdout = None;
        let reaped = self.leader.kill_and_reap();
        match (failure, reaped) {
            (Failure::Ended, Ok(exit)) => FramedError::Exited(exit),
            (Failure::Refused(error), _) => error,
            (Failure::Io(error), _) | (Failure::Ended, Err(error)) => FramedError::Io(error),
        }
    }
}

/// Why an exchange stopped, before the child is killed.
enum Failure {
    /// The child closed its end.
    Ended,
    /// A deadline or a limit.
    Refused(FramedError),
    /// The OS failed.
    Io(io::Error),
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
    )
}
