//! The `endpoint` contract suite (`jfl_testkit::contracts::endpoint`, ver §5) against the real
//! carrier: the daemon's [`Listener`] and its connections on a current-thread Tokio runtime, and
//! the proxy's [`ClientConn`], over a Unix socket in a scratch state directory.
//!
//! The server runs on a thread of its own, as the daemon does: its reader reads ahead into a
//! channel and its writer takes the frames the suite hands it, so a frame larger than the socket's
//! buffers goes through while the other side is busy, in either direction.

use std::os::fd::{AsFd, BorrowedFd};
use std::thread::JoinHandle;
use std::time::Duration;

use jfl_adapter_ipc::message::End;
use jfl_adapter_ipc::{
    Accepted, ClientConn, Incoming, Listener, Received, RecvError, SocketPath, StateDir,
    current_uid,
};
use jfl_testkit::contracts::endpoint::{Connection, EndpointUnderTest, Got, Read, Refused, check};

#[cfg(test)]
mod support;
use support::{Scratch, plenty, runtime};

#[cfg(test)]
mod real {
    use super::*;

    /// A wait at least this long is one a frame already sent, or the end of a closed connection,
    /// must finish within: the client waits as long as it takes. A shorter one is a single wait.
    const GENEROUS: Duration = Duration::from_secs(20);

    /// Frames read or to write that may wait in a channel: far more than any case leaves unread.
    const QUEUE: usize = 64;

    enum Command {
        Send(Vec<u8>),
        Close,
    }

    /// The real carrier: one scratch state directory and listener per connection.
    pub(super) struct Real;

    impl EndpointUnderTest for Real {
        fn open(&mut self) -> Result<Box<dyn Connection>, String> {
            RealConn::open().map(|conn| Box::new(conn) as Box<dyn Connection>)
        }
    }

    struct RealConn {
        client: Option<ClientConn>,
        reads: tokio::sync::mpsc::Receiver<Incoming>,
        commands: Option<tokio::sync::mpsc::Sender<Command>>,
        server: Option<JoinHandle<()>>,
        _scratch: Scratch,
    }

    impl RealConn {
        fn open() -> Result<Self, String> {
            let scratch = Scratch::new();
            let at = |e: &dyn std::fmt::Display| format!("{}: {e}", scratch.path().display());
            let state = StateDir::open(&scratch.state()).map_err(|e| at(&e))?;
            let path = SocketPath::of_state(&state).map_err(|e| at(&e))?;
            let (bound, is_bound) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
            let (read, reads) = tokio::sync::mpsc::channel(QUEUE);
            let (commands, mut command) = tokio::sync::mpsc::channel(QUEUE);
            let socket = path.clone();
            let server = std::thread::spawn(move || {
                runtime().block_on(async move {
                    let listener = match Listener::bind(&socket, current_uid()) {
                        Ok(listener) => listener,
                        Err(e) => {
                            let _sent = bound.send(Err(e.to_string()));
                            return;
                        }
                    };
                    let _sent = bound.send(Ok(()));
                    let Ok(Accepted::Peer(mut reader, mut writer)) = listener.accept().await else {
                        return;
                    };
                    let reading = tokio::spawn(async move {
                        loop {
                            let incoming = reader.next().await;
                            let ended = matches!(incoming, Incoming::End(_));
                            if read.send(incoming).await.is_err() {
                                return;
                            }
                            if ended {
                                // The read after an end, which must be the same end.
                                let _sent = read.send(reader.next().await).await;
                                return;
                            }
                        }
                    });
                    while let Some(Command::Send(body)) = command.recv().await {
                        if writer.send(&body).await.is_err() {
                            break;
                        }
                    }
                    drop(writer);
                    reading.abort();
                });
            });
            is_bound
                .recv()
                .map_err(|_| "the server thread ended before its bind".to_owned())??;
            let client = ClientConn::connect(path.path()).map_err(|e| at(&e))?;
            Ok(Self {
                client: Some(client),
                reads,
                commands: Some(commands),
                server: Some(server),
                _scratch: scratch,
            })
        }

