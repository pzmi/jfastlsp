//! The long-lived mode against real children: length-prefixed frames, the frame limit, the
//! deadline, the end of stream and the kill on drop.

use std::io::{BufRead, BufReader};
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use jfl_process::{
    Command, Ending, Exit, FramedError, FramedOptions, RunError, Runner, StderrSink,
};
use jfl_testkit::poll::wait_until;

#[cfg(test)]
mod support;
use support::{CHILD, GENEROUS, is_gone, run};

#[cfg(test)]
mod helpers {
    use super::*;

    /// Starts the child that leaves its group, and returns once it says it has.
    pub(crate) fn start_outside_its_group() -> jfl_process::FramedChild {
        let (reader, writer) = std::io::pipe().unwrap();
        let options = FramedOptions {
            stderr: StderrSink::Fd(OwnedFd::from(writer)),
            ..FramedOptions::default()
        };
        let child = start("join-parent-group", options).unwrap();
        let mut line = String::new();
        BufReader::new(reader).read_line(&mut line).unwrap();
        assert_eq!(line, "moved\n");
        child
    }
}

fn start(mode: &str, options: FramedOptions) -> Result<jfl_process::FramedChild, RunError> {
    Runner::new().start_framed(&Command::new(CHILD).arg(mode), options)
}

#[test]
fn should_round_trip_frames_when_the_child_echoes_them() {
    // given
    let mut child = start("echo-frames", FramedOptions::default()).unwrap();
    let big = vec![7_u8; 300_000];

    // when
    let first = child.request(b"hello", GENEROUS).unwrap();
    let empty = child.request(b"", GENEROUS).unwrap();
    let large = child.request(&big, GENEROUS).unwrap();

    // then
    assert_eq!(first, b"hello");
    assert_eq!(empty, b"");
    assert_eq!(large, big);
    assert_eq!(child.close(GENEROUS).unwrap(), Exit::Code(0));
}

#[test]
fn should_refuse_before_allocating_when_the_child_declares_an_oversized_frame() {
    // given: the child declares u32::MAX bytes
    let mut child = start("oversized-frame", FramedOptions::default()).unwrap();

    // when
    let received = child.recv(GENEROUS);

    // then
    assert!(
        matches!(
            received,
            Err(FramedError::FrameTooLarge {
                declared: 4_294_967_295,
                ..
            })
        ),
        "{received:?}"
    );
    assert!(matches!(child.recv(GENEROUS), Err(FramedError::Dead)));
}

#[test]
fn should_refuse_a_payload_over_the_limit_when_sending_and_keep_the_child() {
    // given
    let options = FramedOptions {
        max_frame: 8,
        ..FramedOptions::default()
    };
    let mut child = start("echo-frames", options).unwrap();

    // when
    let refused = child.send(b"123456789", GENEROUS);

    // then
    assert!(
        matches!(
            refused,
            Err(FramedError::PayloadTooLarge { len: 9, max: 8 })
        ),
        "{refused:?}"
    );
    assert_eq!(child.request(b"12345678", GENEROUS).unwrap(), b"12345678");
}

#[test]
fn should_kill_the_child_when_a_reply_misses_its_deadline() {
    // given: the child never reads or answers
    let mut child = start("hang", FramedOptions::default()).unwrap();
    let pid = child.id().to_string();

    // when
    let reply = child.request(b"ping", Duration::from_millis(200));

    // then
    assert!(matches!(reply, Err(FramedError::Deadline)), "{reply:?}");
    assert!(is_gone(&pid));
}

#[test]
fn should_kill_the_child_when_a_send_misses_its_deadline() {
    // given: the child never reads, so a frame larger than the pipe's buffer cannot be written
    let mut child = start("hang", FramedOptions::default()).unwrap();
    let pid = child.id().to_string();
    let payload = vec![0_u8; 4 << 20];

    // when
    let sent = child.send(&payload, Duration::from_millis(200));

    // then
    assert!(matches!(sent, Err(FramedError::Deadline)), "{sent:?}");
    assert!(is_gone(&pid));
}

#[test]
fn should_report_the_exit_when_the_child_ends_the_stream() {
    // given
    let mut child = start("exit", FramedOptions::default()).unwrap();

    // when
    let reply = child.recv(GENEROUS);

    // then: `exit` without a code exits 2
    assert!(
        matches!(reply, Err(FramedError::Exited(Exit::Code(2)))),
        "{reply:?}"
    );
}

#[test]
fn should_kill_the_group_when_the_handle_is_dropped() {
    // given
    let child = start("hang", FramedOptions::default()).unwrap();
    let pid = child.id().to_string();

    // when
    drop(child);

    // then
    wait_until(GENEROUS, "the dropped child to be gone", || is_gone(&pid)).unwrap();
    let after = run(&Command::new(CHILD).arg("exit").arg("0"), GENEROUS);
    assert_eq!(after.ending, Ending::Exited(Exit::Code(0)));
}

#[test]
fn should_kill_the_leader_when_it_left_its_group_and_misses_a_deadline() {
    // given
    let mut child = helpers::start_outside_its_group();
    let pid = child.id().to_string();
    let started = Instant::now();

    // when
    let reply = child.recv(Duration::from_millis(300));

    // then
    assert!(matches!(reply, Err(FramedError::Deadline)), "{reply:?}");
    assert!(started.elapsed() < GENEROUS / 4, "{:?}", started.elapsed());
    assert!(is_gone(&pid));
}

#[test]
fn should_kill_the_leader_when_it_left_its_group_and_the_handle_is_dropped() {
    // given
    let child = helpers::start_outside_its_group();
    let pid = child.id().to_string();
    let started = Instant::now();

    // when
    drop(child);

    // then
    assert!(started.elapsed() < GENEROUS / 4, "{:?}", started.elapsed());
    assert!(is_gone(&pid));
}
