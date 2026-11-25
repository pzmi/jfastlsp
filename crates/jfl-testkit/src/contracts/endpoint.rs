//! The `endpoint` contract (ver §5): what any carrier of the daemon's frames must do between a
//! client (the proxy) and a server (the daemon), checked the same way against [`DuplexEndpoint`],
//! an in-memory duplex, here and against `jfl-adapter-ipc`'s Unix socket (its listener and its
//! client) in `crates/jfl-adapter-ipc/tests/contract.rs`.
//!
//! `jfl-testkit` may not depend on `jfl-adapter-ipc` (plan §6.3), so the suite speaks its own
//! small vocabulary: a [`Connection`] is one accepted connection, its client half and its server
//! half driven by the suite one call at a time, and the server reports each frame as a [`Read`]
//! (the adapter's `Incoming`, its params kept as JSON). Frames are LSP's `Content-Length` framing
//! with `jfl-protocol`'s limit ([`DEFAULT_MAX_FRAME_LEN`]).
//!
//! The cases pin ordering, whole frames of any size in both directions, the refusals (a body
//! that is not a message, a frame over the limit, a client body over the limit, framing lost)
//! with the framing intact after each that allows it, the ends (a clean end, a truncated frame,
//! the server's close as the client's end, an end that stays the same end), and a client read
//! whose deadline passed losing nothing. The suite asks a server for a frame only when one was
//! sent or the client closed, so an implementation never has to answer "nothing yet".
//!
//! The suite's own tests run it against deliberately broken carriers and assert each fails the
//! case it should (`endpoint::tests`).

use std::collections::VecDeque;
use std::time::Duration;

use jfl_protocol::frame::encode_frame;
use jfl_protocol::rpc::{ErrorCode, RequestId};
use jfl_protocol::{DEFAULT_MAX_FRAME_LEN, FrameDecoder};
use serde_json::{Value, json};

/// What the server read: one frame, or the end of its input.
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    /// A request.
    Call {
        /// Its id.
        id: RequestId,
        /// Its method.
        method: String,
        /// Its params.
        params: Value,
    },
    /// A notification.
    Note {
        /// Its method.
        method: String,
        /// Its params.
        params: Value,
    },
    /// A body that is neither, with its id when one could be read; the framing is intact.
    Malformed {
        /// The id.
        id: Option<RequestId>,
        /// `ParseError` for a body that is not JSON, `InvalidRequest` for JSON that is not a
        /// request or a notification.
        code: ErrorCode,
    },
    /// A frame over the limit, skipped; the framing is intact.
    Oversized {
        /// Its declared length.
        declared: u64,
    },
    /// The input ended: `clean` at the end of the stream between two frames; not clean when the
    /// framing was lost, the stream ended inside a frame, or the read failed.
    End {
        /// Whether it ended between two frames.
        clean: bool,
    },
    /// Nothing will come: the carrier was asked with nothing sent and the client open (a fake
    /// says so where a real carrier would block; the suite never asks then).
    Stalled,
}

/// What the client's read got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Got {
    /// One frame's body.
    Frame(Vec<u8>),
    /// The server closed the connection between two frames.
    Eof,
    /// Nothing within the wait.
    TimedOut,
    /// Anything else, described.
    Failed(String),
}

/// Why the client's send was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The body is over the limit: nothing was written.
    TooLarge,
    /// Anything else, described.
    Failed(String),
}

/// One accepted connection, both halves.
pub trait Connection {
    /// The client sends one frame of `body`, written whole before it returns (the server reads
    /// concurrently where the carrier needs it to).
    fn client_send(&mut self, body: &[u8]) -> Result<(), Refused>;
    /// The client writes `bytes` as they are, framing or not (a hostile or broken peer).
    fn client_write_raw(&mut self, bytes: &[u8]);
    /// The client reads the next frame, waiting at most `wait`.
    fn client_recv(&mut self, wait: Duration) -> Got;
    /// The client closes its half.
    fn client_close(&mut self);
    /// The server reads the next frame (one was sent, or the client closed).
    fn server_next(&mut self) -> Read;
    /// The server sends one frame of `body`; the client reads it later.
    fn server_send(&mut self, body: &[u8]);
    /// The server closes the connection.
    fn server_close(&mut self);
}

