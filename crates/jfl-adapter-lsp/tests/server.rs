//! The session in process (rust-analyzer's slow-tests pattern, on bytes: `memory`'s docs say why
//! not `Connection::memory()`): a client writes frames into the server's input pipe and reads
//! frames from its output pipe, with a probe handler whose pool jobs wait on the test, so every
//! interleaving below is forced rather than hoped for. No sleeps: the test and the jobs meet on
//! channels.

use std::io::Write;
use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use jfl_adapter_lsp::memory::{PipeReader, PipeWriter, pipe};
use jfl_adapter_lsp::transport::End;
use jfl_adapter_lsp::{Config, Dispatch, Ended, Handler, Job, Reply, serve};
use jfl_protocol::frame::{FrameDecoder, FrameError};
use jfl_protocol::rpc::ErrorCode;
use lsp_server::{Notification, Request, RequestId};
use serde_json::{Value, json};

#[cfg(test)]
mod support {
    use std::io::Read;

    use super::*;

    /// A handler whose behaviour each test drives by method name.
    ///
    /// - notification `inc`: adds one to the counter; any other notification is logged by name.
    /// - `log` (now): the notifications so far; `now` (now): the counter.
    /// - `get` (pool): reports its id on `started`, waits for the gate, answers its snapshot of
    ///   the counter.
    /// - `quick` (pool): answers its snapshot at once; `panic` (pool): panics.
    /// - `boom` (main loop) and notification `boom`: the handler itself panics; after notification
    ///   `break-snapshot`, taking a snapshot panics.
    pub(crate) struct Probe {
        pub(crate) log: Vec<String>,
        pub(crate) counter: u64,
        pub(crate) gate: Arc<Mutex<Receiver<()>>>,
        pub(crate) started: SyncSender<RequestId>,
        pub(crate) broken_snapshot: bool,
    }

    impl Handler for Probe {
        type Snapshot = u64;

        fn snapshot(&self) -> Arc<u64> {
            assert!(!self.broken_snapshot, "a snapshot that fails");
            Arc::new(self.counter)
        }

        fn request(&mut self, request: Request) -> Dispatch<u64> {
            match request.method.as_str() {
                "log" => Dispatch::Now(Reply::result(&self.log)),
                "now" => Dispatch::Now(Reply::result(&self.counter)),
                "get" => {
                    let (gate, started, id) =
                        (Arc::clone(&self.gate), self.started.clone(), request.id);
                    Dispatch::Pool(Job::new(move |snapshot: &u64, _| {
                        let _ = started.send(id);
                        let _ = gate.lock().unwrap().recv();
                        Reply::result(snapshot)
                    }))
                }
                "quick" => Dispatch::Pool(Job::new(|snapshot: &u64, _| Reply::result(snapshot))),
                "boom" => panic!("a handler that fails on a request"),
                "panic" => Dispatch::Pool(Job::new(|_: &u64, _| panic!("a job that fails"))),
                _ => Dispatch::MethodNotFound,
            }
        }

        fn notification(&mut self, notification: Notification) {
            if notification.method == "inc" {
                self.counter = self.counter.saturating_add(1);
            } else if notification.method == "boom" {
                panic!("a handler that fails on a notification");
            } else if notification.method == "break-snapshot" {
                self.broken_snapshot = true;
            } else {
                self.log.push(notification.method);
            }
        }
    }

    /// The client end of a session running on its own thread.
    pub(crate) struct Session {
        pub(crate) to_server: PipeWriter,
        pub(crate) from_server: PipeReader,
        decoder: FrameDecoder,
        pending: Vec<u8>,
        pub(crate) gate: SyncSender<()>,
        pub(crate) started: Receiver<RequestId>,
        pub(crate) server: JoinHandle<Ended>,
    }

    pub(crate) fn config(max_frame_len: usize) -> Config {
        Config {
            max_frame_len,
            workers: NonZeroUsize::new(2).unwrap(),
            queue: 4,
        }
    }

