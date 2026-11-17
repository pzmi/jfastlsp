//! The `process_runner` contract suite (`jfl_testkit::contracts::process_runner`, ver §5)
//! against the real runner and real children.

use std::ffi::OsString;
use std::time::Duration;

use jfl_process::{
    Command, Ending, Exit, FramedError, FramedOptions, OutputLimits, Refusal, RunError, Runner,
};
use jfl_testkit::contracts::process_runner::{
    Behaviour, FramedObserved, FramedScript, Invocation, Observed, RunnerUnderTest, Stream, check,
};
use jfl_testkit::poll::wait_until;

#[cfg(test)]
mod support;
use support::{CHILD, GENEROUS, is_gone};

#[cfg(test)]
mod real {
    use super::*;
    /// The real runner, with the suite's parent environment injected and each behaviour mapped to
    /// the test child's argv.
    pub(super) struct Real;

    impl RunnerUnderTest for Real {
        fn survives(&self, id: &str) -> bool {
            wait_until(GENEROUS, "the grandchild to be gone", || is_gone(id)).is_err()
        }

        fn framed(&self, script: &FramedScript) -> FramedObserved {
            let mode = match script {
                FramedScript::Echo(_) => "echo-frames",
                FramedScript::OversizedReply => "oversized-frame",
                FramedScript::Silent => "hang",
            };
            let command = Command::new(CHILD).arg(mode);
            let mut child = match Runner::new().start_framed(&command, FramedOptions::default()) {
                Ok(child) => child,
                Err(error) => return FramedObserved::Other(error.to_string()),
            };
            let observed = match script {
                FramedScript::Echo(frames) => frames
                    .iter()
                    .map(|frame| child.request(frame, GENEROUS))
                    .collect::<Result<Vec<_>, _>>()
                    .map(FramedObserved::Replies),
                FramedScript::OversizedReply => child
                    .recv(GENEROUS)
                    .map(|_| FramedObserved::Other("an oversized frame was accepted".to_owned())),
                FramedScript::Silent => child
                    .recv(Duration::from_millis(200))
                    .map(|_| FramedObserved::Other("a silent child answered".to_owned())),
            };
            match observed {
                Ok(observed) => observed,
                Err(FramedError::FrameTooLarge { .. }) => FramedObserved::FrameTooLarge,
                Err(FramedError::Deadline) => FramedObserved::Deadline,
                Err(other) => FramedObserved::Other(other.to_string()),
            }
        }

        fn run(&self, invocation: &Invocation) -> Observed {
            let argv: Vec<OsString> = match &invocation.behaviour {
                Behaviour::EchoArgs => vec![CHILD.into(), "args".into()],
                Behaviour::PrintEnv => vec![CHILD.into(), "env".into()],
                Behaviour::Write { stdout, stderr } => vec![
                    CHILD.into(),
                    "write".into(),
                    stdout.to_string().into(),
                    stderr.to_string().into(),
                ],
                Behaviour::Exit(code) => vec![CHILD.into(), "exit".into(), code.to_string().into()],
                Behaviour::Hang => vec![CHILD.into(), "hang".into()],
                Behaviour::Missing => vec!["/nonexistent/jfl-process-contract/missing".into()],
                Behaviour::EmptyArgv => Vec::new(),
                Behaviour::NulInArgument => vec![CHILD.into(), "args".into(), "a\0b".into()],
                Behaviour::BareWithoutPath => vec!["jfl-process-test-child".into()],
                Behaviour::Grandchild => vec![CHILD.into(), "grandchild".into()],
            };
            let command = match Command::from_argv(argv) {
                Ok(command) => command,
                Err(Refusal::EmptyArgv) => return Observed::Refused,
                Err(other) => panic!("unexpected refusal {other}"),
            };
            let mut command =
                command
                    .args(invocation.args.iter().cloned())
                    .output_limits(OutputLimits {
                        stdout: invocation.stdout_limit,
                        stderr: invocation.stderr_limit,
                    });
            for name in &invocation.inherit_env {
                command = command.inherit_env(name.clone());
            }
            for (name, value) in &invocation.set_env {
                command = command.env(name.clone(), value.clone());
            }
            let runner = Runner::with_env(invocation.parent_env.clone());
            match runner.run(&command, invocation.deadline) {
                Ok(outcome) => {
                    let stdout = Stream {
                        dropped: outcome.stdout.dropped(),
                        bytes: outcome.stdout.into_bytes(),
                    };
                    let stderr = Stream {
                        dropped: outcome.stderr.dropped(),
                        bytes: outcome.stderr.into_bytes(),
                    };
                    match outcome.ending {
                        Ending::Exited(Exit::Code(code)) => Observed::Exited {
                            code: Some(code),
                            stdout,
                            stderr,
                        },
                        Ending::Exited(_) => Observed::Exited {
                            code: None,
                            stdout,
                            stderr,
                        },
                        Ending::KilledOnDeadline => Observed::KilledOnDeadline { stdout, stderr },
                    }
                }
                Err(RunError::FailedToStart { .. }) => Observed::FailedToStart,
                Err(RunError::Refused(_)) => Observed::Refused,
                Err(other) => panic!("unexpected error {other}"),
            }
        }
    }
}

#[test]
fn should_pass_the_process_runner_contract_when_running_real_children() {
    // given
    let runner = real::Real;

    // when
    let violations = check(&runner);

    // then
    assert_eq!(violations, []);
}