/// A carrier under test: it opens connections.
pub trait EndpointUnderTest {
    /// Opens one connection: a client connected and the server's accept of it, done before this
    /// returns. A case may close its client at once, and a server may skip a peer that is gone
    /// by its accept (macOS's listener does: `jfl-adapter-ipc`'s `server` module), so a carrier
    /// that returned before its accept would lose the case's frames (FND-19).
    fn open(&mut self) -> Result<Box<dyn Connection>, String>;
}

/// A case the carrier failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

/// One case of the suite: `Err` says what the carrier got wrong.
type Case = fn(&mut dyn Connection) -> Result<(), String>;

/// A wait the carrier must finish well within: a frame already sent, or the end of a closed
/// connection.
const PLENTY: Duration = Duration::from_secs(20);

/// Every case, by name, in the order [`check`] runs them.
const CASES: [(&str, Case); 10] = [
    ("frames_in_order", frames_in_order),
    ("large_frames_whole", large_frames_whole),
    ("malformed_keeps_framing", malformed_keeps_framing),
    ("oversized_skipped", oversized_skipped),
    ("client_refuses_oversized", client_refuses_oversized),
    ("clean_end_is_final", clean_end_is_final),
    ("server_close_is_eof", server_close_is_eof),
    ("timed_out_read_loses_nothing", timed_out_read_loses_nothing),
    ("truncated_frame_is_not_clean", truncated_frame_is_not_clean),
    ("lost_framing_ends", lost_framing_ends),
];

/// The names of every case, for a carrier that runs each as a test of its own (so a case that
/// hangs is named by the test that hangs, FND-19).
#[must_use]
pub fn case_names() -> Vec<&'static str> {
    CASES.iter().map(|(case, _)| *case).collect()
}

/// Runs every case, each on a connection of its own, and returns the violations; an empty list
/// is a pass.
pub fn check(endpoint: &mut impl EndpointUnderTest) -> Vec<Violation> {
    CASES
        .iter()
        .filter_map(|(case, run)| run_case(endpoint, case, *run).err())
        .collect()
}

/// Runs the case named `case` on a connection of its own. A name that is no case is a violation
/// of its own, so a carrier's list of tests cannot drift from [`case_names`] silently.
pub fn check_case(endpoint: &mut impl EndpointUnderTest, case: &str) -> Result<(), Violation> {
    match CASES.iter().find(|(name, _)| *name == case) {
        Some((name, run)) => run_case(endpoint, name, *run),
        None => Err(Violation {
            case: "unknown",
            detail: format!("no case is named {case:?}"),
        }),
    }
}

fn run_case(
    endpoint: &mut impl EndpointUnderTest,
    case: &'static str,
    run: Case,
) -> Result<(), Violation> {
    endpoint
        .open()
        .map_err(|e| format!("could not open a connection: {e}"))
        .and_then(|mut conn| run(conn.as_mut()))
        .map_err(|detail| Violation { case, detail })
}

fn request(id: &RequestId, method: &str, params: &Value) -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        .to_string()
        .into_bytes()
}

fn notification(method: &str, params: &Value) -> Vec<u8> {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
        .to_string()
        .into_bytes()
}

fn send(conn: &mut dyn Connection, body: &[u8]) -> Result<(), String> {
    conn.client_send(body)
        .map_err(|e| format!("the client's send failed: {e:?}"))
}

fn expect_read(conn: &mut dyn Connection, want: &Read) -> Result<(), String> {
    let got = conn.server_next();
    if got == *want {
        Ok(())
    } else {
        Err(format!(
            "the server read {}, want {}",
            brief(&got),
            brief(want)
        ))
    }
}

