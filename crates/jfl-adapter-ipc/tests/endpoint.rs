//! The IPC adapter against the real OS (ver §2, integration): the state directory's checks and
//! modes, the locks, the listener's mode and peer check, framing both ways, the notify handshake
//! and close-on-exec, in a scratch directory. The mode tests hold under a non-root uid too (run
//! under `setpriv --reuid --regid --clear-groups`, since the sandbox runs as root and CI does not).

#[cfg(test)]
mod support;

use std::fs::{self, Permissions};
use std::os::fd::AsFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::time::Duration;

use jfl_adapter_ipc::lock::FileId;
use jfl_adapter_ipc::notify::{self, Notice, NotifyListener, Waited};
use jfl_adapter_ipc::{
    Accepted, ClientConn, ConnectError, Flock, Incoming, Listener, LockError, Received, RecvError,
    SocketPath, StateDir, StateDirError, current_uid,
};
use jfl_protocol::rpc::RequestId;
use rustix::io::FdFlags;
use support::{Scratch, runtime};

/// Ten seconds left, always: the tests' deadline for a wait that must end at once.
fn plenty() -> impl FnMut() -> Option<std::time::Duration> {
    || Some(std::time::Duration::from_secs(10))
}

#[cfg(test)]
mod given {
    use super::*;

    /// The permission bits of `path` (not following a symlink).
    pub(crate) fn mode(path: &std::path::Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    /// Whether `fd` is close-on-exec.
    pub(crate) fn cloexec(fd: impl AsFd) -> bool {
        rustix::io::fcntl_getfd(fd)
            .unwrap()
            .contains(FdFlags::CLOEXEC)
    }
}

use given::{cloexec, mode};

#[test]
fn should_create_the_state_directory_0700_when_it_is_absent() {
    // given
    let scratch = Scratch::new();
    let path = scratch.path().join("parent").join("s");

    // when
    let state = StateDir::open(&path).unwrap();

    // then
    assert_eq!(state.path(), path);
    assert_eq!(mode(&path), 0o700);
    assert_eq!(fs::metadata(&path).unwrap().uid(), current_uid());
}

#[test]
fn should_refuse_an_existing_state_directory_naming_it_when_its_mode_is_0755() {
    // given
    let scratch = Scratch::new();
    let path = scratch.state();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();

    // when
    let refused = StateDir::open(&path);

    // then
    let error = refused.unwrap_err();
    assert!(
        matches!(error, StateDirError::Mode { mode: 0o755, .. }),
        "{error}"
    );
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "{error}"
    );
    assert!(error.to_string().contains("0755, not 0700"), "{error}");
}

#[test]
fn should_refuse_an_existing_state_directory_when_another_uid_owns_it() {
    // given: the directory is this uid's, and the daemon is told it runs as another
    let scratch = Scratch::new();
    let path = scratch.state();
    StateDir::open(&path).unwrap();
    let other = current_uid().wrapping_add(1);

    // when
    let refused = StateDir::open_for(&path, other);

    // then
    let error = refused.unwrap_err();
    assert!(
        matches!(error, StateDirError::Owner { owner, uid, .. } if owner == current_uid() && uid == other),
        "{error}"
    );
    assert!(
        error.to_string().contains(&path.display().to_string()),
        "{error}"
    );
}

#[test]
fn should_refuse_a_symlink_when_it_stands_for_the_state_directory() {
    // given
    let scratch = Scratch::new();
    let real = scratch.path().join("real");
    StateDir::open(&real).unwrap();
    let link = scratch.state();
    symlink(&real, &link).unwrap();

    // when
    let refused = StateDir::open(&link);

    // then
    assert!(
        matches!(refused, Err(StateDirError::NotADirectory { .. })),
        "{refused:?}"
    );
}

#[test]
fn should_refuse_a_second_holder_when_the_lock_is_held_and_admit_one_after_release() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let first = Flock::try_acquire(&state.lock_path()).unwrap();

    // when
    let second = Flock::try_acquire(&state.lock_path());
    drop(first);
    let third = Flock::try_acquire(&state.lock_path());

    // then
    assert!(matches!(second, Err(LockError::Held { .. })), "{second:?}");
    let third = third.unwrap();
    assert!(state.lock_path().exists(), "the lock file is never removed");
    assert_eq!(mode(&state.lock_path()), 0o600);
    assert!(third.still_named().unwrap());
    assert!(cloexec(&third));
}

