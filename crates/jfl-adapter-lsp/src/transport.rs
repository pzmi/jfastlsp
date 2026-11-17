//! Bytes in and out of an LSP session.
//!
//! **In:** [`FrameReader`] reads the client's bytes in chunks of [`READ_CHUNK`] and feeds them to
//! `jfl-protocol`'s bounded decoder (ADR 0021): the declared length is checked against the limit
//! before anything is allocated, and `lsp-server`'s own reader, which trusts the length, is never
//! used on input. Each frame becomes one [`Incoming`]: a message, a body that is not one, a frame
//! skipped for its size, or the end of the session.
//!
//! **Out:** [`Outgoing`] is written by [`write_one`], every frame by this crate, never by
//! `lsp_server::Message::write`, which hands each message's whole text to `log::debug!` (so a `log`
//! bridged into `tracing` at DEBUG would log source text, against fnd §5; the root `clippy.toml`
//! denies it and `lsp_server`'s reader and stdio transport, orchestrator, LSP-01 review). A result
//! is serialized from the handler's typed value straight to the frame body
//! ([`crate::Reply::result`]), never carried in `lsp_server::Response`, whose result is a
//! `serde_json::Value` (plan §5.3: about 20 ms against 9 ms for a 10,000-location answer). An
//! error is `jfl-protocol`'s `Response`, its id null when it could not be read.
//!
//! **Parsing** is one typed pass ([`parse`]) with the rules `lsp_server::Message`'s untagged
//! decoding lacks: `jsonrpc` must be `"2.0"`, and a body with an `id` member is a request (or a
//! response), never a notification, whatever the id holds; an id that is not an `i32` or a string
//! (LSP 3.17's `integer | string`) is an `InvalidRequest` with a null id, never applied.

use std::io::{self, Read, Write};

use jfl_protocol::frame::{FrameDecoder, FrameError, Oversized};
use jfl_protocol::rpc::{ErrorCode, ErrorObject, Outcome, Response};
use lsp_server::{Message, Notification, Request, RequestId, ResponseError};
use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// One read from the client: a pipe's capacity on Linux, so a burst of small messages is one
/// system call and a large frame is copied in pieces the decoder can take whole.
pub const READ_CHUNK: usize = 64 * 1024;

/// What the client sent, one frame at a time.
#[derive(Debug)]
pub enum Incoming {
    /// A JSON-RPC message.
    Message(Message),
    /// A frame whose body is not a JSON-RPC message; the framing is intact and the session goes on.
    Malformed(Malformed),
    /// A frame over the limit, skipped unread; the session goes on.
    Oversized(Oversized),
    /// The session ends: the stream ended between frames, or its framing or the read failed.
    End(End),
}

/// A body that is not a message, and the answer it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed {
    /// The request's id, when one could be read.
    pub id: Option<RequestId>,
    /// [`ErrorCode::ParseError`] for a body that is not JSON, [`ErrorCode::InvalidRequest`] for
    /// JSON that is not a message.
    pub code: ErrorCode,
}

/// Why the input ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// End of stream between two frames.
    Eof,
    /// The framing is lost (a malformed header, a stream cut inside a frame): the decoder stays
    /// failed rather than resynchronise on a guess, so the session ends.
    Framing(FrameError),
    /// The read itself failed.
    Read(io::ErrorKind),
}

/// The client's frames, decoded with a limit.
///
/// No input panics it, and the decoder never holds more than the limit
/// (`tests/properties.rs::should_never_panic_or_buffer_past_the_limit_when_fed_arbitrary_bytes`).
#[derive(Debug)]
pub struct FrameReader<R> {
    input: R,
    decoder: FrameDecoder,
    chunk: Box<[u8]>,
    start: usize,
    end: usize,
    ended: bool,
}

impl<R: Read> FrameReader<R> {
    /// A reader of `input` that refuses frames longer than `max_frame_len` bytes.
    pub fn new(input: R, max_frame_len: usize) -> Self {
        Self {
            input,
            decoder: FrameDecoder::new(max_frame_len),
            chunk: vec![0; READ_CHUNK].into_boxed_slice(),
            start: 0,
            end: 0,
            ended: false,
        }
    }

    /// The bytes the decoder holds for the frame it is reading: never more than the limit.
    #[must_use]
    pub const fn buffered_capacity(&self) -> usize {
        self.decoder.buffered_capacity()
    }