fn expect_got(conn: &mut dyn Connection, wait: Duration, want: &Got) -> Result<(), String> {
    let got = conn.client_recv(wait);
    if got == *want {
        Ok(())
    } else {
        Err(format!(
            "the client got {}, want {}",
            brief_got(&got),
            brief_got(want)
        ))
    }
}

/// A read described without a large body.
fn brief(read: &Read) -> String {
    let text = format!("{read:?}");
    match text.char_indices().nth(200) {
        Some((at, _)) => format!(
            "{}… ({} bytes)",
            text.get(..at).unwrap_or_default(),
            text.len()
        ),
        None => text,
    }
}

fn brief_got(got: &Got) -> String {
    match got {
        Got::Frame(body) => format!(
            "a frame of {} bytes starting {:?}",
            body.len(),
            String::from_utf8_lossy(body.get(..body.len().min(80)).unwrap_or_default())
        ),
        other => format!("{other:?}"),
    }
}

fn frames_in_order(conn: &mut dyn Connection) -> Result<(), String> {
    let first = RequestId::Number(1);
    let third = RequestId::String("c/3".to_owned());
    send(conn, &request(&first, "jfl/a", &json!({"n": 1})))?;
    send(conn, &notification("jfl/b", &json!({"n": 2})))?;
    send(conn, &request(&third, "jfl/c", &json!({"n": 3})))?;
    expect_read(
        conn,
        &Read::Call {
            id: first,
            method: "jfl/a".to_owned(),
            params: json!({"n": 1}),
        },
    )?;
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/b".to_owned(),
            params: json!({"n": 2}),
        },
    )?;
    expect_read(
        conn,
        &Read::Call {
            id: third,
            method: "jfl/c".to_owned(),
            params: json!({"n": 3}),
        },
    )?;
    let answers = [
        br#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":"c/3","result":null}"#.to_vec(),
    ];
    for answer in &answers {
        conn.server_send(answer);
    }
    for answer in answers {
        expect_got(conn, PLENTY, &Got::Frame(answer))?;
    }
    Ok(())
}

fn large_frames_whole(conn: &mut dyn Connection) -> Result<(), String> {
    // Larger than any socket buffer and any one read, and of an odd length.
    let text: String = (0..(1 << 20) + 17)
        .map(|i| char::from(b'a'.saturating_add(u8::try_from(i % 26).unwrap_or(0))))
        .collect();
    let params = json!({"text": text});
    send(conn, &notification("jfl/big", &params))?;
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/big".to_owned(),
            params,
        },
    )?;
    let answer = format!(r#"{{"jsonrpc":"2.0","id":2,"result":"{text}"}}"#).into_bytes();
    conn.server_send(&answer);
    expect_got(conn, PLENTY, &Got::Frame(answer))
}

fn malformed_keeps_framing(conn: &mut dyn Connection) -> Result<(), String> {
    send(conn, b"not json")?;
    send(conn, br#"{"jsonrpc":"2.0","id":9,"method":7}"#)?;
    send(conn, &notification("jfl/after", &json!({})))?;
    expect_read(
        conn,
        &Read::Malformed {
            id: None,
            code: ErrorCode::ParseError,
        },
    )?;
    expect_read(
        conn,
        &Read::Malformed {
            id: Some(RequestId::Number(9)),
            code: ErrorCode::InvalidRequest,
        },
    )?;
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/after".to_owned(),
            params: json!({}),
        },
    )
}

fn oversized_skipped(conn: &mut dyn Connection) -> Result<(), String> {
    let len = DEFAULT_MAX_FRAME_LEN.saturating_add(1);
    conn.client_write_raw(format!("Content-Length: {len}\r\n\r\n").as_bytes());
    conn.client_write_raw(&vec![b'x'; len]);
    send(conn, &notification("jfl/after", &json!({})))?;
    expect_read(
        conn,
        &Read::Oversized {
            declared: u64::try_from(len).unwrap_or(u64::MAX),
        },
    )?;
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/after".to_owned(),
            params: json!({}),
        },
    )
}