    impl Session {
        pub(crate) fn start(config: Config) -> Self {
            let (to_server, server_in) = pipe();
            let (server_out, from_server) = pipe();
            let (gate, gate_rx) = sync_channel(16);
            let (started_tx, started) = sync_channel(16);
            let probe = Probe {
                log: Vec::new(),
                counter: 0,
                gate: Arc::new(Mutex::new(gate_rx)),
                started: started_tx,
                broken_snapshot: false,
            };
            let server = thread::spawn(move || serve(server_in, server_out, probe, &config));
            Self {
                to_server,
                from_server,
                decoder: FrameDecoder::new(1 << 20),
                pending: Vec::new(),
                gate,
                started,
                server,
            }
        }

        pub(crate) fn request(&mut self, id: impl Into<RequestId>, method: &str) {
            let id = serde_json::to_value(id.into()).unwrap();
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method}));
        }

        pub(crate) fn notify(&mut self, method: &str, params: Value) {
            self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
        }

        /// One message, framed by the test (never `lsp_server::Message::write`, which the crate's
        /// clippy list denies).
        pub(crate) fn send(&mut self, message: &Value) {
            self.to_server.write_all(&frame(message)).unwrap();
        }

        pub(crate) fn cancel(&mut self, id: Value) {
            self.notify("$/cancelRequest", json!({ "id": id }));
        }

        pub(crate) fn raw(&mut self, bytes: &[u8]) {
            self.to_server.write_all(bytes).unwrap();
        }

        /// The server's next message; blocks until it comes.
        pub(crate) fn recv(&mut self) -> Value {
            loop {
                while !self.pending.is_empty() {
                    let progress = self.decoder.feed(&self.pending).unwrap();
                    self.pending.drain(..progress.consumed);
                    if let Some(body) = progress.frame {
                        return serde_json::from_slice(&body).unwrap();
                    }
                }
                let mut chunk = [0_u8; 4096];
                let n = self.from_server.read(&mut chunk).unwrap();
                assert!(n > 0, "the server's output ended");
                self.pending.extend_from_slice(&chunk[..n]);
            }
        }

        /// Ends the input and waits for the session to end; returns how, and what else it sent.
        pub(crate) fn finish(mut self) -> (Ended, Vec<Value>) {
            drop(self.to_server);
            let mut rest = Vec::new();
            self.from_server.read_to_end(&mut rest).unwrap();
            self.pending.extend(rest);
            let mut tail = Vec::new();
            while !self.pending.is_empty() {
                let progress = self.decoder.feed(&self.pending).unwrap();
                self.pending.drain(..progress.consumed);
                if let Some(body) = progress.frame {
                    tail.push(serde_json::from_slice(&body).unwrap());
                }
            }
            (self.server.join().unwrap(), tail)
        }
    }

    pub(crate) fn frame(message: &Value) -> Vec<u8> {
        let body = message.to_string();
        format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    pub(crate) fn error_code(message: &Value) -> Option<i32> {
        message["error"]["code"]
            .as_i64()
            .and_then(|c| i32::try_from(c).ok())
    }
}

use support::{Session, config, error_code, frame};

#[test]
fn should_apply_notifications_in_order_when_a_request_follows_them() {
    // given
    let mut session = Session::start(Config::default());
    for method in ["first", "second", "third"] {
        session.notify(method, json!({}));
    }

    // when
    session.request(1, "log");
    let answer = session.recv();

    // then
    assert_eq!(
        answer,
        json!({"jsonrpc": "2.0", "id": 1, "result": ["first", "second", "third"]})
    );
    let (ended, tail) = session.finish();
    assert_eq!(ended.input, End::Eof);
    assert!(tail.is_empty(), "{tail:?}");
}

#[test]
fn should_answer_from_the_snapshot_when_a_notification_follows_the_request() {
    // given
    let mut session = Session::start(config(1024));
    session.request("held".to_owned(), "get");
    assert_eq!(
        session.started.recv().unwrap(),
        RequestId::from("held".to_owned())
    );

    // when
    session.notify("inc", json!({}));
    session.request(2, "now");
    let now = session.recv();
    session.gate.send(()).unwrap();
    let held = session.recv();

    // then
    assert_eq!(now, json!({"jsonrpc": "2.0", "id": 2, "result": 1}));
    assert_eq!(held, json!({"jsonrpc": "2.0", "id": "held", "result": 0}));
    session.finish();
}

