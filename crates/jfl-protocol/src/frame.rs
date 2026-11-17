//! The LSP base protocol's `Content-Length` framing (ADR 0021): a sans-io decoder the IPC and LSP
//! adapters feed bytes to, and the matching encoder.
//!
//! A frame is a header block of `Name: value` lines, each ending in CRLF, an empty line, then
//! exactly `Content-Length` bytes of body. The decoder is strict where leniency could desynchronise
//! the stream or let a peer reserve memory:
//!
//! - **The declared length is checked against the limit before any allocation.** A frame over
//!   the limit is not read: its body is skipped as it arrives, nothing is buffered, and
//!   [`Progress::oversized`] reports it, so the carrier can answer with an error and go on (one
//!   oversized `didOpen` does not end a session; the framing is intact, since the length is
//!   known). Only a length beyond `u64` is fatal ([`FrameError::TooLarge`]). The header is
//!   collected in a fixed buffer inside the decoder ([`MAX_HEADER_LEN`] bytes), and the body buffer
//!   grows with the bytes that actually arrived (at most twice them, so a large frame is not
//!   copied once per read), never past the declared length, so a lying `Content-Length` costs
//!   nothing until its bytes come.
//! - Only `Content-Length` (required) and `Content-Type` (optional, JSON-RPC in UTF-8) are
//!   accepted, each at most once, names in any case; any other field, a line without a colon, a
//!   bare LF, a non-ASCII or control byte, a header block longer than [`MAX_HEADER_LEN`] and a
//!   length that is not plain decimal digits are refused.
//! - After any other refusal the decoder stays failed: the stream's framing is lost, so the
//!   connection is closed rather than resynchronised on a guess.
//!
//! No input panics it (`tests/properties.rs::should_never_panic_or_buffer_past_the_limit_when_fed_arbitrary_bytes`).

use alloc::vec::Vec;
use core::fmt::Write as _;

/// The default largest body accepted: 16 MiB (plan §2.5).
pub const DEFAULT_MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// The largest header block accepted, terminating empty line included. Two short fields need
/// about 80 bytes.
pub const MAX_HEADER_LEN: usize = 1024;

const CONTENT_LENGTH: &str = "content-length";
const CONTENT_TYPE: &str = "content-type";
const MEDIA_TYPE: &str = "application/vscode-jsonrpc";

/// Why a stream is refused. The values carry no bytes of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// A `Content-Length` beyond `u64` (the frame cannot even be skipped), or a body
    /// [`encode_frame`] was asked to send over the limit. A decoded frame over the limit that
    /// fits `u64` is skipped instead ([`Progress::oversized`]).
    #[error("frame of {declared} bytes is larger than the limit of {limit} bytes")]
    TooLarge {
        /// The declared length, saturated at `u64::MAX`.
        declared: u64,
        /// The limit.
        limit: usize,
    },
    /// No empty line within [`MAX_HEADER_LEN`] bytes.
    #[error("header block longer than {MAX_HEADER_LEN} bytes")]
    HeaderTooLong,
    /// A line that is not `Name: value` ending in CRLF, or a byte no header holds.
    #[error("malformed header line")]
    MalformedHeader,
    /// A field other than `Content-Length` and `Content-Type`.
    #[error("unknown header field")]
    UnknownHeader,
    /// A field given twice.
    #[error("duplicate header field")]
    DuplicateHeader,
    /// No `Content-Length` field.
    #[error("no Content-Length header field")]
    MissingContentLength,
    /// A `Content-Length` that is not decimal digits.
    #[error("Content-Length is not a decimal number")]
    InvalidContentLength,
    /// A `Content-Type` other than `application/vscode-jsonrpc` in UTF-8.
    #[error("Content-Type is not application/vscode-jsonrpc in UTF-8")]
    UnsupportedContentType,
    /// The allocator refused the body's bytes.
    #[error("out of memory for a frame of {len} bytes")]
    OutOfMemory {
        /// The declared length.
        len: usize,
    },
    /// The stream ended inside a frame ([`FrameDecoder::finish`]).
    #[error("the stream ended inside a frame")]
    Truncated,
}