fn client_refuses_oversized(conn: &mut dyn Connection) -> Result<(), String> {
    let refused = conn.client_send(&vec![b' '; DEFAULT_MAX_FRAME_LEN.saturating_add(1)]);
    if refused != Err(Refused::TooLarge) {
        return Err(format!(
            "a body over the limit was not refused as too large: {refused:?}"
        ));
    }
    send(conn, &notification("jfl/after", &json!({})))?;
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/after".to_owned(),
            params: json!({}),
        },
    )
}

fn clean_end_is_final(conn: &mut dyn Connection) -> Result<(), String> {
    send(conn, &notification("jfl/last", &json!({})))?;
    conn.client_close();
    expect_read(
        conn,
        &Read::Note {
            method: "jfl/last".to_owned(),
            params: json!({}),
        },
    )?;
    expect_read(conn, &Read::End { clean: true })?;
    expect_read(conn, &Read::End { clean: true })
}

fn server_close_is_eof(conn: &mut dyn Connection) -> Result<(), String> {
    let answer = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_vec();
    conn.server_send(&answer);
    conn.server_close();
    expect_got(conn, PLENTY, &Got::Frame(answer))?;
    expect_got(conn, PLENTY, &Got::Eof)
}

fn timed_out_read_loses_nothing(conn: &mut dyn Connection) -> Result<(), String> {
    expect_got(conn, Duration::from_millis(50), &Got::TimedOut)?;
    let answer = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_vec();
    conn.server_send(&answer);
    expect_got(conn, PLENTY, &Got::Frame(answer))
}

fn truncated_frame_is_not_clean(conn: &mut dyn Connection) -> Result<(), String> {
    conn.client_write_raw(b"Content-Length: 10\r\n\r\nabc");
    conn.client_close();
    expect_read(conn, &Read::End { clean: false })?;
    // The same end again: never a clean one after a truncated frame.
    expect_read(conn, &Read::End { clean: false })
}

fn lost_framing_ends(conn: &mut dyn Connection) -> Result<(), String> {
    conn.client_write_raw(b"Content-Lenght: 2\r\n\r\n{}");
    send(conn, &notification("jfl/after", &json!({})))?;
    // Never resynchronised onto the frame that follows, on a guess.
    expect_read(conn, &Read::End { clean: false })
}

/// The in-memory duplex: bytes queued each way and read through `jfl-protocol`'s decoder, with
/// the rules a real carrier must keep. It cannot be told to fail: it exists so the suite itself
/// can be checked against a known-good carrier and the broken ones of its tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct DuplexEndpoint;

impl EndpointUnderTest for DuplexEndpoint {
    fn open(&mut self) -> Result<Box<dyn Connection>, String> {
        Ok(Box::new(Duplex::default()))
    }
}

/// One in-memory connection.
#[derive(Debug)]
pub struct Duplex {
    to_server: VecDeque<u8>,
    to_client: VecDeque<u8>,
    server_in: FrameDecoder,
    client_in: FrameDecoder,
    client_closed: bool,
    server_closed: bool,
    server_end: Option<Read>,
}

impl Default for Duplex {
    fn default() -> Self {
        Self {
            to_server: VecDeque::new(),
            to_client: VecDeque::new(),
            server_in: FrameDecoder::new(DEFAULT_MAX_FRAME_LEN),
            client_in: FrameDecoder::new(DEFAULT_MAX_FRAME_LEN),
            client_closed: false,
            server_closed: false,
            server_end: None,
        }
    }
}

/// One step of a decoder over a queue: a frame, an oversized frame, an error, or nothing more.
enum Step {
    Frame(Vec<u8>),
    Oversized(u64),
    Lost,
    Empty,
}

