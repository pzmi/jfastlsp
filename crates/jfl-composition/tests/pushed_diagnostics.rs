//! Pushed diagnostics between a real daemon (this crate's test daemon) and a proxy that watches
//! (LSP-04): the proxy's `seq` rule, what it holds when a notification could not be sent, and a
//! re-sent document's version, each order forced by reading the connection only once the daemon's
//! frame is known to be there; and `jfl lsp`'s link clearing a closed document with no daemon. No sleeps: every wait is a
//! bounded `poll(2)` on the connection, or the testkit's `poll` helper for a process exit.

#[cfg(test)]
mod given {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use jfl_adapter_ipc::{SocketPath, StateDir};
    use jfl_composition::daemon::proxy::{Budget, Launcher, Proxy, Spawned};
    use jfl_composition::daemon::{DetachedDaemon, ProcessLauncher};
    use jfl_domain::PositionEncoding;
    use jfl_protocol::DocumentUri;
    use jfl_protocol::messages::{ClientKind, PublishDiagnosticsParams};

    const PROGRAM: &str = env!("CARGO_BIN_EXE_jfl-composition-test-daemon");
    pub(crate) const WAIT: Duration = Duration::from_secs(20);
    pub(crate) const BROKEN: &str = "class A {\n  int x = 1\n}\n";
    pub(crate) const FIXED: &str = "class A {\n  int x = 1;\n}\n";
    pub(crate) const FIXED_TOO: &str = "class A {\n  int x = 2;\n}\n";

    static NEXT: AtomicU32 = AtomicU32::new(0);

    type Children = Arc<Mutex<Vec<DetachedDaemon>>>;

    /// A launcher that keeps every daemon it starts, so the test can wait for and reap each.
    pub(crate) struct Recording {
        launcher: ProcessLauncher,
        children: Children,
    }

    /// The proxy's view of a recorded daemon: whether it has exited is read from the recording.
    struct Handle(u32, Children);

    impl Spawned for Handle {
        fn exited(&mut self) -> bool {
            self.1
                .lock()
                .unwrap()
                .iter_mut()
                .find(|c| c.id() == self.0)
                .is_none_or(|c| !matches!(c.try_exit(), Ok(None)))
        }
    }

    impl Launcher for Recording {
        fn launch(&mut self, notify: &Path) -> io::Result<Box<dyn Spawned>> {
            let child = self.launcher.spawn(notify)?;
            let pid = child.id();
            self.children.lock().unwrap().push(child);
            Ok(Box::new(Handle(pid, Arc::clone(&self.children))))
        }
    }

    /// A scratch directory with a state directory; on drop every daemon still running is killed
    /// and reaped, and the directory removed.
    pub(crate) struct World {
        root: PathBuf,
        state: StateDir,
        children: Children,
    }