/// What one [`FrameDecoder::feed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// Bytes of the input taken; at least one when the input was not empty.
    pub consumed: usize,
    /// The body of the frame those bytes completed, if any.
    pub frame: Option<Vec<u8>>,
    /// A frame over the limit whose header those bytes completed: its body is skipped, never
    /// buffered, and the next frame is decoded as usual.
    pub oversized: Option<Oversized>,
}

/// A frame refused for its size and skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oversized {
    /// Its declared length.
    pub declared: u64,
    /// The limit.
    pub limit: usize,
}

/// What a header declares: a body to read, or one to skip.
enum Length {
    Within(usize),
    Oversized(u64),
}

#[derive(Debug)]
enum State {
    Header,
    Body { len: usize, body: Vec<u8> },
    Skip { remaining: u64 },
    Failed(FrameError),
}

/// A sans-io `Content-Length` decoder: bytes in, frame bodies out.
///
/// Pinned by: `frame::tests` (each refusal, the oversized skip) and `tests/properties.rs`
/// (`should_skip_an_oversized_frame_without_buffering_when_valid_frames_surround_it`, arbitrary bytes and
/// chunkings, the lying length, encode then decode).
#[derive(Debug)]
pub struct FrameDecoder {
    limit: usize,
    header: [u8; MAX_HEADER_LEN],
    header_len: usize,
    state: State,
}

