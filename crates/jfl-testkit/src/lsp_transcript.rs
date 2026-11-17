//! The LSP conformance level's transcript runner (ver §2, §4): a scripted session against a
//! server binary over stdio, black box.
//!
//! A transcript is a `fixtures/lsp/<scenario>.jsonl` file, one JSON object per line, each with
//! exactly one member:
//!
//! - `{"comment": "…"}`: what the scenario pins; ignored.
//! - `{"send": {…}}`: a client message, written as compact JSON in a frame with a correct
//!   `Content-Length`.
//! - `{"send_raw": "…"}`: the string's UTF-8 bytes, written as they are (a malformed header).
//! - `{"send_oversized": N}`: a header declaring `N` bytes, then `N` bytes of spaces.
//! - `{"expect": {…}}`: a message the server must send.
//! - `{"exit": N}`: the server's exit code once its input ended; when no line says, the code is not
//!   checked (LSP-02 states it in its lifecycle transcripts: 0 after `shutdown`, 1 without).
//!
//! **Running.** Every client line goes into a file that becomes the server's standard input, so
//! the input ends after the last one; standard output and standard error go to files. The server
//! must exit within the deadline (it is killed otherwise and the run fails), and its output is then
//! read whole. Files rather than pipes, so no thread is needed and a server that writes more than a
//! pipe holds cannot deadlock the run. The scratch directory is the caller's
//! (`CARGO_TARGET_TMPDIR`), one subdirectory per transcript.
//!
//! **Normalisation**, the only differences a comparison forgives:
//!
//! 1. Messages are compared as JSON values: member order and whitespace do not count.
//! 2. An expected string `"<any>"` matches any value (used for error messages, which are words for
//!    people, not part of the contract).
//! 3. Responses with a non-null `id` are matched to the expected response with the same `id`, in
//!    any order (requests run concurrently, and LSP lets responses come in any order); every other
//!    server message (a response with a null `id`, a notification, a request to the client) is
//!    matched in order.
//! 4. Every server message must be expected and every expected one sent: nothing extra, nothing
//!    missing.
//!
//! Pinned by the unit tests below and `crates/jfl-adapter-lsp/tests/lsp_transcripts.rs` (with its
//! negative control).

use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use jfl_protocol::frame::{DEFAULT_MAX_FRAME_LEN, FrameDecoder};
use serde_json::Value;

use crate::poll::wait_until;

/// An expected string that matches any value.
pub const ANY: &str = "<any>";

/// One scripted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The file's stem, also the scratch subdirectory's name.
    pub name: String,
    /// The client's bytes, in order.
    pub input: Vec<u8>,
    /// The server's messages.
    pub expected: Vec<Value>,
    /// The exit code the server must end with, when the transcript says.
    pub exit: Option<i32>,
}

/// A transcript that cannot be read, or a run that did not match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

fn fail(message: impl Into<String>) -> Failure {
    Failure(message.into())
}

/// Parses a transcript's text; `name` is for messages and the scratch directory.
pub fn parse(name: &str, text: &str) -> Result<Transcript, Failure> {
    let mut input = Vec::new();
    let mut expected = Vec::new();
    let mut exit = None;
    for (number, line) in text.lines().enumerate() {
        let at = || format!("{name}:{}", number.saturating_add(1));
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(line).map_err(|e| fail(format!("{}: not JSON: {e}", at())))?;
        let Some((key, value)) = value.as_object().and_then(|o| {
            let mut members = o.iter();
            let first = members.next();
            if members.next().is_some() {
                None
            } else {
                first
            }
        }) else {
            return Err(fail(format!("{}: not an object with one member", at())));
        };
        match (key.as_str(), value) {
            ("comment", _) => {}
            ("send", message) => {
                let body = serde_json::to_vec(message).map_err(|e| fail(e.to_string()))?;
                let _ = write!(Header(&mut input), "Content-Length: {}\r\n\r\n", body.len());
                input.extend(body);
            }
            ("send_raw", Value::String(raw)) => input.extend(raw.as_bytes()),
            ("send_oversized", Value::Number(n)) => {
                let len = n
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| fail(format!("{}: send_oversized is not a length", at())))?;
                let _ = write!(Header(&mut input), "Content-Length: {len}\r\n\r\n");
                input.resize(input.len().saturating_add(len), b' ');
            }
            ("expect", message @ Value::Object(_)) => expected.push(message.clone()),
            ("exit", Value::Number(n)) if exit.is_none() => {
                let code = n
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or_else(|| fail(format!("{}: exit is not an exit code", at())))?;
                exit = Some(code);
            }
            (other, _) => return Err(fail(format!("{}: unknown step `{other}`", at()))),
        }
    }
    Ok(Transcript {
        name: name.to_owned(),
        input,
        expected,
        exit,
    })
}