    impl World {
        pub(crate) fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("jfl-pushed-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let state = StateDir::open(&root.join("s")).unwrap();
            Self {
                root,
                state,
                children: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub(crate) fn state_path(&self) -> &Path {
            self.state.path()
        }

        pub(crate) fn uri(&self, name: &str) -> DocumentUri {
            DocumentUri::parse(&format!("file://{}/{name}", self.root.display())).unwrap()
        }

        /// A proxy that watches diagnostics in UTF-16, its spawns recorded.
        pub(crate) fn proxy(&self) -> Proxy<Recording> {
            Proxy::new(
                "pushed",
                ClientKind::Lsp,
                self.state.clone(),
                SocketPath::of_state(&self.state).unwrap(),
                Recording {
                    launcher: ProcessLauncher::new(Path::new(PROGRAM), self.state.clone()),
                    children: Arc::clone(&self.children),
                },
                Budget {
                    reply: WAIT,
                    spawn: WAIT,
                },
            )
            .with_diagnostics(PositionEncoding::Utf16)
        }

        /// Asks the running daemon to stop and waits for its exit.
        pub(crate) fn stop(&self) {
            assert!(jfl_composition::daemon::stop_running(
                self.state.path(),
                WAIT
            ));
            let children = Arc::clone(&self.children);
            jfl_testkit::poll::wait_until(WAIT, "every daemon started to exit", || {
                children
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .all(|c| matches!(c.try_exit(), Ok(Some(_)) | Err(_)))
            })
            .unwrap();
        }

        pub(crate) fn spawned(&self) -> usize {
            self.children.lock().unwrap().len()
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            for child in self.children.lock().unwrap().iter_mut() {
                if matches!(child.try_exit(), Ok(None)) {
                    let _ = jfl_testkit::signal::send(child.id(), "KILL");
                    let _ = jfl_testkit::poll::wait_until(WAIT, "a killed daemon to exit", || {
                        !matches!(child.try_exit(), Ok(None))
                    });
                }
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Waits until the daemon has sent something on `proxy`'s connection (or closed it), without
    /// reading it.
    #[track_caller]
    pub(crate) fn readable(proxy: &Proxy<Recording>) {
        let at = std::panic::Location::caller();
        let (_ringer, bell) = jfl_adapter_ipc::bell::bell().unwrap();
        let connected = proxy.fd().is_some();
        let started = std::time::Instant::now();
        let woken = bell.wait(proxy.fd(), WAIT).unwrap();
        let elapsed = started.elapsed();
        assert!(
            woken.readable,
            "the daemon sent nothing (waited at {at}: {elapsed:?} of {WAIT:?}, interrupted: {}, \
             connected: {connected}, phase: {:?})",
            elapsed < WAIT,
            proxy.phase()
        );
    }

    /// The bytes on the wire of a publication of `uri` with no diagnostics, at the client's
    /// `version` and the connection's `seq` (one notification per version): the daemon's own
    /// encoding (`daemon::diagnostics`), framed.
    pub(crate) fn empty_frame_len(uri: &DocumentUri, version: i32) -> u64 {
        let body = jfl_protocol::codec::encode(&jfl_protocol::codec::notification::<
            jfl_protocol::messages::PublishDiagnostics,
        >(PublishDiagnosticsParams {
            uri: uri.clone(),
            version: Some(version),
            seq: u64::try_from(version).unwrap(),
            position_encoding: PositionEncoding::Utf16,
            diagnostics: Vec::new(),
        }))
        .unwrap();
        let frame =
            jfl_protocol::frame::encode_frame(&body, jfl_protocol::DEFAULT_MAX_FRAME_LEN).unwrap();
        u64::try_from(frame.len()).unwrap()
    }

    /// Waits until at least `bytes` wait unread on `proxy`'s connection (`FIONREAD`), without
    /// reading them.
    pub(crate) fn queued_at_least(proxy: &Proxy<Recording>, bytes: u64) {
        jfl_testkit::poll::wait_until(WAIT, "the daemon's frames on the connection", || {
            proxy
                .fd()
                .is_some_and(|fd| rustix::io::ioctl_fionread(fd).is_ok_and(|n| n >= bytes))
        })
        .unwrap();
    }

    /// Reads what came and takes the current publications, waiting for the first one.
    #[track_caller]
    pub(crate) fn next_published(proxy: &mut Proxy<Recording>) -> Vec<PublishDiagnosticsParams> {
        loop {
            readable(proxy);
            proxy.poll();
            let got = proxy.take_published();
            if !got.is_empty() {
                return got;
            }
        }
    }
}

use given::*;

#[test]
fn should_drop_a_publication_when_it_is_older_than_the_documents_last_notification() {
    // given: the proxy has read version 1's publication and not handed it on yet
    let world = World::new();
    let mut proxy = world.proxy();
    let doc = world.uri("A.java");
    let _ = proxy.did_open(doc.clone(), 1, BROKEN.to_owned());
    proxy.attach(WAIT).unwrap();
    readable(&proxy);
    proxy.poll();

    // when: the client's newer text goes before the old publication is handed on (no read in
    // between, so version 2's publication cannot be among what is taken, however fast it comes)
    assert!(proxy.did_change(doc.clone(), 2, FIXED.to_owned()));
    let stale = proxy.take_published();
    let fresh = next_published(&mut proxy);

    // then: version 1's errors never reach the client; version 2's empty set does
    assert!(stale.is_empty(), "{stale:?}");
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].version, Some(2));
    assert!(fresh[0].diagnostics.is_empty());
    world.stop();
}

#[test]
fn should_hand_on_only_the_newer_publication_when_both_arrive_before_the_proxy_reads() {
    // given: version 1's publication waits unread on the connection
    let world = World::new();
    let mut proxy = world.proxy();
    let doc = world.uri("A.java");
    let _ = proxy.did_open(doc.clone(), 1, FIXED.to_owned());
    proxy.attach(WAIT).unwrap();
    readable(&proxy);

    // when: the client's newer text goes, and its publication too is on the connection, whole,
    // before the proxy reads (the order a loaded machine gave the old test, forced)
    assert!(proxy.did_change(doc.clone(), 2, FIXED_TOO.to_owned()));
    let both = empty_frame_len(&doc, 1) + empty_frame_len(&doc, 2);
    queued_at_least(&proxy, both);
    proxy.poll();
    let handed = proxy.take_published();

    // then: one read takes both, and only version 2's is handed on
    assert_eq!(handed.len(), 1, "{handed:?}");
    assert_eq!(handed[0].version, Some(2));
    world.stop();
}

#[test]
fn should_publish_at_the_clients_version_when_a_restarted_daemon_gets_the_document_again() {
    // given: a document at version 5, published by the first daemon
    let world = World::new();
    let mut proxy = world.proxy();
    let doc = world.uri("A.java");
    let _ = proxy.did_open(doc.clone(), 1, FIXED.to_owned());
    let _ = proxy.did_change(doc.clone(), 5, BROKEN.to_owned());
    proxy.attach(WAIT).unwrap();
    let first = next_published(&mut proxy);

    // when: the daemon stops; the proxy notices, starts another and re-sends the document
    world.stop();
    readable(&proxy);
    proxy.poll();
    proxy.attach(WAIT).unwrap();
    let again = next_published(&mut proxy);

    // then: obligation 11, the client's version kept, never renumbered
    assert_eq!(world.spawned(), 2);
    assert_eq!(first[0].version, Some(5));
    assert_eq!(again[0].version, Some(5));
    assert_eq!(again[0].diagnostics.len(), 1);
    world.stop();
}

#[test]
fn should_hand_on_nothing_older_when_a_change_could_not_be_sent_after_an_abandoned_call() {
    // given: version 1's publication is held, then a call gives up (its connection dropped)
    let world = World::new();
    let mut proxy = world.proxy();
    let doc = world.uri("A.java");
    let _ = proxy.did_open(doc.clone(), 1, BROKEN.to_owned());
    proxy.attach(WAIT).unwrap();
    readable(&proxy);
    proxy.poll();
    proxy.abandon();

    // when: the client's next version cannot reach the daemon now
    let sent = proxy.did_change(doc.clone(), 2, FIXED.to_owned());
    let held = proxy.take_published();
    proxy.attach(WAIT).unwrap();
    let fresh = next_published(&mut proxy);

    // then: version 1 is never handed on after version 2; the re-sent version 2 is
    assert!(!sent);
    assert!(held.is_empty(), "{held:?}");
    assert_eq!(fresh[0].version, Some(2));
    assert!(fresh[0].diagnostics.is_empty());
    world.stop();
}

#[test]
fn should_hand_on_nothing_of_the_document_when_its_close_could_not_be_sent_after_an_abandoned_call()
{
    // given: a publication held for a document, then a call gives up
    let world = World::new();
    let mut proxy = world.proxy();
    let doc = world.uri("A.java");
    let _ = proxy.did_open(doc.clone(), 1, BROKEN.to_owned());
    proxy.attach(WAIT).unwrap();
    readable(&proxy);
    proxy.poll();
    proxy.abandon();

    // when: the client closes it while the proxy is disconnected
    let sent = proxy.did_close(&doc);
    let held = proxy.take_published();

    // then: nothing of the closed document is handed on (the link clears it itself)
    assert!(!sent);
    assert!(held.is_empty(), "{held:?}");
    world.stop();
}

#[test]
fn should_publish_the_empty_set_when_a_document_closes_and_no_daemon_can_be_reached() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU8;
    use std::sync::mpsc::sync_channel;
    use std::thread;

    use jfl_adapter_lsp::memory::pipe;
    use jfl_adapter_lsp::{Config, serve};
    use jfl_composition::daemon::lsp::{LINK_QUEUE, LspConfig, ProxyDaemon, Session, link};
    use jfl_protocol::frame::FrameDecoder;
    use serde_json::{Value, json};

    // given: `jfl lsp`'s session and link whose daemon cannot start (no such program)
    let world = World::new();
    let config = LspConfig {
        state_dir: world.state_path().to_owned(),
        idle: None,
    };
    let missing = world.state_path().join("no-such-jfl");
    let (ringer, bell) = jfl_adapter_ipc::bell::bell().unwrap();
    let (items, queue) = sync_channel(LINK_QUEUE);
    thread::spawn(move || {
        let mut daemon = ProxyDaemon::new(config, &missing, "nodaemon").with_bell(bell);
        link(&queue, &mut daemon);
    });
    let session = Session::new(items, Arc::new(AtomicU8::new(0)), "0.0.0").with_ringer(ringer);
    let (mut to_server, server_in) = pipe();
    let (server_out, mut from_server) = pipe();
    let server = thread::spawn(move || serve(server_in, server_out, session, &Config::default()));
    let mut send = |message: Value| {
        let body = message.to_string();
        let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        to_server.write_all(frame.as_bytes()).unwrap();
    };
    let uri = "file:///w/src/A.java";

    // when
    send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"capabilities": {"textDocument": {"publishDiagnostics": {}}}}}));
    send(
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument":
        {"uri": uri, "languageId": "java", "version": 1, "text": BROKEN}}}),
    );
    send(json!({"jsonrpc": "2.0", "method": "textDocument/didClose",
        "params": {"textDocument": {"uri": uri}}}));
    let mut decoder = FrameDecoder::new(1 << 20);
    let mut pending = Vec::new();
    let mut got = Vec::new();
    while got.len() < 2 {
        let progress = decoder.feed(&pending).unwrap();
        pending.drain(..progress.consumed);
        if let Some(body) = progress.frame {
            got.push(serde_json::from_slice::<Value>(&body).unwrap());
            continue;
        }
        let mut chunk = [0_u8; 4096];
        let n = from_server.read(&mut chunk).unwrap();
        assert!(n > 0, "the server's output ended");
        pending.extend_from_slice(&chunk[..n]);
    }
    drop(send);
    drop(to_server);
    let _ended = server.join().unwrap();

    // then: the initialize answer, then the cleared document though nothing reached a daemon
    assert_eq!(got[0]["id"], json!(1));
    assert_eq!(
        got[1],
        json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "diagnostics": []}})
    );
}
