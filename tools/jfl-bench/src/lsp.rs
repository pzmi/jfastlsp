//! A minimal LSP client for the comparison arms: `Content-Length` framing and a request loop over
//! a server's stdio, on the caller's thread.
//!
//! The harness holds no thread or timed-wait grant (fnd §2.4), so the client is synchronous: it
//! writes a request and reads messages until the response with that id, answering a server's
//! own requests ([`answer`]: `null`, or one `null` per item of a `workspace/configuration`) and
//! handing notifications to the caller's predicate. A server that
//! never answers is ended from outside: the arm runs it under `timeout(1)`, which closes the pipe,
//! and the read fails with end of file. Frames are bounded (a header line, the header count and
//! the body) before anything is allocated, because the peer is a program under test.
//!
//! Pinned by: this module's tests (framing limits, server requests answered, notifications seen,
//! end of file).

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{BenchError, Context, Result};

/// The largest message body read (64 MiB): a `references` answer over a large corpus is a few MiB.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// The longest header line read.
const MAX_HEADER_LINE: u64 = 1024;
/// The most header lines in one frame.
const MAX_HEADER_LINES: usize = 16;

/// Write one frame.
pub fn write_frame(w: &mut impl Write, body: &[u8]) -> Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", body.len()).context("write a frame header")?;
    w.write_all(body).context("write a frame body")?;
    w.flush().context("flush a frame")
}

/// Read one frame's body; `None` at end of input before a header starts.
pub fn read_frame(r: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    let mut lines = 0usize;
    loop {
        let mut line = Vec::new();
        let n = r
            .by_ref()
            .take(MAX_HEADER_LINE)
            .read_until(b'\n', &mut line)
            .context("read a frame header")?;
        if n == 0 {
            if lines == 0 {
                return Ok(None);
            }
            return Err(BenchError::new("end of input inside a frame header"));
        }
        if line.last() != Some(&b'\n') {
            return Err(BenchError::new("a frame header line is too long"));
        }
        lines = lines.saturating_add(1);
        if lines > MAX_HEADER_LINES {
            return Err(BenchError::new("too many frame header lines"));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            break;
        }
        if let Some((key, value)) = text.split_once(':')
            && key.trim().eq_ignore_ascii_case("content-length")
        {
            let v: usize = value
                .trim()
                .parse()
                .map_err(|_| BenchError::new(format!("bad Content-Length {value:?}")))?;
            length = Some(v);
        }
    }
    let length = length.context("a frame without Content-Length")?;
    if length > MAX_BODY_BYTES {
        return Err(BenchError::new(format!(
            "a frame of {length} bytes is over the {MAX_BODY_BYTES}-byte limit"
        )));
    }
    let mut body = vec![0u8; length];
    r.read_exact(&mut body).context("read a frame body")?;
    Ok(Some(body))
}

/// A client over one server's input and output.
pub struct Client<R, W> {
    reader: R,
    writer: W,
    next_id: u64,
}

impl<R: BufRead, W: Write> Client<R, W> {
    /// A client reading `reader` (the server's stdout) and writing `writer` (its stdin).
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 1,
        }
    }

    /// The writer, for a test to read what was sent.
    pub fn writer(&self) -> &W {
        &self.writer
    }

    fn send(&mut self, msg: &Value) -> Result<()> {
        let body = serde_json::to_vec(msg).context("encode a message")?;
        write_frame(&mut self.writer, &body)
    }

    /// Send a notification.
    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Read the next message of any kind, answering it first ([`answer`]) when it is a request
    /// from the server.
    fn next(&mut self) -> Result<Value> {
        let body = read_frame(&mut self.reader)?
            .context("the server closed its output (exited, or ended by the arm's deadline)")?;
        let msg: Value = serde_json::from_slice(&body).context("decode a message")?;
        if let (Some(id), Some(method)) = (msg.get("id"), msg.get("method")) {
            let result = answer(method.as_str().unwrap_or(""), msg.get("params"));
            let id = id.clone();
            self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}))?;
        }
        Ok(msg)
    }

    /// Wait for a notification `want` accepts, answering server requests meanwhile.
    pub fn wait_notification(&mut self, want: impl Fn(&str, &Value) -> bool) -> Result<Value> {
        loop {
            let msg = self.next()?;
            if msg.get("id").is_some() {
                continue;
            }
            let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            if want(method, &params) {
                return Ok(params);
            }
        }
    }

    /// Send a request and wait for its response: its `result`, or the `error` as a failure. The
    /// time is from the write to the response read, both included.
    pub fn request(&mut self, method: &str, params: Value) -> Result<(Value, f64)> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let t = Instant::now();
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let msg = self.next()?;
            if msg.get("method").is_some() || msg.get("id") != Some(&Value::from(id)) {
                continue;
            }
            let elapsed = t.elapsed().as_secs_f64() * 1e3;
            if let Some(err) = msg.get("error") {
                return Err(BenchError::new(format!("{method}: {err}")));
            }
            return Ok((msg.get("result").cloned().unwrap_or(Value::Null), elapsed));
        }
    }
}

/// The most `workspace/configuration` items answered; a server asks for a handful.
const MAX_CONFIGURATION_ITEMS: usize = 1024;