impl FrameDecoder {
    /// A decoder that refuses bodies longer than `limit` bytes.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            header: [0; MAX_HEADER_LEN],
            header_len: 0,
            state: State::Header,
        }
    }

    /// The bytes reserved for the body being read: at most twice what has arrived, and never
    /// more than the declared length (so never more than the limit).
    #[must_use]
    pub const fn buffered_capacity(&self) -> usize {
        match &self.state {
            State::Body { body, .. } => body.capacity(),
            State::Header | State::Skip { .. } | State::Failed(_) => 0,
        }
    }

    /// Takes bytes from `input` up to the end of at most one frame. Feed the rest of the input
    /// again until it is consumed. An error is final: every later call returns it again.
    pub fn feed(&mut self, input: &[u8]) -> Result<Progress, FrameError> {
        let mut consumed = 0;
        let mut oversized = None;
        if let State::Header = self.state {
            (consumed, oversized) = match self.feed_header(input) {
                Ok(Some(taken)) => taken,
                Ok(None) => {
                    return Ok(Progress {
                        consumed: input.len(),
                        frame: None,
                        oversized: None,
                    });
                }
                Err(err) => return Err(self.fail(err)),
            };
        }
        let rest = input.get(consumed..).unwrap_or_default();
        match &mut self.state {
            State::Failed(err) => Err(*err),
            State::Header => Ok(Progress {
                consumed,
                frame: None,
                oversized,
            }),
            State::Skip { remaining } => {
                let take = usize::try_from(*remaining).map_or(rest.len(), |r| r.min(rest.len()));
                *remaining = remaining.saturating_sub(u64::try_from(take).unwrap_or(u64::MAX));
                if *remaining == 0 {
                    self.state = State::Header;
                }
                Ok(Progress {
                    consumed: consumed.saturating_add(take),
                    frame: None,
                    oversized,
                })
            }
            State::Body { len, body } => {
                let len = *len;
                let need = len.saturating_sub(body.len());
                let take = need.min(rest.len());
                let chunk = rest.get(..take).unwrap_or_default();
                let needed = body.len().saturating_add(take);
                if needed > body.capacity() {
                    let grown = needed.max(body.capacity().saturating_mul(2)).min(len);
                    if body
                        .try_reserve_exact(grown.saturating_sub(body.len()))
                        .is_err()
                    {
                        return Err(self.fail(FrameError::OutOfMemory { len }));
                    }
                }
                body.extend_from_slice(chunk);
                let consumed = consumed.saturating_add(take);
                if body.len() < len {
                    return Ok(Progress {
                        consumed,
                        frame: None,
                        oversized,
                    });
                }
                let frame = core::mem::take(body);
                self.state = State::Header;
                Ok(Progress {
                    consumed,
                    frame: Some(frame),
                    oversized,
                })
            }
        }
    }

    /// `Ok` when the stream may end here: between frames, with no partial header or body.
    pub fn finish(&self) -> Result<(), FrameError> {
        match &self.state {
            State::Failed(err) => Err(*err),
            State::Body { .. } | State::Skip { .. } => Err(FrameError::Truncated),
            State::Header if self.header_len > 0 => Err(FrameError::Truncated),
            State::Header => Ok(()),
        }
    }

    fn fail(&mut self, err: FrameError) -> FrameError {
        self.state = State::Failed(err);
        err
    }

    /// Copies header bytes until the empty line; returns how many bytes of `input` the header
    /// took once it is complete and checked (the state is then `Body`, or `Skip` with the
    /// oversized frame reported), or `None` when all of `input` went into a header still open.
    fn feed_header(
        &mut self,
        input: &[u8],
    ) -> Result<Option<(usize, Option<Oversized>)>, FrameError> {
        for (at, &byte) in input.iter().enumerate() {
            let previous = self
                .header_len
                .checked_sub(1)
                .and_then(|i| self.header.get(i))
                .copied();
            if !header_byte(byte) || (byte == b'\n' && previous != Some(b'\r')) {
                return Err(FrameError::MalformedHeader);
            }
            let slot = self
                .header
                .get_mut(self.header_len)
                .ok_or(FrameError::HeaderTooLong)?;
            *slot = byte;
            self.header_len = self.header_len.saturating_add(1);
            let block = self.header.get(..self.header_len).unwrap_or_default();
            if block == b"\r\n" {
                return Err(FrameError::MissingContentLength);
            }
            if block.ends_with(b"\r\n\r\n") {
                let lines = block
                    .get(..block.len().saturating_sub(4))
                    .unwrap_or_default();
                let length = parse_header(lines, self.limit)?;
                self.header_len = 0;
                let taken = at.saturating_add(1);
                return Ok(Some(match length {
                    Length::Within(len) => {
                        self.state = State::Body {
                            len,
                            body: Vec::new(),
                        };
                        (taken, None)
                    }
                    Length::Oversized(declared) => {
                        self.state = State::Skip {
                            remaining: declared,
                        };
                        let limit = self.limit;
                        (taken, Some(Oversized { declared, limit }))
                    }
                }));
            }
        }
        Ok(None)
    }
}

/// Printable ASCII, horizontal tab, CR and LF: what a header line may hold.
const fn header_byte(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\r' | b'\n' | b' '..=b'~')
}

/// The body length a header block (without its final empty line) declares.
fn parse_header(lines: &[u8], limit: usize) -> Result<Length, FrameError> {
    // Every byte is printable ASCII, tab, CR or LF (`header_byte`), so the block is UTF-8.
    let text = core::str::from_utf8(lines).map_err(|_| FrameError::MalformedHeader)?;
    let mut length = None;
    let mut content_type = false;
    for line in text.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or(FrameError::MalformedHeader)?;
        if name.is_empty() || !name.bytes().all(token_byte) || value.contains(['\r', '\n']) {
            return Err(FrameError::MalformedHeader);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case(CONTENT_LENGTH) {
            if length.is_some() {
                return Err(FrameError::DuplicateHeader);
            }
            length = Some(parse_length(value, limit)?);
        } else if name.eq_ignore_ascii_case(CONTENT_TYPE) {
            if content_type {
                return Err(FrameError::DuplicateHeader);
            }
            content_type = true;
            check_content_type(value)?;
        } else {
            return Err(FrameError::UnknownHeader);
        }
    }
    length.ok_or(FrameError::MissingContentLength)
}

