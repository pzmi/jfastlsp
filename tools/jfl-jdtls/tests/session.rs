//! Component tests: a session (the real mux and its threads) over the scripted fake backend,
//! with clients on in-memory Unix socket pairs.

use std::error::Error;
use std::io::{BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use jfl_jdtls::fake::{FakeInfo, serve};
use jfl_jdtls::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
use jfl_jdtls::json::Value;
use jfl_jdtls::mux::{ClientId, MuxConfig};
use jfl_jdtls::session::{self, Backend, Event, Owner, SessionConfig, SessionHandle};

const WAIT: Duration = Duration::from_secs(20);

type Res<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Default)]
struct TestOwner {
    evictable: bool,
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Owner for TestOwner {
    fn evict(&self, still_idle: &mut dyn FnMut() -> bool) -> bool {
        let evicted = self.evictable && still_idle();
        if evicted && let Ok(mut log) = self.log.lock() {
            log.push("evicted");
        }
        evicted
    }

    fn ended(&self) {
        if let Ok(mut log) = self.log.lock() {
            log.push("ended");
        }
    }
}

fn start(owner: TestOwner, idle: Duration) -> Res<SessionHandle> {
    let (ours, theirs) = UnixStream::pair()?;
    let fake_reader = theirs.try_clone()?;
    thread::spawn(move || {
        let mut writer = theirs;
        let _ = serve(
            &mut BufReader::new(fake_reader),
            &mut writer,
            &FakeInfo::default(),
        );
        let _ = writer.shutdown(Shutdown::Both);
    });
    let backend = Backend {
        reader: Box::new(ours.try_clone()?),
        writer: Box::new(ours),
        child: None,
    };
    Ok(session::start(config(idle), backend, owner)?)
}

fn config(idle: Duration) -> SessionConfig {
    SessionConfig {
        mux: MuxConfig {
            root_uri: "file:///w/repo".into(),
            root_path: "/w/repo".into(),
            root_name: "repo".into(),
            process_id: None,
            settings: None,
        },
        idle_timeout: idle,
        stop_timeout: Duration::from_secs(10),
        tick: Duration::from_millis(10),
    }
}

/// A client: its socket, and the messages it received, in order.
struct Client {
    stream: UnixStream,
    inbox: Receiver<Value>,
}

impl Client {
    fn connect(session: &SessionHandle, id: ClientId) -> Res<Self> {
        let (ours, theirs) = UnixStream::pair()?;
        let sink = session::spawn_writer(theirs.try_clone()?, 64, |s: UnixStream| {
            let _ = s.shutdown(Shutdown::Both);
        })?;
        session.events.send(Event::Attach { client: id, sink })?;
        let events = session.events.clone();
        thread::spawn(move || session::pump_client(&events, id, theirs));
        let (tx, inbox) = sync_channel(256);
        let mut reader = BufReader::new(ours.try_clone()?);
        thread::spawn(move || {
            while let Ok(Some(body)) = read_frame(&mut reader, MAX_FRAME_BYTES) {
                let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
                    return;
                };
                if tx.send(msg).is_err() {
                    return;
                }
            }
        });
        Ok(Self {
            stream: ours,
            inbox,
        })
    }

    fn send(&mut self, text: &str) -> Res<()> {
        write_frame(&mut self.stream, text.as_bytes())?;
        Ok(self.stream.flush()?)
    }

    fn next(&self) -> Res<Value> {
        Ok(self.inbox.recv_timeout(WAIT)?)
    }

    /// The next message that is not a notification.
    fn next_response(&self) -> Res<Value> {
        loop {
            let msg = self.next()?;
            if msg.get("id").is_some() {
                return Ok(msg);
            }
        }
    }

    fn initialize(&mut self) -> Res<()> {
        self.send(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#)?;
        let reply = self.next_response()?;
        if reply.get("result").is_none() {
            return Err(format!("initialize failed: {reply:?}").into());
        }
        self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#)
    }
}

fn stats(client: &mut Client) -> Res<Value> {
    client.send(r#"{"jsonrpc":"2.0","id":"stats","method":"fake/stats"}"#)?;
    let reply = client.next_response()?;
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| "no stats".into())
}

#[test]
fn should_share_one_backend_and_keep_ids_apart_when_two_clients_use_the_same_ids() -> Res<()> {
    // given
    let session = start(TestOwner::default(), Duration::from_secs(600))?;
    let mut a = Client::connect(&session, 1)?;
    let mut b = Client::connect(&session, 2)?;
    a.initialize()?;
    b.initialize()?;

    // when: both send id 1, then id 2
    a.send(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"from":"a1"}}"#)?;
    b.send(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"from":"b1"}}"#)?;
    b.send(r#"{"jsonrpc":"2.0","id":2,"method":"fake/echo","params":{"from":"b2"}}"#)?;
    a.send(r#"{"jsonrpc":"2.0","id":2,"method":"fake/echo","params":{"from":"a2"}}"#)?;

    // then: each sees exactly its own answers, under its own ids, and one initialize reached
    // the backend
    for (client, who) in [(&a, "a"), (&b, "b")] {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let reply = client.next_response()?;
            let id = reply.get("id").cloned().unwrap();
            let from = reply
                .get("result")
                .unwrap()
                .get("from")
                .and_then(Value::as_str)
                .unwrap()
                .to_owned();
            seen.push((id, from));
        }
        seen.sort_by(|x, y| x.1.cmp(&y.1));
        assert_eq!(
            seen,
            vec![
                (Value::from(1), format!("{who}1")),
                (Value::from(2), format!("{who}2"))
            ]
        );
    }
    assert_eq!(stats(&mut a)?.get("initializeCount"), Some(&Value::from(1)));
    assert!(a.inbox.try_recv().is_err() && b.inbox.try_recv().is_err());
    Ok(())
}