fn step(queue: &mut VecDeque<u8>, decoder: &mut FrameDecoder) -> Step {
    while !queue.is_empty() {
        let (pending, _) = queue.as_slices();
        let progress = match decoder.feed(pending) {
            Ok(progress) => progress,
            Err(_) => return Step::Lost,
        };
        let taken = progress.consumed.max(1).min(queue.len());
        queue.drain(..taken);
        if let Some(oversized) = progress.oversized {
            return Step::Oversized(oversized.declared);
        }
        if let Some(body) = progress.frame {
            return Step::Frame(body);
        }
    }
    Step::Empty
}

/// Classifies a body as the adapter does (a request, a notification, or neither, with its id
/// when one can be read).
pub fn classify(body: &[u8]) -> Read {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Read::Malformed {
            id: None,
            code: ErrorCode::ParseError,
        };
    };
    let id = value
        .get("id")
        .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok());
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match (value.get("method").and_then(Value::as_str), value.get("id")) {
        (Some(method), Some(_)) => match id {
            Some(id) => Read::Call {
                id,
                method: method.to_owned(),
                params,
            },
            None => Read::Malformed {
                id: None,
                code: ErrorCode::InvalidRequest,
            },
        },
        (Some(method), None) => Read::Note {
            method: method.to_owned(),
            params,
        },
        (None, _) => Read::Malformed {
            id,
            code: ErrorCode::InvalidRequest,
        },
    }
}

impl Connection for Duplex {
    fn client_send(&mut self, body: &[u8]) -> Result<(), Refused> {
        let frame = encode_frame(body, DEFAULT_MAX_FRAME_LEN).map_err(|_| Refused::TooLarge)?;
        self.to_server.extend(frame);
        Ok(())
    }

    fn client_write_raw(&mut self, bytes: &[u8]) {
        self.to_server.extend(bytes);
    }

    fn client_recv(&mut self, _wait: Duration) -> Got {
        match step(&mut self.to_client, &mut self.client_in) {
            Step::Frame(body) => Got::Frame(body),
            Step::Oversized(declared) => Got::Failed(format!("a frame of {declared} bytes")),
            Step::Lost => Got::Failed("the framing is lost".to_owned()),
            // Nothing can arrive while the suite waits: it is the only writer.
            Step::Empty if !self.server_closed => Got::TimedOut,
            Step::Empty => match self.client_in.finish() {
                Ok(()) => Got::Eof,
                Err(e) => Got::Failed(e.to_string()),
            },
        }
    }

    fn client_close(&mut self) {
        self.client_closed = true;
    }

    fn server_next(&mut self) -> Read {
        if let Some(end) = &self.server_end {
            return end.clone();
        }
        let read = match step(&mut self.to_server, &mut self.server_in) {
            Step::Frame(body) => return classify(&body),
            Step::Oversized(declared) => return Read::Oversized { declared },
            Step::Lost => Read::End { clean: false },
            Step::Empty if !self.client_closed => return Read::Stalled,
            Step::Empty => Read::End {
                clean: self.server_in.finish().is_ok(),
            },
        };
        self.server_end = Some(read.clone());
        read
    }

    fn server_send(&mut self, body: &[u8]) {
        if let Ok(frame) = encode_frame(body, DEFAULT_MAX_FRAME_LEN) {
            self.to_client.extend(frame);
        }
    }