/// Appends formatted text to a byte buffer.
struct Header<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Header<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend(s.as_bytes());
        Ok(())
    }
}

/// Every `*.jsonl` transcript in `dir`, by name.
pub fn load_dir(dir: &Path) -> Result<Vec<Transcript>, Failure> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| fail(format!("{}: {e}", dir.display())))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| fail(format!("{}: not a UTF-8 name", path.display())))?;
            let text =
                fs::read_to_string(path).map_err(|e| fail(format!("{}: {e}", path.display())))?;
            parse(name, &text)
        })
        .collect()
}

/// Runs `transcript` against `program` with `args` (environment cleared) and checks its output.
pub fn run(
    program: &Path,
    args: &[&str],
    transcript: &Transcript,
    scratch: &Path,
    deadline: Duration,
) -> Result<(), Failure> {
    let io = |what: &str, e: std::io::Error| fail(format!("{}: {what}: {e}", transcript.name));
    let dir = scratch.join(&transcript.name);
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| io("clearing the scratch directory", e))?;
    }
    fs::create_dir_all(&dir).map_err(|e| io("creating the scratch directory", e))?;
    let (stdin, stdout, stderr) = (dir.join("stdin"), dir.join("stdout"), dir.join("stderr"));
    fs::write(&stdin, &transcript.input).map_err(|e| io("writing the input", e))?;
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::from(File::open(&stdin).map_err(|e| io("stdin", e))?))
        .stdout(Stdio::from(
            File::create(&stdout).map_err(|e| io("stdout", e))?,
        ))
        .stderr(Stdio::from(
            File::create(&stderr).map_err(|e| io("stderr", e))?,
        ))
        .spawn()
        .map_err(|e| io("starting the server", e))?;
    let mut status = None;
    let waited = wait_until(
        deadline,
        "the server to exit after its input ended",
        || match child.try_wait() {
            Ok(None) => false,
            Ok(Some(exit)) => {
                status = Some(exit.code());
                true
            }
            Err(_) => true,
        },
    );
    if let Err(timeout) = waited {
        let _ = child.kill();
        let _ = child.wait();
        let err = fs::read_to_string(&stderr).unwrap_or_default();
        return Err(fail(format!(
            "{}: {timeout}; stderr:\n{err}",
            transcript.name
        )));
    }
    let output = fs::read(&stdout).map_err(|e| io("reading the output", e))?;
    let got = frames(&output).map_err(|e| fail(format!("{}: {e}", transcript.name)))?;
    compare(&transcript.expected, &got).map_err(|e| fail(format!("{}: {e}", transcript.name)))?;
    match (transcript.exit, status) {
        (Some(want), Some(Some(code))) if want == code => Ok(()),
        (Some(want), got) => Err(fail(format!(
            "{}: the server exited with {got:?}, not {want}",
            transcript.name
        ))),
        (None, _) => Ok(()),
    }
}

/// The JSON bodies of the frames in `bytes`, which must end between frames.
pub fn frames(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let mut decoder = FrameDecoder::new(DEFAULT_MAX_FRAME_LEN);
    let mut rest = bytes;
    let mut out = Vec::new();
    while !rest.is_empty() {
        let progress = decoder
            .feed(rest)
            .map_err(|e| format!("the server's framing: {e}"))?;
        if progress.oversized.is_some() {
            return Err("the server sent a frame over the limit".to_owned());
        }
        rest = rest.get(progress.consumed..).unwrap_or_default();
        if let Some(body) = progress.frame {
            out.push(serde_json::from_slice(&body).map_err(|e| format!("a server frame: {e}"))?);
        }
    }
    decoder
        .finish()
        .map_err(|e| format!("the server's output ends inside a frame: {e}"))?;
    Ok(out)
}

/// A response's id, when it is a response with a non-null id.
fn response_id(message: &Value) -> Option<&Value> {
    let object = message.as_object()?;
    let is_response = !object.contains_key("method")
        && (object.contains_key("result") || object.contains_key("error"));
    object.get("id").filter(|id| is_response && !id.is_null())
}

/// Whether `actual` is `expected` up to normalisation rule 2.
#[must_use]
pub fn matches(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::String(any), _) if any == ANY => true,
        (Value::Object(e), Value::Object(a)) => {
            e.len() == a.len()
                && e.iter()
                    .all(|(key, ev)| a.get(key).is_some_and(|av| matches(ev, av)))
        }
        (Value::Array(e), Value::Array(a)) => {
            e.len() == a.len() && e.iter().zip(a).all(|(ev, av)| matches(ev, av))
        }
        _ => expected == actual,
    }
}

