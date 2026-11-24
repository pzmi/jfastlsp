//! `jfl lsp`'s session in process (component): the adapter's session over in-memory pipes, the
//! [`Session`] handler, and the link's loop over a stand-in daemon that each test drives through
//! channels, so the order of a cancellation and the daemon's answer is forced, never hoped for
//! (the transcripts of `fixtures/lsp/` cannot hold a request in flight: their input is written
//! whole before the server starts). No sleeps: the test, the link and the session meet on
//! channels.

#[cfg(test)]
mod given {
    use std::io::{Read, Write};
    use std::sync::atomic::AtomicU8;
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};

    use jfl_adapter_lsp::memory::{PipeReader, PipeWriter, pipe};
    use jfl_adapter_lsp::{Config, Ended, serve};
    use jfl_composition::daemon::lsp::{Daemon, Failure, LINK_QUEUE, Session, link};
    use jfl_protocol::frame::FrameDecoder;
    use jfl_protocol::messages::PositionParams;
    use jfl_protocol::{DocumentUri, WireEnvelope};
    use serde_json::{Value, json};

    pub(crate) const MAIN: &str = "file:///w/src/Main.java";

    /// What the stand-in daemon was told, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Heard {
        Open(String, i32, String),
        Change(String, i32, String),
        Close(String),
        Definition(u32, u32),
    }

    /// A daemon that reports each call on `heard` and answers each definition with what the test
    /// sends on `answers` (waiting for it).
    pub(crate) struct StandIn {
        heard: SyncSender<Heard>,
        answers: Arc<Mutex<Receiver<Result<WireEnvelope, Failure>>>>,
    }

    impl Daemon for StandIn {
        fn open(&mut self, uri: DocumentUri, version: i32, text: String) {
            let _ = self
                .heard
                .send(Heard::Open(uri.as_str().to_owned(), version, text));
        }

        fn change(&mut self, uri: DocumentUri, version: i32, text: String) {
            let _ = self
                .heard
                .send(Heard::Change(uri.as_str().to_owned(), version, text));
        }

        fn close(&mut self, uri: &DocumentUri) {
            let _ = self.heard.send(Heard::Close(uri.as_str().to_owned()));
        }

        fn definition(
            &mut self,
            params: PositionParams,
            _budget: std::time::Duration,
        ) -> Result<WireEnvelope, Failure> {
            let _ = self.heard.send(Heard::Definition(params.line, params.col));
            self.answers.lock().unwrap().recv().unwrap()
        }
    }

    /// The client's end of a session with a stand-in daemon.
    pub(crate) struct Client {
        to_server: PipeWriter,
        from_server: PipeReader,
        decoder: FrameDecoder,
        pending: Vec<u8>,
        /// Messages read while waiting for another id's answer.
        stash: Vec<Value>,
        pub(crate) heard: Receiver<Heard>,
        pub(crate) answers: SyncSender<Result<WireEnvelope, Failure>>,
        server: JoinHandle<Ended>,
    }

    impl Client {
        pub(crate) fn start() -> Self {
            let (to_server, server_in) = pipe();
            let (server_out, from_server) = pipe();
            let (heard_tx, heard) = sync_channel(64);
            let (answers, answers_rx) = sync_channel(64);
            let (items, queue) = sync_channel(LINK_QUEUE);
            thread::spawn(move || {
                let mut daemon = StandIn {
                    heard: heard_tx,
                    answers: Arc::new(Mutex::new(answers_rx)),
                };
                link(&queue, &mut daemon);
            });
            let session = Session::new(items, Arc::new(AtomicU8::new(0)), "0.0.0");
            let server =
                thread::spawn(move || serve(server_in, server_out, session, &Config::default()));
            let mut client = Self {
                to_server,
                from_server,
                decoder: FrameDecoder::new(1 << 20),
                pending: Vec::new(),
                stash: Vec::new(),
                heard,
                answers,
                server,
            };
            client.send(&json!({"jsonrpc": "2.0", "id": "init", "method": "initialize", "params": {"capabilities": {}}}));
            client.answer(&json!("init"));
            client
        }

        pub(crate) fn send(&mut self, message: &Value) {
            let body = message.to_string();
            let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
            self.to_server.write_all(frame.as_bytes()).unwrap();
        }

        pub(crate) fn definition(&mut self, id: u32, line: u32, character: u32) {
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/definition",
                "params": {"textDocument": {"uri": MAIN}, "position": {"line": line, "character": character}}}));
        }

        pub(crate) fn cancel(&mut self, id: u32) {
            self.send(
                &json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": id}}),
            );
        }

        pub(crate) fn notify(&mut self, method: &str, params: Value) {
            self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
        }

        /// Sends `shutdown` under `id` and waits for its answer: the session's main loop answers it
        /// in order, so every message sent before it has been applied once it comes. Answers to
        /// other requests may come before or after it (the pool answers in any order): they are
        /// kept for [`Client::answer`].
        pub(crate) fn barrier(&mut self, id: &str) {
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "shutdown"}));
            self.answer(&json!(id));
        }

        /// The answer to request `id`, whenever it came; others read meanwhile are kept.
        pub(crate) fn answer(&mut self, id: &Value) -> Value {
            if let Some(at) = self.stash.iter().position(|m| &m["id"] == id) {
                return self.stash.remove(at);
            }
            loop {
                let next = self.recv();
                if &next["id"] == id {
                    return next;
                }
                self.stash.push(next);
            }
        }

        /// Every message read but not taken by [`Client::answer`].
        pub(crate) fn unclaimed(&self) -> &[Value] {
            &self.stash
        }

        fn recv(&mut self) -> Value {
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

        pub(crate) fn finish(self) -> Ended {
            drop(self.to_server);
            drop(self.answers);
            self.server.join().unwrap()
        }
    }

    pub(crate) fn envelope() -> WireEnvelope {
        serde_json::from_value(json!({
            "schema_version": {"major": 1, "minor": 0},
            "position_encoding": "utf-16",
            "query": null,
            "view": {"worktree": "file:///w", "head": null, "generation": 1},
            "target": {"symbol": "demo.Greeter#wave()", "status": "exact"},
            "exact": [{"uri": "file:///w/src/Greeter.java", "range": [3, 16, 3, 20]}],
            "possible": [],
            "complete": true,
            "incomplete_because": [],
            "freshness": {"synced_at": null, "pending_events": 0}
        }))
        .unwrap()
    }
}