    fn server_close(&mut self) {
        self.server_closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broken carrier: the duplex with one rule changed.
    struct Broken {
        conn: Duplex,
        fault: Fault,
        held: Option<Read>,
        timed_out: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        /// The server reads the second of two queued frames first.
        Reorders,
        /// Bodies over 64 KiB reach the client cut short.
        Truncates,
        /// A body that is not a message ends the input.
        EndsOnMalformed,
        /// A frame over the limit ends the input instead of being skipped.
        EndsOnOversized,
        /// A body over the limit has its header written before the refusal.
        WritesPartOfOversized,
        /// After any end, a clean end (the bug `ConnReader::next` had: its doc promised the same
        /// end).
        ForgetsTheEnd,
        /// After a clean end, nothing more (a reader that stops answering once it ended).
        StallsAfterTheEnd,
        /// The server's close does not end the client's stream.
        NoEofOnClose,
        /// A client read that timed out drops the next frame.
        LosesAfterTimeout,
        /// A stream that ends inside a frame reads as a clean end.
        CleanOnTruncation,
        /// Lost framing skips to the next `Content-Length` and goes on.
        Resynchronises,
    }

    struct BrokenEndpoint(Fault);

    impl EndpointUnderTest for BrokenEndpoint {
        fn open(&mut self) -> Result<Box<dyn Connection>, String> {
            Ok(Box::new(Broken {
                conn: Duplex::default(),
                fault: self.0,
                held: None,
                timed_out: false,
            }))
        }
    }

    impl Connection for Broken {
        fn client_send(&mut self, body: &[u8]) -> Result<(), Refused> {
            if self.fault == Fault::WritesPartOfOversized && body.len() > DEFAULT_MAX_FRAME_LEN {
                self.conn
                    .client_write_raw(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            }
            self.conn.client_send(body)
        }

        fn client_write_raw(&mut self, bytes: &[u8]) {
            if self.fault == Fault::Resynchronises && bytes.starts_with(b"Content-Lenght") {
                // Drops the broken frame, as if it had searched on for the next header.
                return;
            }
            self.conn.client_write_raw(bytes);
        }

        fn client_recv(&mut self, wait: Duration) -> Got {
            let got = self.conn.client_recv(wait);
            match (self.fault, got) {
                (Fault::Truncates, Got::Frame(mut body)) if body.len() > 64 * 1024 => {
                    body.truncate(64 * 1024);
                    Got::Frame(body)
                }
                (Fault::NoEofOnClose, Got::Eof) => Got::TimedOut,
                (Fault::LosesAfterTimeout, Got::TimedOut) => {
                    self.timed_out = true;
                    Got::TimedOut
                }
                (Fault::LosesAfterTimeout, Got::Frame(_)) if self.timed_out => {
                    self.timed_out = false;
                    self.conn.client_recv(wait)
                }
                (_, got) => got,
            }
        }

        fn client_close(&mut self) {
            self.conn.client_close();
        }

        fn server_next(&mut self) -> Read {
            if let Some(held) = self.held.take() {
                return held;
            }
            let read = self.conn.server_next();
            match (self.fault, read) {
                (Fault::Reorders, first @ (Read::Call { .. } | Read::Note { .. })) => {
                    let second = self.conn.server_next();
                    if matches!(second, Read::Stalled | Read::End { .. }) {
                        self.held = Some(second);
                        first
                    } else {
                        self.held = Some(first);
                        second
                    }
                }
                (Fault::EndsOnMalformed, Read::Malformed { .. })
                | (Fault::EndsOnOversized, Read::Oversized { .. }) => {
                    let end = Read::End { clean: false };
                    self.conn.server_end = Some(end.clone());
                    end
                }
                (Fault::StallsAfterTheEnd, end @ Read::End { clean: true }) => {
                    self.conn.server_end = Some(Read::Stalled);
                    end
                }
                (Fault::ForgetsTheEnd, end @ Read::End { .. }) => {
                    self.conn.server_end = Some(Read::End { clean: true });
                    end
                }
                (Fault::CleanOnTruncation, Read::End { .. }) if self.conn.client_closed => {
                    Read::End { clean: true }
                }
                (_, read) => read,
            }
        }

        fn server_send(&mut self, body: &[u8]) {
            self.conn.server_send(body);
        }

        fn server_close(&mut self) {
            self.conn.server_close();
        }
    }

    fn failed_cases(fault: Fault) -> Vec<&'static str> {
        check(&mut BrokenEndpoint(fault))
            .into_iter()
            .map(|v| v.case)
            .collect()
    }

    #[test]
    fn should_pass_every_case_when_the_carrier_is_the_in_memory_duplex() {
        // given
        let mut endpoint = DuplexEndpoint;

        // when
        let violations = check(&mut endpoint);

        // then
        assert_eq!(violations, []);
    }

    #[test]
    fn should_pass_each_case_by_name_when_the_carrier_is_the_in_memory_duplex() {
        // given
        let names = case_names();

        // when
        let outcomes: Vec<Result<(), Violation>> = names
            .iter()
            .map(|case| check_case(&mut DuplexEndpoint, case))
            .collect();

        // then
        assert_eq!(names.len(), 10);
        assert!(outcomes.iter().all(Result::is_ok), "{outcomes:?}");
    }

    #[test]
    fn should_name_the_case_it_failed_when_run_by_name() {
        // given
        let fault = Fault::NoEofOnClose;

        // when
        let failed = check_case(&mut BrokenEndpoint(fault), "server_close_is_eof");
        let other = check_case(&mut BrokenEndpoint(fault), "frames_in_order");

        // then
        assert_eq!(failed.map_err(|v| v.case), Err("server_close_is_eof"));
        assert_eq!(other, Ok(()));
    }

    #[test]
    fn should_report_a_violation_when_no_case_has_the_name() {
        // given
        let name = "no_such_case";

        // when
        let outcome = check_case(&mut DuplexEndpoint, name);

        // then
        assert_eq!(outcome.map_err(|v| v.case), Err("unknown"));
    }

    #[test]
    fn should_fail_the_order_case_when_the_server_reads_frames_out_of_order() {
        // given
        let fault = Fault::Reorders;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["frames_in_order"]);
    }