    /// The next frame, or `None` once an [`Incoming::End`] has been returned.
    pub fn next_incoming(&mut self) -> Option<Incoming> {
        if self.ended {
            return None;
        }
        let incoming = self.read_frame();
        if matches!(incoming, Incoming::End(_)) {
            self.ended = true;
        }
        Some(incoming)
    }

    fn read_frame(&mut self) -> Incoming {
        loop {
            while self.start < self.end {
                let pending = self.chunk.get(self.start..self.end).unwrap_or_default();
                let progress = match self.decoder.feed(pending) {
                    Ok(progress) => progress,
                    Err(err) => return Incoming::End(End::Framing(err)),
                };
                self.start = self
                    .start
                    .saturating_add(progress.consumed.max(1))
                    .min(self.end);
                if let Some(oversized) = progress.oversized {
                    return Incoming::Oversized(oversized);
                }
                if let Some(body) = progress.frame {
                    return parse(&body);
                }
            }
            match self.input.read(&mut self.chunk) {
                Ok(0) => {
                    return Incoming::End(match self.decoder.finish() {
                        Ok(()) => End::Eof,
                        Err(err) => End::Framing(err),
                    });
                }
                Ok(read) => {
                    self.start = 0;
                    self.end = read.min(self.chunk.len());
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Incoming::End(End::Read(err.kind())),
            }
        }
    }
}

/// A message's members, as sent: `Some(Value::Null)` for an `id` or `result` member that is
/// `null`, `None` for one that is absent.
#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default, deserialize_with = "present")]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default, deserialize_with = "present")]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ResponseError>,
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// An id LSP allows: an `i32` or a string.
fn request_id(id: Value) -> Option<RequestId> {
    serde_json::from_value(id).ok()
}

/// A frame's body as a message, or why it is not one.
///
/// Pinned by: `transport::tests::should_classify_a_body_when_it_is_not_a_message` and
/// `fixtures/lsp/malformed-body.jsonl`.
#[must_use]
pub fn parse(body: &[u8]) -> Incoming {
    let invalid = |id: Option<RequestId>| {
        Incoming::Malformed(Malformed {
            id,
            code: ErrorCode::InvalidRequest,
        })
    };
    let raw = match serde_json::from_slice::<Raw>(body) {
        Ok(raw) => raw,
        Err(_) if serde_json::from_slice::<IgnoredAny>(body).is_err() => {
            return Incoming::Malformed(Malformed {
                id: None,
                code: ErrorCode::ParseError,
            });
        }
        Err(_) => {
            #[derive(Deserialize)]
            struct Probe {
                id: RequestId,
            }
            return invalid(serde_json::from_slice::<Probe>(body).ok().map(|p| p.id));
        }
    };
    let id = raw.id.map(request_id);
    if raw.jsonrpc.as_deref() != Some("2.0") {
        return invalid(id.flatten());
    }
    match (raw.method, id, raw.result, raw.error) {
        (Some(method), None, None, None) => {
            Incoming::Message(Message::Notification(Notification {
                method,
                params: raw.params,
            }))
        }
        (Some(method), Some(Some(id)), None, None) => {
            Incoming::Message(Message::Request(Request {
                id,
                method,
                params: raw.params,
            }))
        }
        (None, Some(Some(id)), Some(result), None) => {
            Incoming::Message(Message::Response(lsp_server::Response {
                id,
                response_result: Ok(result),
            }))
        }
        (None, Some(Some(id)), None, Some(error)) => {
            Incoming::Message(Message::Response(lsp_server::Response {
                id,
                response_result: Err(error),
            }))
        }
        (_, id, _, _) => invalid(id.flatten()),
    }
}

/// What the server sends.
#[derive(Debug)]
pub enum Outgoing {
    /// A result: `body` is the result's JSON, serialized from its typed value.
    Result {
        /// The request's id.
        id: RequestId,
        /// The result's JSON.
        body: Vec<u8>,
    },
    /// An error answer.
    Error {
        /// The request's id; `None` when it could not be read.
        id: Option<RequestId>,
        /// The error.
        error: ErrorObject,
    },
}

/// `{"jsonrpc":"2.0","id":` … `,"result":` … `}` around a typed result.
const RESULT_HEAD: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":";
const RESULT_MID: &[u8] = b",\"result\":";
const RESULT_TAIL: &[u8] = b"}";

/// Writes one message as a frame; the caller flushes.
///
/// Pinned by: `transport::tests::should_frame_a_typed_result_when_it_is_written` and
/// `transport::tests::should_write_a_null_id_when_an_error_has_no_id`.
pub fn write_one(out: &mut impl Write, outgoing: Outgoing) -> io::Result<()> {
    match outgoing {
        Outgoing::Result { id, body } => {
            let id = serde_json::to_vec(&id).map_err(io::Error::other)?;
            let len = [RESULT_HEAD.len(), id.len(), RESULT_MID.len(), body.len(), 1]
                .iter()
                .try_fold(0_usize, |sum, part| sum.checked_add(*part))
                .ok_or_else(|| io::Error::other("result too large to frame"))?;
            write!(out, "Content-Length: {len}\r\n\r\n")?;
            out.write_all(RESULT_HEAD)?;
            out.write_all(&id)?;
            out.write_all(RESULT_MID)?;
            out.write_all(&body)?;
            out.write_all(RESULT_TAIL)
        }
        Outgoing::Error { id, error } => {
            let response = Response::<()> {
                id: id.and_then(protocol_id),
                outcome: Outcome::Error(error),
            };
            let body = serde_json::to_vec(&response).map_err(io::Error::other)?;
            write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
            out.write_all(&body)
        }
    }
}

/// `lsp-server`'s id as `jfl-protocol`'s: both are an `i32` or a string, written the same way.
fn protocol_id(id: RequestId) -> Option<jfl_protocol::RequestId> {
    serde_json::to_value(id)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(bytes: &[u8]) -> Vec<serde_json::Value> {
        let mut reader = FrameReader::new(bytes, 1024);
        let mut out = Vec::new();
        while let Some(Incoming::Message(message)) = reader.next_incoming() {
            out.push(serde_json::to_value(&message).unwrap());
        }
        out
    }

    fn bodies(bytes: &[u8]) -> Vec<serde_json::Value> {
        let mut decoder = FrameDecoder::new(1 << 20);
        let mut rest = bytes;
        let mut out = Vec::new();
        while !rest.is_empty() {
            let progress = decoder.feed(rest).unwrap();
            rest = &rest[progress.consumed..];
            if let Some(body) = progress.frame {
                out.push(serde_json::from_slice(&body).unwrap());
            }
        }
        out
    }

    #[test]
    fn should_classify_a_body_when_it_is_not_a_message() {
        // given
        let invalid = ErrorCode::InvalidRequest;
        let seven = || Some(RequestId::from(7));
        let cases: Vec<(&[u8], Option<RequestId>, ErrorCode)> = vec![
            (b"{not json", None, ErrorCode::ParseError),
            (b"[1,2]", None, invalid),
            (br#"{"jsonrpc":"2.0","id":7}"#, seven(), invalid),
            (
                br#"{"jsonrpc":"2.0","id":"x","method":3}"#,
                Some(RequestId::from("x".to_owned())),
                invalid,
            ),
            // An id LSP does not allow: never a notification, never applied.
            (
                br#"{"jsonrpc":"2.0","id":4294967296,"method":"m"}"#,
                None,
                invalid,
            ),
            (br#"{"jsonrpc":"2.0","id":1.5,"method":"m"}"#, None, invalid),
            (
                br#"{"jsonrpc":"2.0","id":true,"method":"m"}"#,
                None,
                invalid,
            ),
            (
                br#"{"jsonrpc":"2.0","id":null,"method":"m"}"#,
                None,
                invalid,
            ),
            // Not JSON-RPC 2.0.
            (
                br#"{"jsonrpc":"1.0","id":7,"method":"m"}"#,
                seven(),
                invalid,
            ),
            (br#"{"id":7,"method":"m"}"#, seven(), invalid),
            (br#"{"jsonrpc":2,"id":7,"method":"m"}"#, seven(), invalid),
            (
                br#"{"jsonrpc":"2.0","method":"m"}x"#,
                None,
                ErrorCode::ParseError,
            ),
            // Both a result and a method, or neither.
            (
                br#"{"jsonrpc":"2.0","id":7,"method":"m","result":1}"#,
                seven(),
                invalid,
            ),
            (
                br#"{"jsonrpc":"2.0","id":7,"result":1,"error":{"code":1,"message":""}}"#,
                seven(),
                invalid,
            ),
        ];

        for (body, id, code) in cases {
            // when
            let incoming = parse(body);

            // then
            match incoming {
                Incoming::Malformed(malformed) => {
                    assert_eq!(malformed, Malformed { id, code });
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn should_read_each_kind_when_a_body_is_a_message() {
        // given
        let bodies: [&[u8]; 4] = [
            br#"{"jsonrpc":"2.0","id":-1,"method":"m","params":{"a":1}}"#,
            br#"{"jsonrpc":"2.0","method":"n"}"#,
            br#"{"jsonrpc":"2.0","id":"r","result":null}"#,
            br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"no"}}"#,
        ];

        // when
        let got: Vec<Incoming> = bodies.iter().map(|b| parse(b)).collect();

        // then
        match &got[..] {
            [
                Incoming::Message(Message::Request(request)),
                Incoming::Message(Message::Notification(notification)),
                Incoming::Message(Message::Response(ok)),
                Incoming::Message(Message::Response(err)),
            ] => {
                assert_eq!(request.id, RequestId::from(-1));
                assert_eq!(request.params, serde_json::json!({"a": 1}));
                assert_eq!(notification.method, "n");
                assert_eq!(notification.params, Value::Null);
                assert!(matches!(&ok.response_result, Ok(Value::Null)));
                assert!(matches!(&err.response_result, Err(e) if e.code == -32601));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn should_read_ids_of_both_types_when_requests_arrive() {
        // given
        let input = b"Content-Length: 37\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"a\"}\
Content-Length: 39\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"b\"}";

        // when
        let got = frames(input);

        // then
        assert_eq!(got.len(), 2);
        assert_eq!(got[0]["id"], 1);
        assert_eq!(got[1]["id"], "x");
    }

    #[test]
    fn should_report_the_end_when_the_stream_stops_inside_a_frame() {
        // given
        let input: &[u8] = b"Content-Length: 10\r\n\r\n{}";
        let mut reader = FrameReader::new(input, 1024);

        // when
        let first = reader.next_incoming();
        let after = reader.next_incoming();

        // then
        assert!(
            matches!(
                first,
                Some(Incoming::End(End::Framing(FrameError::Truncated)))
            ),
            "{first:?}"
        );
        assert!(after.is_none());
    }

    #[test]
    fn should_skip_an_oversized_frame_when_it_is_over_the_limit() {
        // given
        let mut input = b"Content-Length: 2000\r\n\r\n".to_vec();
        input.extend(std::iter::repeat_n(b'x', 2000));
        input.extend(b"Content-Length: 2\r\n\r\n{}");
        let mut reader = FrameReader::new(input.as_slice(), 1024);

        // when
        let first = reader.next_incoming();
        let second = reader.next_incoming();
        let third = reader.next_incoming();

        // then
        assert!(
            matches!(
                first,
                Some(Incoming::Oversized(Oversized {
                    declared: 2000,
                    limit: 1024
                }))
            ),
            "{first:?}"
        );
        assert!(matches!(second, Some(Incoming::Malformed(_))), "{second:?}");
        assert!(matches!(third, Some(Incoming::End(End::Eof))), "{third:?}");
    }

    #[test]
    fn should_frame_a_typed_result_when_it_is_written() {
        // given
        let outgoing = Outgoing::Result {
            id: RequestId::from("a\"b".to_owned()),
            body: b"[1,{\"k\":null}]".to_vec(),
        };
        let mut out = Vec::new();

        // when
        write_one(&mut out, outgoing).unwrap();

        // then
        assert_eq!(
            bodies(&out),
            [serde_json::json!({"jsonrpc": "2.0", "id": "a\"b", "result": [1, {"k": null}]})]
        );
    }

    #[test]
    fn should_write_a_null_id_when_an_error_has_no_id() {
        // given
        let error = ErrorObject {
            code: ErrorCode::ParseError,
            message: "not JSON".to_owned(),
            data: None,
        };
        let mut out = Vec::new();

        // when
        write_one(
            &mut out,
            Outgoing::Error {
                id: None,
                error: error.clone(),
            },
        )
        .unwrap();
        write_one(
            &mut out,
            Outgoing::Error {
                id: Some(RequestId::from(4)),
                error,
            },
        )
        .unwrap();

        // then
        assert_eq!(
            bodies(&out),
            [
                serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "not JSON"}}),
                serde_json::json!({"jsonrpc": "2.0", "id": 4, "error": {"code": -32700, "message": "not JSON"}}),
            ]
        );
    }
}