#[test]
fn should_answer_request_cancelled_when_the_request_is_cancelled_in_flight() {
    // given
    let mut session = Session::start(config(1024));
    session.request(5, "get");
    session.started.recv().unwrap();

    // when
    session.cancel(json!(5));
    session.request(6, "now");
    let barrier = session.recv();
    session.gate.send(()).unwrap();
    let cancelled = session.recv();

    // then
    assert_eq!(
        barrier["id"], 6,
        "the cancel is applied before the next message is read"
    );
    assert_eq!(cancelled["id"], 5);
    assert_eq!(
        error_code(&cancelled),
        Some(ErrorCode::RequestCancelled.code())
    );
    let (_, tail) = session.finish();
    assert!(tail.is_empty(), "answered once: {tail:?}");
}

#[test]
fn should_keep_the_result_when_the_cancel_comes_after_it() {
    // given
    let mut session = Session::start(config(1024));
    session.request("q".to_owned(), "quick");
    let result = session.recv();

    // when
    session.cancel(json!("q"));
    session.cancel(json!("never-sent"));
    session.request(7, "now");
    let next = session.recv();

    // then
    assert_eq!(result, json!({"jsonrpc": "2.0", "id": "q", "result": 0}));
    assert_eq!(next["id"], 7);
    let (_, tail) = session.finish();
    assert!(tail.is_empty(), "{tail:?}");
}

#[test]
fn should_refuse_a_second_request_when_its_id_is_already_in_flight() {
    // given
    let mut session = Session::start(config(1024));
    session.request(9, "get");
    session.started.recv().unwrap();

    // when
    session.request(9, "quick");
    let on_the_pool = session.recv();
    session.request(9, "now");
    let on_the_main_loop = session.recv();
    session.gate.send(()).unwrap();
    let first = session.recv();

    // then
    for duplicate in [&on_the_pool, &on_the_main_loop] {
        assert_eq!(
            duplicate["id"],
            Value::Null,
            "the pending request 9 is not settled by it"
        );
        assert_eq!(
            error_code(duplicate),
            Some(ErrorCode::InvalidRequest.code())
        );
    }
    assert_eq!(first, json!({"jsonrpc": "2.0", "id": 9, "result": 0}));
    session.finish();
}

#[test]
fn should_answer_internal_error_when_a_job_panics() {
    // given
    let mut session = Session::start(config(1024));

    // when
    session.request(11, "panic");
    let failed = session.recv();
    session.request(12, "quick");
    let after = session.recv();

    // then
    assert_eq!(failed["id"], 11);
    assert_eq!(error_code(&failed), Some(ErrorCode::InternalError.code()));
    assert_eq!(after, json!({"jsonrpc": "2.0", "id": 12, "result": 0}));
    session.finish();
}

#[test]
fn should_answer_method_not_found_when_a_method_is_unknown_or_a_notification_is_sent_as_a_request()
{
    // given
    let mut session = Session::start(config(1024));

    // when
    session.request("s".to_owned(), "textDocument/noSuchMethod");
    session.request(13, "$/cancelRequest");
    session.notify("textDocument/noSuchNotification", json!({}));
    let answers = [session.recv(), session.recv()];

    // then
    assert_eq!(answers[0]["id"], "s");
    assert_eq!(answers[1]["id"], 13);
    for answer in &answers {
        assert_eq!(error_code(answer), Some(ErrorCode::MethodNotFound.code()));
    }
    let (_, tail) = session.finish();
    assert!(
        tail.is_empty(),
        "a notification is never answered: {tail:?}"
    );
}