    #[test]
    fn should_fail_the_large_frame_case_when_the_carrier_cuts_frames_short() {
        // given
        let fault = Fault::Truncates;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["large_frames_whole"]);
    }

    #[test]
    fn should_fail_the_malformed_case_when_a_body_that_is_not_a_message_ends_the_input() {
        // given
        let fault = Fault::EndsOnMalformed;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["malformed_keeps_framing"]);
    }

    #[test]
    fn should_fail_the_oversized_case_when_a_frame_over_the_limit_ends_the_input() {
        // given
        let fault = Fault::EndsOnOversized;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["oversized_skipped"]);
    }

    #[test]
    fn should_fail_the_client_refusal_case_when_part_of_an_oversized_body_goes_out() {
        // given
        let fault = Fault::WritesPartOfOversized;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["client_refuses_oversized"]);
    }

    #[test]
    fn should_fail_the_truncation_case_when_an_end_is_followed_by_a_clean_one() {
        // given
        let fault = Fault::ForgetsTheEnd;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["truncated_frame_is_not_clean"]);
    }

    #[test]
    fn should_fail_the_clean_end_case_when_a_clean_end_is_followed_by_nothing() {
        // given
        let fault = Fault::StallsAfterTheEnd;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["clean_end_is_final"]);
    }

    #[test]
    fn should_fail_the_close_case_when_the_servers_close_is_not_the_clients_end() {
        // given
        let fault = Fault::NoEofOnClose;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["server_close_is_eof"]);
    }

    #[test]
    fn should_fail_the_timeout_case_when_a_timed_out_read_loses_the_next_frame() {
        // given
        let fault = Fault::LosesAfterTimeout;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["timed_out_read_loses_nothing"]);
    }

    #[test]
    fn should_fail_the_truncation_case_when_a_frame_cut_short_reads_as_a_clean_end() {
        // given
        let fault = Fault::CleanOnTruncation;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["truncated_frame_is_not_clean"]);
    }

    #[test]
    fn should_fail_the_lost_framing_case_when_the_carrier_resynchronises() {
        // given
        let fault = Fault::Resynchronises;

        // when
        let failed = failed_cases(fault);

        // then
        assert_eq!(failed, ["lost_framing_ends"]);
    }
}