/// RFC 9110's `tchar`: what a field name is made of.
const fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn parse_length(value: &str, limit: usize) -> Result<Length, FrameError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FrameError::InvalidContentLength);
    }
    let declared = value.bytes().try_fold(0_u64, |acc, digit| {
        acc.checked_mul(10)?
            .checked_add(u64::from(digit.saturating_sub(b'0')))
    });
    let Some(declared) = declared else {
        return Err(FrameError::TooLarge {
            declared: u64::MAX,
            limit,
        });
    };
    match usize::try_from(declared) {
        Ok(len) if len <= limit => Ok(Length::Within(len)),
        _ => Ok(Length::Oversized(declared)),
    }
}

/// `application/vscode-jsonrpc`, optionally with `charset=utf-8` (or LSP's legacy `utf8`).
fn check_content_type(value: &str) -> Result<(), FrameError> {
    let mut parts = value.split(';').map(|part| part.trim_matches([' ', '\t']));
    if !parts
        .next()
        .is_some_and(|media| media.eq_ignore_ascii_case(MEDIA_TYPE))
    {
        return Err(FrameError::UnsupportedContentType);
    }
    for parameter in parts {
        let charset = parameter
            .split_once('=')
            .filter(|(key, _)| {
                key.trim_matches([' ', '\t'])
                    .eq_ignore_ascii_case("charset")
            })
            .map(|(_, value)| value.trim_matches([' ', '\t', '"']));
        match charset {
            Some(name)
                if name.eq_ignore_ascii_case("utf-8") || name.eq_ignore_ascii_case("utf8") => {}
            _ => return Err(FrameError::UnsupportedContentType),
        }
    }
    Ok(())
}

