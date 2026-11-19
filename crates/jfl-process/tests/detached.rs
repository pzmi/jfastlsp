//! The detached mode: the daemon launcher's.

use std::io::Read;
use std::os::fd::OwnedFd;

use jfl_process::{Command, Detached, Exit, Runner};
use jfl_testkit::poll::wait_until;

#[cfg(test)]
mod support;
use support::{CHILD, GENEROUS, is_gone, ps_state};

#[cfg(test)]
mod helpers {
    use super::*;

    pub(crate) fn spawn(command: &Command) -> (Detached, String) {
        let (mut log_reader, log_writer) = std::io::pipe().unwrap();
        let detached = Runner::new()
            .spawn_detached(command, OwnedFd::from(log_writer))
            .unwrap();
        let mut log = String::new();
        log_reader.read_to_string(&mut log).unwrap();
        (detached, log)
    }
}

#[test]
fn should_write_to_the_log_when_detached() {
    // given: a pipe stands in for the log file the launcher opens
    let command = Command::new(CHILD).args(["args", "to the log"]);

    // when
    let (mut detached, log) = helpers::spawn(&command);

    // then
    assert_eq!(log, "to the log\0");
    let mut exit = None;
    wait_until(GENEROUS, "the detached child to exit", || {
        exit = detached.try_exit().unwrap();
        exit.is_some()
    })
    .unwrap();
    assert_eq!(exit, Some(Exit::Code(0)));
    assert_eq!(ps_state(&detached.id().to_string()), None);
}

#[test]
fn should_let_the_child_call_setsid_when_detached() {
    // given: the daemon leaves the launcher's session itself (ADR 0021, amended 2026-11-17)
    let command = Command::new(CHILD).arg("setsid");

    // when
    let (detached, log) = helpers::spawn(&command);

    // then
    assert_eq!(log, "setsid ok\n");
    let pid = detached.id().to_string();
    wait_until(GENEROUS, "the detached child to exit", || is_gone(&pid)).unwrap();
}

#[test]
fn should_reap_an_exited_child_when_the_handle_is_dropped() {
    // given: a detached child that has exited and is a zombie
    let (detached, _log) = helpers::spawn(&Command::new(CHILD).args(["exit", "0"]));
    let pid = detached.id().to_string();
    wait_until(GENEROUS, "the detached child to be a zombie", || {
        ps_state(&pid).is_some_and(|state| state.starts_with('Z'))
    })
    .unwrap();

    // when
    drop(detached);

    // then
    assert_eq!(ps_state(&pid), None);
}

#[cfg(target_os = "linux")]
#[test]
fn should_signal_the_exit_descriptor_when_a_detached_child_exits() {
    // given
    let (detached, _log) = helpers::spawn(&Command::new(CHILD).args(["exit", "0"]));
    let exit = detached.exit_fd();

    // when
    let readable = || {
        let Some(fd) = exit else { return false };
        let mut polls = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let none = rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        rustix::event::poll(&mut polls, Some(&none)).is_ok_and(|n| n > 0)
    };
    let seen = wait_until(GENEROUS, "the exit descriptor to be readable", readable);

    // then
    assert!(exit.is_some());
    assert!(seen.is_ok(), "{seen:?}");
}

#[cfg(not(target_os = "linux"))]
#[test]
fn should_offer_no_exit_descriptor_when_the_platform_has_none() {
    // given
    let (mut detached, _log) = helpers::spawn(&Command::new(CHILD).args(["exit", "0"]));

    // when
    let exit = detached.exit_fd().is_some();
    wait_until(GENEROUS, "the detached child to exit", || {
        detached.try_exit().unwrap().is_some()
    })
    .unwrap();

    // then
    assert!(!exit);
}