/// The result for a server's own request: for `workspace/configuration`, one `null` per item
/// asked (up to [`MAX_CONFIGURATION_ITEMS`]), which tells the server "no client setting" so it
/// keeps what `initializationOptions` gave it; for anything else (`client/registerCapability`,
/// `window/workDoneProgress/create`, ...), `null`.
pub fn answer(method: &str, params: Option<&Value>) -> Value {
    if method != "workspace/configuration" {
        return Value::Null;
    }
    let items = params
        .and_then(|p| p.get("items"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
        .min(MAX_CONFIGURATION_ITEMS);
    Value::Array(vec![Value::Null; items])
}

/// Whether an answer holds something: a non-empty array, or any object.
pub fn is_found(result: &Value) -> bool {
    match result {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        _ => true,
    }
}

/// A `file://` URI for an absolute path, percent-encoding all but the unreserved characters and
/// `/`.
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frames(msgs: &[Value]) -> Vec<u8> {
        let mut out = Vec::new();
        for m in msgs {
            write_frame(&mut out, &serde_json::to_vec(m).unwrap()).unwrap();
        }
        out
    }

    fn sent(bytes: &[u8]) -> Vec<Value> {
        let mut r = Cursor::new(bytes.to_vec());
        let mut out = Vec::new();
        while let Some(body) = read_frame(&mut r).unwrap() {
            out.push(serde_json::from_slice(&body).unwrap());
        }
        out
    }

    #[test]
    fn should_round_trip_a_body_when_framing() {
        // given
        let mut buf = Vec::new();
        // when
        write_frame(&mut buf, b"{\"a\":1}").unwrap();
        let back = read_frame(&mut Cursor::new(buf.clone())).unwrap();
        // then
        assert!(buf.starts_with(b"Content-Length: 7\r\n\r\n"));
        assert_eq!(back.as_deref(), Some(&b"{\"a\":1}"[..]));
        assert_eq!(read_frame(&mut Cursor::new(Vec::new())).unwrap(), None);
    }

    #[test]
    fn should_refuse_before_allocating_when_a_frame_is_too_large_or_malformed() {
        // given
        let huge = format!("Content-Length: {}\r\n\r\n", MAX_BODY_BYTES + 1);
        let long_line = format!("X-{}: 1\r\n\r\n", "a".repeat(2000));
        let many = "X: 1\r\n".repeat(20);
        // when
        // then
        assert!(
            read_frame(&mut Cursor::new(huge))
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
        assert!(
            read_frame(&mut Cursor::new(long_line))
                .unwrap_err()
                .to_string()
                .contains("too long")
        );
        assert!(
            read_frame(&mut Cursor::new(many))
                .unwrap_err()
                .to_string()
                .contains("too many")
        );
        assert!(read_frame(&mut Cursor::new("\r\n")).is_err());
        assert!(read_frame(&mut Cursor::new("Content-Length: 9\r\n\r\n{}")).is_err());
    }

    #[test]
    fn should_answer_server_requests_and_skip_notifications_when_waiting_for_a_response() {
        // given: the server sends a notification, a request of its own, then the response
        let script = frames(&[
            json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"message":"hi"}}),
            json!({"jsonrpc":"2.0","id":"s1","method":"client/registerCapability","params":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":[{"uri":"file:///a/B.java"}]}),
        ]);
        let mut client = Client::new(Cursor::new(script), Vec::new());
        // when
        let (result, ms) = client
            .request("textDocument/definition", json!({}))
            .unwrap();
        // then
        assert!(is_found(&result));
        assert!(ms >= 0.0);
        let out = sent(client.writer());
        assert_eq!(out[0]["method"], "textDocument/definition");
        assert_eq!(out[0]["id"], 1);
        assert_eq!(out[1], json!({"jsonrpc":"2.0","id":"s1","result":null}));
    }

    #[test]
    fn should_answer_one_null_per_item_when_the_server_asks_for_configuration() {
        // given
        let script = frames(&[
            json!({"jsonrpc":"2.0","id":7,"method":"workspace/configuration","params":{"items":[{"section":"java"},{}]}}),
            json!({"jsonrpc":"2.0","id":1,"result":null}),
        ]);
        let mut client = Client::new(Cursor::new(script), Vec::new());
        // when
        client.request("workspace/symbol", json!({})).unwrap();
        // then
        let out = sent(client.writer());
        assert_eq!(out[1], json!({"jsonrpc":"2.0","id":7,"result":[null,null]}));
        let many = json!({"items": vec![json!({}); MAX_CONFIGURATION_ITEMS + 5]});
        assert_eq!(
            answer("workspace/configuration", Some(&many))
                .as_array()
                .map(Vec::len),
            Some(MAX_CONFIGURATION_ITEMS)
        );
        assert_eq!(answer("client/registerCapability", None), Value::Null);
    }

    #[test]
    fn should_fail_when_the_server_answers_an_error_or_closes_its_output() {
        // given
        let script =
            frames(&[json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}})]);
        let mut client = Client::new(Cursor::new(script), Vec::new());
        // when
        let err = client.request("x/unknown", json!({})).unwrap_err();
        let eof = client.request("x/again", json!({})).unwrap_err();
        // then
        assert!(err.to_string().contains("-32601"), "{err}");
        assert!(eof.to_string().contains("closed its output"), "{eof}");
    }

    #[test]
    fn should_return_the_params_when_the_awaited_notification_arrives() {
        // given
        let script = frames(&[
            json!({"jsonrpc":"2.0","method":"language/status","params":{"type":"Starting"}}),
            json!({"jsonrpc":"2.0","method":"language/status","params":{"type":"ServiceReady"}}),
        ]);
        let mut client = Client::new(Cursor::new(script), Vec::new());
        // when
        let params = client
            .wait_notification(|m, p| m == "language/status" && p["type"] == "ServiceReady")
            .unwrap();
        // then
        assert_eq!(params["type"], "ServiceReady");
    }

    #[test]
    fn should_encode_reserved_characters_when_building_a_file_uri() {
        // given
        // when
        // then
        assert_eq!(
            file_uri(Path::new("/a b/C#.java")),
            "file:///a%20b/C%23.java"
        );
        assert!(!is_found(&json!([])));
        assert!(!is_found(&Value::Null));
        assert!(is_found(&json!({"contents": "x"})));
    }
}