#[test]
fn should_carry_on_when_a_frame_is_over_the_limit() {
    // given
    let mut session = Session::start(config(64));
    let mut oversized = b"Content-Length: 65\r\n\r\n".to_vec();
    oversized.extend([b' '; 65]);

    // when
    session.raw(&oversized);
    session.request(14, "now");
    let refused = session.recv();
    let after = session.recv();

    // then
    assert_eq!(
        refused,
        json!({"jsonrpc": "2.0", "id": null, "error": {
            "code": -32600,
            "message": "a frame of 65 bytes is larger than the limit of 64 bytes",
            "data": {"declared": 65, "limit": 64}
        }})
    );
    assert_eq!(after, json!({"jsonrpc": "2.0", "id": 14, "result": 0}));
    session.finish();
}

#[test]
fn should_end_the_session_when_the_framing_is_lost() {
    // given
    let mut session = Session::start(config(1024));

    // when
    session.raw(b"Content-Length: 2\r\nX-Unknown: 1\r\n\r\n{}");
    let refused = session.recv();
    let (ended, tail) = session.finish();

    // then
    assert_eq!(refused["id"], Value::Null);
    assert_eq!(error_code(&refused), Some(ErrorCode::ParseError.code()));
    assert_eq!(ended.input, End::Framing(FrameError::UnknownHeader));
    assert!(tail.is_empty(), "{tail:?}");
}

#[test]
fn should_report_the_failed_write_when_the_client_stops_reading() {
    // given
    let mut session = Session::start(config(1024));
    session.request(15, "now");
    let first = session.recv();

    // when
    let Session {
        to_server,
        from_server,
        server,
        ..
    } = session;
    drop(from_server);
    let mut to_server = to_server;
    for id in 16..20 {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": "now"});
        to_server.write_all(&frame(&request)).unwrap();
    }
    drop(to_server);
    let ended = server.join().unwrap();

    // then
    assert_eq!(first["id"], 15);
    assert_eq!(ended.input, End::Eof);
    assert_eq!(ended.output, Some(std::io::ErrorKind::BrokenPipe));
}

#[test]
fn should_keep_serving_when_the_handler_panics_on_the_main_loop() {
    // given
    let mut session = Session::start(config(1024));

    // when
    session.notify("boom", json!({}));
    session.request(21, "boom");
    let failed_request = session.recv();
    session.notify("break-snapshot", json!({}));
    session.request(22, "quick");
    let failed_snapshot = session.recv();
    for _ in 0..100 {
        session.notify("boom", json!({}));
    }
    session.request(23, "now");
    let after = session.recv();
    let (ended, tail) = session.finish();

    // then
    assert_eq!(failed_request["id"], 21);
    assert_eq!(
        error_code(&failed_request),
        Some(ErrorCode::InternalError.code())
    );
    assert_eq!(failed_snapshot["id"], 22);
    assert_eq!(
        error_code(&failed_snapshot),
        Some(ErrorCode::InternalError.code())
    );
    assert_eq!(after, json!({"jsonrpc": "2.0", "id": 23, "result": 0}));
    assert_eq!(ended.input, End::Eof);
    assert!(tail.is_empty(), "{tail:?}");
}

#[test]
fn should_never_start_a_job_when_it_is_cancelled_in_the_queue() {
    // given
    let one_worker = Config {
        workers: NonZeroUsize::MIN,
        ..config(1024)
    };
    let mut session = Session::start(one_worker);
    session.request("a".to_owned(), "get");
    assert_eq!(
        session.started.recv().unwrap(),
        RequestId::from("a".to_owned())
    );
    session.request("b".to_owned(), "get");

    // when
    session.cancel(json!("b"));
    session.request(24, "now");
    let barrier = session.recv();
    session.gate.send(()).unwrap();
    session.gate.send(()).unwrap();
    let answers = [session.recv(), session.recv()];

    // then
    assert_eq!(barrier["id"], 24);
    assert_eq!(
        answers[0],
        json!({"jsonrpc": "2.0", "id": "a", "result": 0})
    );
    assert_eq!(answers[1]["id"], "b");
    assert_eq!(
        error_code(&answers[1]),
        Some(ErrorCode::RequestCancelled.code())
    );
    assert!(session.started.try_recv().is_err(), "b's job started");
    session.finish();
}