#[test]
fn should_reset_the_lock_file_to_0600_when_it_was_left_wider() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    fs::write(state.lock_path(), b"").unwrap();
    fs::set_permissions(state.lock_path(), Permissions::from_mode(0o644)).unwrap();

    // when
    let lock = Flock::try_acquire(&state.lock_path()).unwrap();

    // then
    assert_eq!(mode(&state.lock_path()), 0o600);
    drop(lock);
}

#[test]
fn should_see_the_lock_path_no_longer_name_the_lock_when_the_file_is_replaced() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let lock = Flock::try_acquire(&state.lock_path()).unwrap();
    let held = lock.id();

    // when
    fs::remove_file(state.lock_path()).unwrap();
    let gone = lock.still_named().unwrap();
    let second = Flock::try_acquire(&state.lock_path()).unwrap();
    let replaced = lock.still_named().unwrap();

    // then: a second process could lock the new file: the first must notice and stop
    assert!(!gone);
    assert!(!replaced);
    assert_ne!(second.id(), held);
}

#[test]
fn should_bind_the_socket_0600_when_the_daemon_listens() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();

    // when
    let listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();

    // then
    assert_eq!(mode(path.path()), 0o600);
    assert_eq!(FileId::at(path.path()).unwrap(), Some(listener.id()));
    assert!(cloexec(&listener));
}

#[test]
fn should_refuse_the_peer_when_its_uid_is_not_the_daemons() {
    // given: a daemon told its uid is another, so this process is the stranger
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();
    let other = current_uid().wrapping_add(1);
    let listener = rt.block_on(async { Listener::bind(&path, other) }).unwrap();
    let mut client = ClientConn::connect(path.path()).unwrap();

    // when
    let accepted = rt.block_on(listener.accept()).unwrap();
    let read = client.recv(&mut plenty());

    // then
    assert!(matches!(accepted, Accepted::Refused { uid } if uid == current_uid()));
    assert!(matches!(read, Ok(Received::Eof)), "{read:?}");
}

#[test]
fn should_serve_the_next_peer_when_one_closed_before_its_accept() {
    // given: a peer that connected and closed (a proxy's second check), then one that sent its
    // handshake, both waiting in the backlog. On macOS the first one's credentials can no
    // longer be read (`ENOTCONN`); on Linux it is served and reads its EOF
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();
    let listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();
    assert!(jfl_adapter_ipc::client::listening(path.path()));
    let mut client = ClientConn::connect(path.path()).unwrap();
    client
        .send(
            br#"{"jsonrpc":"2.0","id":"hs","method":"jfl/status","params":{"worktree":null}}"#,
            &mut plenty(),
        )
        .unwrap();

    // when: accepting until a peer's first read is a frame, as the daemon's host does; the
    // closed peer, if it is served at all, ends at once
    let mut firsts = Vec::new();
    for _ in 0..2 {
        let first = match rt.block_on(listener.accept()) {
            Ok(Accepted::Peer(mut reader, _writer)) => rt.block_on(reader.next()),
            other => panic!("the accept failed after a peer closed: {other:?}"),
        };
        let served = matches!(first, Incoming::Call(_));
        firsts.push(first);
        if served {
            break;
        }
    }

    // then
    assert!(
        matches!(firsts.last(), Some(Incoming::Call(call)) if call.id == RequestId::String("hs".into())),
        "{firsts:?}"
    );
    assert!(
        firsts
            .iter()
            .rev()
            .skip(1)
            .all(|first| matches!(first, Incoming::End(_))),
        "{firsts:?}"
    );
}