use given::{Client, Heard, MAIN, envelope};
use jfl_composition::daemon::lsp::Failure;
use serde_json::json;

#[test]
fn should_answer_request_cancelled_when_the_cancel_comes_before_the_daemon_answers() {
    // given: the daemon has the request and has not answered
    let mut client = Client::start();
    client.definition(7, 1, 2);
    assert_eq!(client.heard.recv().unwrap(), Heard::Definition(1, 2));

    // when: the cancel is applied (the barrier proves it) before the daemon answers
    client.cancel(7);
    client.barrier("after-cancel");
    client.answers.send(Ok(envelope())).unwrap();
    let answer = client.answer(&json!(7));

    // then: cancelled, whenever it was answered relative to the barrier, and only once
    assert_eq!(answer["error"]["code"], -32800, "{answer}");
    client.barrier("after-answer");
    assert!(client.unclaimed().is_empty(), "{:?}", client.unclaimed());
    client.finish();
}

#[test]
fn should_keep_the_result_when_the_cancel_comes_after_the_answer() {
    // given
    let mut client = Client::start();
    client.definition(8, 1, 2);
    assert_eq!(client.heard.recv().unwrap(), Heard::Definition(1, 2));
    client.answers.send(Ok(envelope())).unwrap();
    let answer = client.answer(&json!(8));

    // when
    client.cancel(8);
    client.barrier("after-cancel");

    // then: the result stands, and the cancel brought no second answer
    assert_eq!(
        answer["result"],
        json!([{"uri": "file:///w/src/Greeter.java", "range": {"start": {"line": 3, "character": 16}, "end": {"line": 3, "character": 20}}}])
    );
    assert!(client.unclaimed().is_empty(), "{:?}", client.unclaimed());
    client.finish();
}

