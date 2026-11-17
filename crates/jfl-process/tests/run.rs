//! The one-shot mode against real children: argv, environment, process group, deadline and
//! grandchildren.

use std::time::Duration;

use jfl_process::{Command, Ending, Exit, Outcome, Refusal, RunError, Runner};
use jfl_testkit::poll::wait_until;

#[cfg(test)]
mod support;
use support::{CHILD, GENEROUS, is_gone, run};

fn first_line(outcome: &Outcome) -> String {
    String::from_utf8_lossy(outcome.stdout.bytes())
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn should_deliver_every_argument_intact_when_it_holds_spaces_and_quotes() {
    // given
    let args = [
        "with space",
        "\"double\"",
        "'single'",
        "",
        "a;b|c&&d",
        "$(id)",
    ];
    let command = Command::new(CHILD).arg("args").args(args);

    // when
    let outcome = run(&command, GENEROUS);

    // then
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
    let got: Vec<&[u8]> = outcome
        .stdout
        .bytes()
        .strip_suffix(b"\0")
        .unwrap()
        .split(|b| *b == 0)
        .collect();
    let want: Vec<&[u8]> = args.iter().map(|a| a.as_bytes()).collect();
    assert_eq!(got, want);
}

#[test]
fn should_pass_only_the_allowlist_when_the_parent_has_other_variables() {
    // given: the test process's own environment, which holds more than the allowlist
    let path = std::env::var_os("PATH").unwrap();
    assert!(std::env::vars_os().count() > 2);
    let command = Command::new(CHILD)
        .arg("env")
        .inherit_env("PATH")
        .inherit_env("JFL_PROCESS_TEST_ABSENT")
        .env("LANG", "C");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
    let mut want = b"LANG=C\0PATH=".to_vec();
    want.extend_from_slice(path.as_encoded_bytes());
    want.push(0);
    assert_eq!(outcome.stdout.bytes(), want);
}

#[test]
fn should_kill_the_group_at_the_deadline_when_the_child_ignores_sigterm() {
    // given: `trap '' TERM` sets SIGTERM to ignored, which `exec` keeps for the test child; the
    // shell is the program under test here, given its script as one argv element. No safe Rust
    // API sets a signal disposition, so the fixture needs it.
    let command = Command::new("/bin/sh").args(["-c", "trap '' TERM; exec \"$0\" hang", CHILD]);
    let deadline = Duration::from_millis(300);

    // when
    let outcome = run(&command, deadline);

    // then
    assert_eq!(outcome.ending, Ending::KilledOnDeadline);
    assert!(outcome.elapsed >= deadline, "{:?}", outcome.elapsed);
    assert!(outcome.elapsed < GENEROUS, "{:?}", outcome.elapsed);
}

#[test]
fn should_leave_no_grandchild_when_the_deadline_kills_the_group() {
    // given
    let command = Command::new(CHILD).arg("grandchild");

    // when
    let outcome = run(&command, Duration::from_millis(500));

    // then
    assert_eq!(outcome.ending, Ending::KilledOnDeadline);
    let grandchild = first_line(&outcome);
    assert!(!grandchild.is_empty(), "the child printed no pid");
    wait_until(GENEROUS, "the killed grandchild to be gone", || {
        is_gone(&grandchild)
    })
    .unwrap();
}

#[test]
fn should_leave_no_grandchild_when_the_leader_exits_first() {
    // given: the grandchild keeps the output pipes open after the leader exits
    let command = Command::new(CHILD).arg("orphan");

    // when
    let outcome = run(&command, GENEROUS);

    // then: the run ends with the leader, not at the deadline, and takes the grandchild with it
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
    assert!(outcome.elapsed < GENEROUS / 2, "{:?}", outcome.elapsed);
    let grandchild = first_line(&outcome);
    assert!(!grandchild.is_empty(), "the child printed no pid");
    wait_until(GENEROUS, "the orphaned grandchild to be gone", || {
        is_gone(&grandchild)
    })
    .unwrap();
}

#[test]
fn should_return_with_the_leader_when_an_escaped_descendant_keeps_writing() {
    // given: a grandchild that left the group (out of the kill's reach) and floods stdout; it
    // stops on its own once the runner closes the pipe
    let command = Command::new(CHILD).arg("escape-and-flood");

    // when
    let outcome = run(&command, GENEROUS);

    // then: the run ends with the leader, not with the pipes' end of file, which the escaped
    // grandchild holds off
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
    assert!(outcome.elapsed < GENEROUS / 2, "{:?}", outcome.elapsed);
}

#[test]
fn should_kill_the_leader_at_the_deadline_when_it_left_its_group() {
    // given: the child moves into the test process's group, out of reach of the group kill
    let command = Command::new(CHILD).arg("join-parent-group");

    // when
    let outcome = run(&command, Duration::from_millis(300));

    // then
    assert_eq!(outcome.stderr.bytes(), b"moved\n");
    assert_eq!(outcome.ending, Ending::KilledOnDeadline);
    assert!(outcome.elapsed < GENEROUS / 4, "{:?}", outcome.elapsed);
}

#[test]
fn should_refuse_setsid_to_a_group_leader_when_run() {
    // given: a one-shot child leads its own group, which is why the detached mode does not
    // make its child one (ADR 0021, amended)
    let command = Command::new(CHILD).arg("setsid");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    let said = String::from_utf8_lossy(outcome.stdout.bytes()).into_owned();
    assert!(said.starts_with("setsid failed"), "{said}");
}

#[test]
fn should_refuse_a_planted_binary_when_path_has_a_relative_entry() {
    // given: the working directory holds an executable named like the bare program
    let dir = std::path::Path::new(CHILD).parent().unwrap();
    let planted = "jfl-process-test-child";
    let runner = Runner::new();

    // when
    let dot = runner.run(
        &Command::new(planted).env("PATH", ".").current_dir(dir),
        GENEROUS,
    );
    let trailing = runner.run(
        &Command::new(planted)
            .env("PATH", "/nonexistent:")
            .current_dir(dir),
        GENEROUS,
    );
    let absolute = runner.run(
        &Command::new(planted)
            .env("PATH", dir.as_os_str())
            .args(["exit", "4"])
            .current_dir("/"),
        GENEROUS,
    );

    // then: refused with a relative or empty entry; found with the absolute one (the control)
    assert!(
        matches!(
            dot,
            Err(RunError::Refused(Refusal::RelativePathEntry { .. }))
        ),
        "{dot:?}"
    );
    assert!(
        matches!(
            trailing,
            Err(RunError::Refused(Refusal::RelativePathEntry { .. }))
        ),
        "{trailing:?}"
    );
    assert_eq!(absolute.unwrap().ending, Ending::Exited(Exit::Code(4)));
}

#[test]
fn should_fail_to_start_when_the_childs_path_does_not_hold_the_program() {
    // given: the parent's PATH holds `true`; the child's does not
    let command = Command::new("true").env("PATH", "/nonexistent");

    // when
    let result = Runner::new().run(&command, GENEROUS);

    // then
    assert!(
        matches!(result, Err(RunError::FailedToStart { .. })),
        "{result:?}"
    );
}

#[test]
fn should_start_the_child_in_its_own_process_group_when_run() {
    // given
    let command = Command::new(CHILD).arg("pgid");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    let ours = rustix::process::getpgrp().as_raw_nonzero().to_string();
    let theirs = first_line(&outcome);
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
    assert!(!theirs.is_empty());
    assert_ne!(theirs, ours);
}

#[test]
fn should_report_the_signal_when_the_child_is_killed_by_one() {
    // given: the child sends itself SIGKILL before the deadline
    let command = Command::new(CHILD).arg("self-kill");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    assert_eq!(outcome.ending, Ending::Exited(Exit::Signal(9)));
}

#[test]
fn should_find_a_bare_program_when_path_is_allowlisted() {
    // given
    let command = Command::new("true").env("PATH", "/usr/bin:/bin");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    assert_eq!(outcome.ending, Ending::Exited(Exit::Code(0)));
}

#[test]
fn should_run_in_the_working_directory_when_one_is_given() {
    // given
    let command = Command::new("/bin/pwd").current_dir("/");

    // when
    let outcome = run(&command, GENEROUS);

    // then
    assert_eq!(outcome.stdout.bytes(), b"/\n");
}

#[test]
fn should_fail_to_start_when_the_working_directory_is_missing() {
    // given
    let command = Command::new(CHILD)
        .arg("exit")
        .arg("0")
        .current_dir("/nonexistent/jfl-process-test");

    // when
    let result = Runner::new().run(&command, GENEROUS);

    // then
    assert!(
        matches!(result, Err(RunError::FailedToStart { .. })),
        "{result:?}"
    );
}