/// The frame of `body`: `Content-Length: N`, an empty line, then the body. A body longer than
/// `limit` is refused, so a peer is never sent what it must refuse.
///
/// Pinned by: `tests/properties.rs::should_decode_what_it_encodes_when_bodies_are_arbitrary`.
pub fn encode_frame(body: &[u8], limit: usize) -> Result<Vec<u8>, FrameError> {
    if body.len() > limit {
        return Err(FrameError::TooLarge {
            declared: u64::try_from(body.len()).unwrap_or(u64::MAX),
            limit,
        });
    }
    let mut header = alloc::string::String::with_capacity(32);
    // Writing to a String cannot fail.
    let _ = write!(header, "Content-Length: {}\r\n\r\n", body.len());
    let mut frame = Vec::new();
    frame
        .try_reserve_exact(header.len().saturating_add(body.len()))
        .map_err(|_| FrameError::OutOfMemory { len: body.len() })?;
    frame.extend_from_slice(header.as_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn decode_one(input: &[u8]) -> Result<Vec<u8>, FrameError> {
        let mut decoder = FrameDecoder::new(64);
        let mut rest = input;
        loop {
            let progress = decoder.feed(rest)?;
            if let Some(frame) = progress.frame {
                return Ok(frame);
            }
            rest = rest.get(progress.consumed..).unwrap_or_default();
            if rest.is_empty() {
                return decoder.finish().map(|()| Vec::new());
            }
        }
    }

    #[test]
    fn should_decode_a_frame_when_the_header_is_well_formed() {
        // given
        let input = b"Content-Length: 2\r\n\r\n{}";

        // when
        let frame = decode_one(input);

        // then
        assert_eq!(frame, Ok(b"{}".to_vec()));
    }

    #[test]
    fn should_accept_a_json_rpc_content_type_when_it_names_utf8() {
        // given
        let inputs: [&[u8]; 4] = [
            b"content-type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 2\r\n\r\n{}",
            b"Content-Length:2\r\nContent-Type: application/vscode-jsonrpc; charset=utf8\r\n\r\n{}",
            b"CONTENT-LENGTH: 2 \r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}",
            b"Content-Length: 002\r\n\r\n{}",
        ];

        for input in inputs {
            // when
            let frame = decode_one(input);

            // then
            assert_eq!(frame, Ok(b"{}".to_vec()));
        }
    }

    #[test]
    fn should_refuse_a_header_when_it_is_junk() {
        // given
        let cases: [(&[u8], FrameError); 11] = [
            (
                b"X-Junk: 1\r\nContent-Length: 2\r\n\r\n{}",
                FrameError::UnknownHeader,
            ),
            (b"Content-Length 2\r\n\r\n{}", FrameError::MalformedHeader),
            (b"Content-Length: 2\n\n{}", FrameError::MalformedHeader),
            (b"Content Length: 2\r\n\r\n{}", FrameError::MalformedHeader),
            (b": 2\r\n\r\n{}", FrameError::MalformedHeader),
            (
                b"Content-Length: \xc3\xa9\r\n\r\n{}",
                FrameError::MalformedHeader,
            ),
            (
                b"Content-Length: 2\x00\r\n\r\n{}",
                FrameError::MalformedHeader,
            ),
            (
                b"Content-Length: -2\r\n\r\n{}",
                FrameError::InvalidContentLength,
            ),
            (
                b"Content-Length: 0x2\r\n\r\n{}",
                FrameError::InvalidContentLength,
            ),
            (
                b"Content-Length:\r\n\r\n{}",
                FrameError::InvalidContentLength,
            ),
            (
                b"Content-Length: 2\r\nContent-Type: text/plain\r\n\r\n{}",
                FrameError::UnsupportedContentType,
            ),
        ];

        for (input, expected) in cases {
            // when
            let frame = decode_one(input);

            // then
            assert_eq!(frame, Err(expected), "{}", input.escape_ascii());
        }
    }

    #[test]
    fn should_refuse_a_header_when_a_field_repeats() {
        // given
        let cases: [&[u8]; 3] = [
            b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            b"Content-Length: 2\r\ncontent-length: 3\r\n\r\n{}",
            b"Content-Type: application/vscode-jsonrpc\r\nContent-Length: 2\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}",
        ];

        for input in cases {
            // when
            let frame = decode_one(input);

            // then
            assert_eq!(frame, Err(FrameError::DuplicateHeader));
        }
    }

    #[test]
    fn should_refuse_a_frame_when_content_length_is_missing() {
        // given
        let cases: [&[u8]; 2] = [
            b"\r\n{}",
            b"Content-Type: application/vscode-jsonrpc\r\n\r\n{}",
        ];

        for input in cases {
            // when
            let frame = decode_one(input);

            // then
            assert_eq!(frame, Err(FrameError::MissingContentLength));
        }
    }

    #[test]
    fn should_refuse_a_header_when_it_never_ends() {
        // given
        let mut input = b"Content-Length: 2\r\n".to_vec();
        input.resize(MAX_HEADER_LEN.saturating_add(1), b'a');

        // when
        let frame = decode_one(&input);

        // then
        assert_eq!(frame, Err(FrameError::HeaderTooLong));
    }

    #[test]
    fn should_fail_the_stream_when_content_length_exceeds_u64() {
        // given
        let mut decoder = FrameDecoder::new(64);
        let header = b"Content-Length: 18446744073709551616000\r\n\r\n";

        // when
        let result = decoder.feed(header);

        // then
        assert_eq!(
            result,
            Err(FrameError::TooLarge {
                declared: u64::MAX,
                limit: 64
            })
        );
        assert_eq!(decoder.buffered_capacity(), 0);
    }

    #[test]
    fn should_skip_an_oversized_frame_and_decode_the_next_when_fed_byte_by_byte() {
        // given
        let mut decoder = FrameDecoder::new(4);
        let input = b"Content-Length: 10\r\n\r\n0123456789Content-Length: 2\r\n\r\n{}";
        let mut reports = vec![];
        let mut frames = vec![];
        let mut peak = 0;

        // when
        for byte in input.chunks(1) {
            let progress = decoder.feed(byte).unwrap();
            assert_eq!(progress.consumed, 1);
            peak = peak.max(decoder.buffered_capacity());
            reports.extend(progress.oversized);
            frames.extend(progress.frame);
        }

        // then
        assert_eq!(
            reports,
            [Oversized {
                declared: 10,
                limit: 4
            }]
        );
        assert_eq!(frames, [b"{}".to_vec()]);
        assert!(peak <= 2, "buffered {peak} bytes");
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn should_report_and_skip_in_one_feed_when_the_oversized_body_has_arrived() {
        // given
        let mut decoder = FrameDecoder::new(4);
        let input = b"Content-Length: 5\r\n\r\nabcdeContent-Length: 1\r\n\r\nx";

        // when
        let first = decoder.feed(input).unwrap();
        let second = decoder.feed(&input[first.consumed..]).unwrap();

        // then
        assert_eq!(first.consumed, 26);
        assert_eq!(
            first.oversized,
            Some(Oversized {
                declared: 5,
                limit: 4
            })
        );
        assert_eq!(first.frame, None);
        assert_eq!(second.frame, Some(b"x".to_vec()));
        assert_eq!(second.oversized, None);
    }

    #[test]
    fn should_report_truncation_when_the_stream_ends_inside_a_skipped_frame() {
        // given
        let mut decoder = FrameDecoder::new(4);

        // when
        let progress = decoder
            .feed(b"Content-Length: 9000000000\r\n\r\nabc")
            .unwrap();

        // then
        assert_eq!(progress.consumed, 33);
        assert_eq!(decoder.buffered_capacity(), 0);
        assert_eq!(decoder.finish(), Err(FrameError::Truncated));
    }

    #[test]
    fn should_stay_failed_when_fed_after_a_refusal() {
        // given
        let mut decoder = FrameDecoder::new(64);
        let _ = decoder.feed(b"X: y\r\n\r\n");

        // when
        let again = decoder.feed(b"Content-Length: 2\r\n\r\n{}");

        // then
        assert_eq!(again, Err(FrameError::UnknownHeader));
        assert_eq!(decoder.finish(), Err(FrameError::UnknownHeader));
    }

    #[test]
    fn should_report_truncation_when_the_stream_ends_inside_a_frame() {
        // given
        let mut in_header = FrameDecoder::new(64);
        let mut in_body = FrameDecoder::new(64);

        // when
        let _ = in_header.feed(b"Content-Len");
        let _ = in_body.feed(b"Content-Length: 4\r\n\r\n{}");

        // then
        assert_eq!(in_header.finish(), Err(FrameError::Truncated));
        assert_eq!(in_body.finish(), Err(FrameError::Truncated));
        assert_eq!(in_body.buffered_capacity(), 2);
    }

    #[test]
    fn should_decode_back_to_back_frames_when_fed_in_one_buffer() {
        // given
        let mut decoder = FrameDecoder::new(64);
        let input =
            b"Content-Length: 1\r\n\r\naContent-Length: 0\r\n\r\nContent-Length: 2\r\n\r\nbc";
        let mut rest: &[u8] = input;
        let mut frames = vec![];

        // when
        while !rest.is_empty() {
            let progress = decoder.feed(rest).unwrap();
            assert!(progress.consumed > 0);
            frames.extend(progress.frame);
            rest = &rest[progress.consumed..];
        }

        // then
        assert_eq!(frames, [b"a".to_vec(), vec![], b"bc".to_vec()]);
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn should_refuse_to_encode_a_body_when_it_exceeds_the_limit() {
        // given
        let body = [b'x'; 65];

        // when
        let encoded = encode_frame(&body, 64);

        // then
        assert_eq!(
            encoded,
            Err(FrameError::TooLarge {
                declared: 65,
                limit: 64
            })
        );
    }
}