#[test]
fn should_answer_at_once_and_never_ask_the_daemon_when_a_queued_request_is_cancelled() {
    // given: the link is busy with request 1, request 2 waits behind it
    let mut client = Client::start();
    client.definition(1, 0, 0);
    assert_eq!(client.heard.recv().unwrap(), Heard::Definition(0, 0));
    client.definition(2, 5, 5);

    // when: request 2 is cancelled; request 1 is still unanswered
    client.cancel(2);
    let cancelled = client.answer(&json!(2));
    client.answers.send(Ok(envelope())).unwrap();
    let first = client.answer(&json!(1));

    // then: 2 is answered before the daemon frees the link, and the daemon never hears of it
    assert_eq!(cancelled["error"]["code"], -32800, "{cancelled}");
    assert!(first["result"].is_array(), "{first}");
    client.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": MAIN}}),
    );
    client.barrier("after-close");
    assert_eq!(client.heard.recv().unwrap(), Heard::Close(MAIN.to_owned()));
    client.finish();
}

#[test]
fn should_send_the_whole_text_with_its_version_when_the_client_changes_a_document_by_ranges() {
    // given
    let mut client = Client::start();
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": MAIN, "languageId": "java", "version": 1, "text": "class 😀A {}\n"}}),
    );

    // when: ranges in UTF-16 units, applied in order
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": MAIN, "version": 2}, "contentChanges": [
            {"range": {"start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 9}}, "text": "B"},
            {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "text": "// end\n"}
        ]}),
    );
    client.definition(3, 0, 99);
    let asked = [
        client.heard.recv().unwrap(),
        client.heard.recv().unwrap(),
        client.heard.recv().unwrap(),
    ];
    client
        .answers
        .send(Err(Failure {
            code: jfl_protocol::rpc::ErrorCode::NoSymbol,
            message: "none".to_owned(),
        }))
        .unwrap();
    let answer = client.answer(&json!(3));

    // then: the daemon got the whole texts, the column was clamped to the line's end (12 units),
    // and no symbol is LSP's null
    assert_eq!(
        asked,
        [
            Heard::Open(MAIN.to_owned(), 1, "class 😀A {}\n".to_owned()),
            Heard::Change(MAIN.to_owned(), 2, "class 😀B {}\n// end\n".to_owned()),
            Heard::Definition(0, 12),
        ]
    );
    assert_eq!(answer, json!({"jsonrpc": "2.0", "id": 3, "result": null}));
    client.finish();
}

#[test]
fn should_answer_content_modified_without_asking_when_a_change_does_not_fit_or_goes_back() {
    for (version, range) in [
        // a range past the text
        (
            2,
            json!({"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 0}}),
        ),
        // a version that does not increase
        (
            1,
            json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}),
        ),
    ] {
        // given
        let mut client = Client::start();
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": MAIN, "languageId": "java", "version": 1, "text": "class A {}\n"}}),
        );
        assert!(matches!(client.heard.recv().unwrap(), Heard::Open(..)));

        // when
        client.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": MAIN, "version": version},
                "contentChanges": [{"range": range, "text": "x"}]}),
        );
        client.definition(4, 0, 6);
        let answer = client.answer(&json!(4));

        // then: the daemon's overlay is dropped and the request never reaches it
        assert_eq!(client.heard.recv().unwrap(), Heard::Close(MAIN.to_owned()));
        assert_eq!(answer["error"]["code"], -32801, "{answer}");
        client.barrier("after");
        assert!(client.heard.try_recv().is_err());
        client.finish();
    }
}