/// Checks `got` against `expected` by the normalisation rules of the module docs.
pub fn compare(expected: &[Value], got: &[Value]) -> Result<(), String> {
    let show = |values: &[&Value]| {
        values
            .iter()
            .map(|v| format!("  {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut unmatched: Vec<&Value> = got.iter().filter(|m| response_id(m).is_some()).collect();
    let mut missing = Vec::new();
    for want in expected.iter().filter(|m| response_id(m).is_some()) {
        let id = response_id(want);
        match unmatched.iter().position(|m| response_id(m) == id) {
            Some(at) if unmatched.get(at).is_some_and(|m| matches(want, m)) => {
                unmatched.remove(at);
            }
            _ => missing.push(want),
        }
    }
    let want_seq: Vec<&Value> = expected
        .iter()
        .filter(|m| response_id(m).is_none())
        .collect();
    let got_seq: Vec<&Value> = got.iter().filter(|m| response_id(m).is_none()).collect();
    let seq_ok = want_seq.len() == got_seq.len()
        && want_seq.iter().zip(&got_seq).all(|(w, g)| matches(w, g));
    if missing.is_empty() && unmatched.is_empty() && seq_ok {
        return Ok(());
    }
    let all: Vec<&Value> = got.iter().collect();
    Err(format!(
        "the server's messages differ from the transcript\nexpected responses not sent:\n{}\nresponses not expected:\n{}\nother messages, expected in order:\n{}\nother messages sent:\n{}\neverything sent:\n{}",
        show(&missing),
        show(&unmatched),
        show(&want_seq),
        show(&got_seq),
        show(&all)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn should_frame_each_send_when_a_transcript_is_parsed() {
        // given
        let text = r#"{"comment": "x"}
{"send": {"id": 1}}
{"send_raw": "Content-Length: 2\r\n\r\n{}"}
{"send_oversized": 3}
{"expect": {"id": 1, "result": null}}
{"exit": 1}
"#;

        // when
        let transcript = parse("t", text).unwrap();

        // then
        assert_eq!(
            transcript.input,
            b"Content-Length: 8\r\n\r\n{\"id\":1}Content-Length: 2\r\n\r\n{}Content-Length: 3\r\n\r\n   "
        );
        assert_eq!(transcript.expected, [json!({"id": 1, "result": null})]);
        assert_eq!(transcript.exit, Some(1));
    }

    #[test]
    fn should_refuse_a_line_when_it_is_not_one_known_step() {
        // given
        let texts = [
            r#"{"send": {}, "expect": {}}"#,
            r#"{"sned": {}}"#,
            "[1]",
            "{",
            r#"{"exit": 1.5}"#,
            "{\"exit\": 0}\n{\"exit\": 1}",
        ];

        for text in texts {
            // when
            let got = parse("t", text);

            // then
            assert!(got.is_err(), "{text}");
        }
    }

    #[test]
    fn should_match_responses_by_id_when_they_arrive_out_of_order() {
        // given
        let expected = [
            json!({"id": 1, "result": 1}),
            json!({"id": null, "error": {"code": -1, "message": ANY}}),
            json!({"id": "b", "result": 2}),
        ];
        let got = [
            json!({"id": "b", "result": 2}),
            json!({"id": null, "error": {"code": -1, "message": "words"}}),
            json!({"id": 1, "result": 1}),
        ];

        // when
        let outcome = compare(&expected, &got);

        // then
        assert_eq!(outcome, Ok(()));
    }

    #[test]
    fn should_fail_when_a_message_is_extra_missing_or_different() {
        // given
        let expected = [json!({"id": 1, "result": 1}), json!({"method": "n"})];
        let cases = [
            vec![json!({"id": 1, "result": 1})],
            vec![
                json!({"id": 1, "result": 1}),
                json!({"method": "n"}),
                json!({"id": 2, "result": 1}),
            ],
            vec![json!({"id": 1, "result": 2}), json!({"method": "n"})],
            vec![
                json!({"id": 1, "result": 1}),
                json!({"method": "n", "params": {}}),
            ],
            vec![json!({"id": "1", "result": 1}), json!({"method": "n"})],
        ];

        for got in cases {
            // when
            let outcome = compare(&expected, &got);

            // then
            assert!(outcome.is_err(), "{got:?}");
        }
    }

    #[test]
    fn should_read_every_frame_when_the_output_ends_between_frames() {
        // given
        let output = b"Content-Length: 2\r\n\r\n{}Content-Length: 4\r\n\r\nnull";

        // when
        let got = frames(output);
        let cut = frames(b"Content-Length: 4\r\n\r\nnu");

        // then
        assert_eq!(got, Ok(vec![json!({}), Value::Null]));
        assert!(cut.is_err());
    }
}
