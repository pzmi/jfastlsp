//! `jfl lsp` end to end over a live session (ver §2, e2e): what a transcript cannot say because a
//! step must wait for an answer before the next one. The daemon a session started is killed
//! mid-session, and the next request is answered by a daemon started again with the session's
//! open documents restored (plan §5.2); a disk written after an open document arrived wins over
//! it (plan §4.4). Each daemon is found through its socket (`jfl/status`), killed by its pid, and
//! the last one stopped when the test's world is dropped.

#[cfg(test)]
mod support;

#[cfg(test)]
mod given {
    use std::path::Path;
    use std::time::Duration;

    use jfl_testkit::lsp_transcript::Live;
    use serde_json::{Value, json};

    use super::support::{DEADLINE, World};

    pub(crate) const MAIN_TEXT: &str =
        include_str!("../../../fixtures/lsp/worktree/src/demo/Main.java");

    /// How long a status probe waits for the daemon.
    pub(crate) const PROBE: Duration = Duration::from_secs(5);

    /// `jfl lsp` started in `world` for a live session.
    pub(crate) fn live(world: &World) -> Live {
        let (args, path) = world.command();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Live::start(
            Path::new(env!("CARGO_BIN_EXE_jfl")),
            &args,
            &[("PATH", path.as_os_str())],
            &world.scratch().join("live"),
        )
        .unwrap()
    }

    pub(crate) fn initialize() -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}})
    }

    pub(crate) fn open(world: &World, version: i32, text: &str) -> Value {
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {
            "uri": world.substitute("file://{{root}}/src/demo/Main.java"),
            "languageId": "java", "version": version, "text": text}}})
    }

    /// `textDocument/definition` of Main.java at `line`, on the call of `tick` (76 UTF-16 units
    /// in, after two astral characters).
    pub(crate) fn tick(world: &World, id: u32, line: u32) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/definition", "params": {
            "textDocument": {"uri": world.substitute("file://{{root}}/src/demo/Main.java")},
            "position": {"line": line, "character": 76}}})
    }

    /// `tick`'s declaration, as the answer names it (in Main.java, as the request spelled it), on
    /// the disk's text, the line `extra` lines down.
    pub(crate) fn tick_declared(world: &World, extra: u32) -> Value {
        let line = 5_u32.saturating_add(extra);
        json!([{"uri": world.substitute("file://{{root}}/src/demo/Main.java"),
            "range": {"start": {"line": line, "character": 19}, "end": {"line": line, "character": 23}}}])
    }

    pub(crate) fn shutdown_and_exit(lsp: &mut Live, id: u32) {
        lsp.send(&json!({"jsonrpc": "2.0", "id": id, "method": "shutdown"}))
            .unwrap();
        assert_eq!(lsp.next(DEADLINE).unwrap()["result"], Value::Null);
        lsp.send(&json!({"jsonrpc": "2.0", "method": "exit"}))
            .unwrap();
    }
}

use given::{MAIN_TEXT, PROBE, initialize, live, open, shutdown_and_exit, tick, tick_declared};
use jfl_testkit::fixtures::git::Repo;
use jfl_testkit::poll::wait_until;
use serde_json::json;
use support::{DEADLINE, World};

#[test]
fn should_answer_from_a_restarted_daemon_with_the_open_documents_when_the_daemon_was_killed() {
    // given: an open document that differs from the disk (one line added on top), which the
    // daemon holds as the connection's overlay and so refuses to answer from the disk
    let world = World::new();
    let mut lsp = live(&world);
    lsp.send(&initialize()).unwrap();
    assert_eq!(lsp.next(DEADLINE).unwrap()["id"], 1);
    let edited = format!("// a line the disk does not have\n{MAIN_TEXT}");
    lsp.send(&open(&world, 1, &edited)).unwrap();
    lsp.send(&tick(&world, 2, 13)).unwrap();
    let before = lsp.next(DEADLINE).unwrap();
    let first = jfl_composition::daemon::running_pid(&world.state(), PROBE).unwrap();

    // when: the daemon is killed, then the client asks again
    assert!(jfl_testkit::signal::send(first, "KILL"));
    wait_until(DEADLINE, "the killed daemon to stop answering", || {
        jfl_composition::daemon::running_pid(&world.state(), PROBE).is_none()
    })
    .unwrap();
    lsp.send(&tick(&world, 3, 13)).unwrap();
    let after = lsp.next(DEADLINE).unwrap();
    let second = jfl_composition::daemon::running_pid(&world.state(), PROBE).unwrap();
    lsp.send(&json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
        "textDocument": {"uri": world.substitute("file://{{root}}/src/demo/Main.java"), "version": 2},
        "contentChanges": [{"range": {"start": {"line": 0, "character": 0},
            "end": {"line": 1, "character": 0}}, "text": ""}]}}))
        .unwrap();
    lsp.send(&tick(&world, 4, 12)).unwrap();
    let synced = lsp.next(DEADLINE).unwrap();
    shutdown_and_exit(&mut lsp, 5);

    // then: the new daemon holds the re-sent document (ContentModified again, not an answer
    // from the disk), and answers once the document equals the disk
    assert_eq!(before["error"]["code"], -32801, "{before}");
    assert_eq!(after["id"], 3);
    assert_eq!(after["error"]["code"], -32801, "{after}");
    assert_ne!(first, second);
    assert_eq!(synced["id"], 4);
    assert_eq!(synced["result"], tick_declared(&world, 0), "{synced}");
    assert_eq!(lsp.finish(DEADLINE).unwrap(), Some(0));
}

#[test]
fn should_answer_from_the_disk_when_it_was_written_after_the_open_document() {
    // given: an open document the disk does not hold
    let world = World::new();
    let mut lsp = live(&world);
    lsp.send(&initialize()).unwrap();
    assert_eq!(lsp.next(DEADLINE).unwrap()["id"], 1);
    let edited = format!("// a line the disk does not have\n{MAIN_TEXT}");
    lsp.send(&open(&world, 1, &edited)).unwrap();
    lsp.send(&tick(&world, 2, 13)).unwrap();
    let refused = lsp.next(DEADLINE).unwrap();

    // when: an agent's tool writes the file (another line on top), the editor never told; the
    // client asks at a position of its own text that is a position of the disk's too
    Repo::open(&world.root)
        .write(
            "src/demo/Main.java",
            format!("// another line\n{MAIN_TEXT}").as_bytes(),
        )
        .unwrap();
    lsp.send(&tick(&world, 3, 13)).unwrap();
    let answered = lsp.next(DEADLINE).unwrap();
    shutdown_and_exit(&mut lsp, 4);

    // then: the disk wins (plan §4.4): answered from it, no longer ContentModified
    assert_eq!(refused["error"]["code"], -32801, "{refused}");
    assert_eq!(answered["id"], 3);
    assert_eq!(answered["result"], tick_declared(&world, 1), "{answered}");
    assert_eq!(lsp.finish(DEADLINE).unwrap(), Some(0));
}