#[test]
fn should_keep_serving_the_others_when_a_client_exits_mid_request() -> Res<()> {
    // given: a holds a request the backend has not answered
    let session = start(TestOwner::default(), Duration::from_secs(600))?;
    let mut a = Client::connect(&session, 1)?;
    let mut b = Client::connect(&session, 2)?;
    a.initialize()?;
    b.initialize()?;
    a.send(r#"{"jsonrpc":"2.0","id":7,"method":"fake/slow"}"#)?;
    let started = loop {
        let msg = a.next()?;
        if msg.get("method").and_then(Value::as_str) == Some("fake/slowStarted") {
            break msg;
        }
    };
    let backend_id = started.get("params").unwrap().get("id").cloned().unwrap();

    // when: a's process dies (its socket closes without shutdown or exit)
    a.stream.shutdown(Shutdown::Both).unwrap();
    drop(a);

    // then: the backend got the cancel for a's request, and b is still served
    let cancelled = loop {
        let msg = b.next()?;
        if msg.get("method").and_then(Value::as_str) == Some("fake/cancelled") {
            break msg.get("params").unwrap().get("id").cloned().unwrap();
        }
    };
    assert_eq!(cancelled, backend_id);
    b.send(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"from":"b"}}"#)?;
    let reply = b.next_response()?;
    assert_eq!(
        reply
            .get("result")
            .unwrap()
            .get("from")
            .and_then(Value::as_str),
        Some("b")
    );
    assert!(!session.thread.is_finished());
    Ok(())
}

#[test]
fn should_keep_the_backend_when_a_client_sends_shutdown_and_exit() -> Res<()> {
    // given
    let session = start(TestOwner::default(), Duration::from_secs(600))?;
    let mut a = Client::connect(&session, 1)?;
    let mut b = Client::connect(&session, 2)?;
    a.initialize()?;
    b.initialize()?;

    // when
    a.send(r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#)?;
    let shut = a.next_response()?;
    a.send(r#"{"jsonrpc":"2.0","method":"exit"}"#)?;

    // then: a's shutdown was answered locally, its connection closed, and b still works
    assert_eq!(shut.get("result"), Some(&Value::Null));
    assert!(
        matches!(
            a.inbox.recv_timeout(WAIT),
            Err(RecvTimeoutError::Disconnected)
        ),
        "a's stream ends"
    );
    let s = stats(&mut b)?;
    assert_eq!(s.get("initializeCount"), Some(&Value::from(1)));
    Ok(())
}

#[test]
fn should_stop_the_backend_gracefully_when_idle_past_the_timeout() -> Res<()> {
    // given
    let owner = TestOwner {
        evictable: true,
        ..TestOwner::default()
    };
    let log = Arc::clone(&owner.log);
    let session = start(owner, Duration::from_millis(50))?;
    let mut a = Client::connect(&session, 1)?;
    a.initialize()?;

    // when: the only client leaves and the session idles
    a.stream.shutdown(Shutdown::Both).unwrap();
    session.thread.join().unwrap();

    // then
    assert_eq!(*log.lock().unwrap(), vec!["evicted", "ended"]);
    Ok(())
}

#[test]
fn should_answer_with_an_error_when_the_backend_dies_mid_request() -> Res<()> {
    // given: a backend that reads one frame and then dies
    let (ours, theirs) = UnixStream::pair().unwrap();
    thread::spawn(move || {
        let mut reader = BufReader::new(theirs.try_clone().unwrap());
        let _ = read_frame(&mut reader, MAX_FRAME_BYTES);
        let _ = theirs.shutdown(Shutdown::Both);
    });
    let backend = Backend {
        reader: Box::new(ours.try_clone().unwrap()),
        writer: Box::new(ours),
        child: None,
    };
    let session = session::start(
        config(Duration::from_secs(600)),
        backend,
        TestOwner::default(),
    )?;
    let mut a = Client::connect(&session, 1)?;

    // when
    a.send(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#)?;

    // then
    let reply = a.next_response()?;
    let code = reply.get("error").unwrap().get("code").cloned();
    assert_eq!(code, Some(Value::from(-32603)));
    session.thread.join().unwrap();
    Ok(())
}

#[test]
fn should_answer_with_an_error_when_the_backend_answer_does_not_decode() -> Res<()> {
    // given
    let session = start(TestOwner::default(), Duration::from_secs(600))?;
    let mut a = Client::connect(&session, 1)?;
    a.initialize()?;

    // when: the backend's answer is not JSON
    a.send(r#"{"jsonrpc":"2.0","id":"g","method":"fake/garbage"}"#)?;
    let reply = a.next_response()?;

    // then: the client gets an error under its own id, and keeps being served
    assert_eq!(reply["id"], "g");
    assert_eq!(reply["error"]["code"], -32603);
    a.send(r#"{"jsonrpc":"2.0","id":2,"method":"fake/echo","params":{"ok":1}}"#)?;
    assert_eq!(a.next_response()?["result"]["ok"], 1);
    Ok(())
}

#[test]
fn should_answer_a_parse_error_and_keep_the_client_when_its_message_does_not_decode() -> Res<()> {
    // given
    let session = start(TestOwner::default(), Duration::from_secs(600))?;
    let mut a = Client::connect(&session, 1)?;
    a.initialize()?;

    // when
    a.send(r#"{"jsonrpc":"2.0","id":7,"method":"x","params":tru}"#)?;
    let reply = a.next_response()?;

    // then
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], -32700);
    a.send(r#"{"jsonrpc":"2.0","id":8,"method":"fake/echo","params":{"ok":1}}"#)?;
    assert_eq!(a.next_response()?["result"]["ok"], 1);
    Ok(())
}