#[test]
fn should_carry_frames_both_ways_when_a_peer_of_the_daemons_uid_connects() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();
    let listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();
    let mut client = ClientConn::connect(path.path()).unwrap();
    client
        .send(
            br#"{"jsonrpc":"2.0","id":"hs","method":"jfl/status","params":{"worktree":null}}"#,
            &mut plenty(),
        )
        .unwrap();

    // when
    let (mut reader, mut writer) = match rt.block_on(listener.accept()).unwrap() {
        Accepted::Peer(reader, writer) => (reader, writer),
        Accepted::Refused { uid } => panic!("refused uid {uid}"),
    };
    let incoming = rt.block_on(reader.next());
    rt.block_on(writer.send(br#"{"jsonrpc":"2.0","id":"hs","result":{}}"#))
        .unwrap();
    let answer = client.recv(&mut plenty()).unwrap();
    drop(client);
    let end = rt.block_on(reader.next());

    // then
    assert!(
        matches!(&incoming, Incoming::Call(call) if call.id == RequestId::String("hs".into())),
        "{incoming:?}"
    );
    assert_eq!(
        answer,
        Received::Frame(br#"{"jsonrpc":"2.0","id":"hs","result":{}}"#.to_vec())
    );
    assert!(matches!(end, Incoming::End(_)), "{end:?}");
    assert!(cloexec(&*reader));
}

#[test]
fn should_skip_an_oversized_frame_without_buffering_it_when_valid_frames_surround_it() {
    // given: a frame declaring 1 GiB, cut short by the next one's bytes never arriving
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();
    let listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();
    let mut client = std::os::unix::net::UnixStream::connect(path.path()).unwrap();
    use std::io::Write;
    client
        .write_all(b"Content-Length: 1073741824\r\n\r\n")
        .unwrap();

    // when
    let (mut reader, _writer) = match rt.block_on(listener.accept()).unwrap() {
        Accepted::Peer(reader, writer) => (reader, writer),
        Accepted::Refused { uid } => panic!("refused uid {uid}"),
    };
    let first = rt.block_on(reader.next());
    drop(client);
    let end = rt.block_on(reader.next());

    // then
    assert!(
        matches!(first, Incoming::Oversized(o) if o.declared == 1 << 30),
        "{first:?}"
    );
    assert!(matches!(end, Incoming::End(_)), "{end:?}");
}

#[test]
fn should_tell_an_absent_socket_from_a_stale_one_when_connecting() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();

    // when
    let absent = ClientConn::connect(path.path());
    drop(std::os::unix::net::UnixListener::bind(path.path()).unwrap());
    let stale = ClientConn::connect(path.path());

    // then
    assert!(matches!(absent, Err(ConnectError::Absent)), "{absent:?}");
    assert!(matches!(stale, Err(ConnectError::Refused)), "{stale:?}");
    assert!(!jfl_adapter_ipc::client::listening(path.path()));
}

#[test]
fn should_time_out_a_read_when_the_daemon_never_answers() {
    // given: a listener that never accepts
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();
    let _listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();
    let mut client = ClientConn::connect(path.path()).unwrap();
    let mut budget = [Some(Duration::from_millis(20)), None].into_iter();

    // when
    let read = client.recv(&mut || budget.next().flatten());

    // then
    assert!(matches!(read, Err(RecvError::TimedOut)), "{read:?}");
}

#[test]
fn should_deliver_the_notice_when_the_daemon_reports_ready() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let listener = NotifyListener::bind(&state.notify_path(1)).unwrap();
    let nothing = listener.recv(Duration::ZERO, None).unwrap();

    // when
    notify::send(&state.notify_path(1), &Notice::Ready { pid: 7 }).unwrap();
    let notice = listener.recv(Duration::from_secs(10), None).unwrap();
    let mode_while_bound = mode(&state.notify_path(1));
    drop(listener);

    // then
    assert_eq!(nothing, Waited::Nothing);
    assert_eq!(notice, Waited::Notice(Notice::Ready { pid: 7 }));
    assert_eq!(mode_while_bound, 0o600);
    assert!(
        !state.notify_path(1).exists(),
        "the proxy removes its notify socket"
    );
}

#[test]
fn should_open_every_descriptor_close_on_exec_when_the_daemon_side_is_up() {
    // given
    let scratch = Scratch::new();
    let state = StateDir::open(&scratch.state()).unwrap();
    let path = SocketPath::of_state(&state).unwrap();
    let rt = runtime();

    // when
    let lock = Flock::try_acquire(&state.lock_path()).unwrap();
    let spawn = Flock::try_acquire(&state.spawn_lock_path()).unwrap();
    let listener = rt
        .block_on(async { Listener::bind(&path, current_uid()) })
        .unwrap();
    let client = ClientConn::connect(path.path()).unwrap();
    let accepted = rt.block_on(listener.accept()).unwrap();

    // then
    assert!(cloexec(&lock));
    assert!(cloexec(&spawn));
    assert!(cloexec(&listener));
    assert!(cloexec(&client));
    match accepted {
        Accepted::Peer(reader, _) => assert!(cloexec(&*reader)),
        Accepted::Refused { uid } => panic!("refused uid {uid}"),
    }
}