        fn command(&self, command: Command) {
            if let Some(commands) = &self.commands {
                let _sent = commands.blocking_send(command);
            }
        }
    }

    impl Drop for RealConn {
        fn drop(&mut self) {
            self.client = None;
            self.commands = None;
            if let Some(server) = self.server.take() {
                let _joined = server.join();
            }
        }
    }

    /// Writes `bytes` whole to the non-blocking socket `fd`, waiting for room with `poll(2)`.
    fn write_raw(fd: BorrowedFd<'_>, mut bytes: &[u8]) {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use rustix::io::Errno;
        while !bytes.is_empty() {
            match rustix::io::write(fd, bytes) {
                Ok(n) => bytes = bytes.get(n..).unwrap_or_default(),
                Err(Errno::AGAIN | Errno::INTR) => {
                    let mut fds = [PollFd::new(&fd, PollFlags::OUT)];
                    let wait = Timespec {
                        tv_sec: 20,
                        tv_nsec: 0,
                    };
                    poll(&mut fds, Some(&wait)).unwrap();
                }
                Err(e) => panic!("raw write: {e}"),
            }
        }
    }

    impl Connection for RealConn {
        fn client_send(&mut self, body: &[u8]) -> Result<(), Refused> {
            let client = self
                .client
                .as_mut()
                .ok_or(Refused::Failed("closed".into()))?;
            match client.send(body, &mut plenty()) {
                Ok(()) => Ok(()),
                Err(RecvError::Framing(jfl_protocol::FrameError::TooLarge { .. })) => {
                    Err(Refused::TooLarge)
                }
                Err(e) => Err(Refused::Failed(e.to_string())),
            }
        }

        fn client_write_raw(&mut self, bytes: &[u8]) {
            if let Some(client) = &self.client {
                write_raw(client.as_fd(), bytes);
            }
        }

        fn client_recv(&mut self, wait: Duration) -> Got {
            let Some(client) = self.client.as_mut() else {
                return Got::Failed("closed".to_owned());
            };
            // No clock here: a generous wait is renewed for as long as reads go on; a short one is
            // one poll, after which nothing is left.
            let mut once = Some(wait);
            let mut remaining = move || {
                if wait >= GENEROUS {
                    Some(wait)
                } else {
                    once.take()
                }
            };
            match client.recv(&mut remaining) {
                Ok(Received::Frame(body)) => Got::Frame(body),
                Ok(Received::Eof) => Got::Eof,
                Err(RecvError::TimedOut) => Got::TimedOut,
                Err(e) => Got::Failed(e.to_string()),
            }
        }

        fn client_close(&mut self) {
            self.client = None;
        }

        fn server_next(&mut self) -> Read {
            match self.reads.blocking_recv() {
                Some(Incoming::Call(call)) => Read::Call {
                    id: call.id,
                    method: call.method,
                    params: call.params,
                },
                Some(Incoming::Note(note)) => Read::Note {
                    method: note.method,
                    params: note.params,
                },
                Some(Incoming::Malformed(malformed)) => Read::Malformed {
                    id: malformed.id,
                    code: malformed.code,
                },
                Some(Incoming::Oversized(oversized)) => Read::Oversized {
                    declared: oversized.declared,
                },
                Some(Incoming::End(end)) => Read::End {
                    clean: end == End::Eof,
                },
                None => Read::Stalled,
            }
        }

        fn server_send(&mut self, body: &[u8]) {
            self.command(Command::Send(body.to_vec()));
        }

        fn server_close(&mut self) {
            self.command(Command::Close);
        }
    }
}
use real::Real;

#[test]
fn should_pass_every_case_of_the_endpoint_contract_when_the_carrier_is_the_unix_socket() {
    // given
    let mut endpoint = Real;

    // when
    let violations = check(&mut endpoint);

    // then
    assert_eq!(violations, []);
}
