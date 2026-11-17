//! LSP base-protocol framing (`Content-Length` headers), bounded before allocation.
//!
//! Hand-rolled rather than `lsp-server`'s `Message::read`: that one allocates whatever
//! `Content-Length` says and parses into typed messages, while a proxy needs the raw body, a
//! frame limit checked before the buffer exists, and `lsp-server` is wrapped to the LSP and IPC
//! adapters by `deny.toml`. Pinned by the tests below.

use std::fmt;
use std::io::{self, BufRead, Read, Write};

/// The largest body accepted (plan §2.5: 16 MiB, refused before allocation).
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// The longest header line accepted, terminator included.
const MAX_HEADER_LINE: u64 = 1024;

/// The most header lines in one frame.
const MAX_HEADERS: usize = 16;

/// Why a frame could not be read.
#[derive(Debug)]
pub enum FrameError {
    /// The underlying stream failed.
    Io(io::Error),
    /// The headers ended without a `Content-Length`.
    MissingLength,
    /// A header line was malformed, too long, or there were too many.
    BadHeader,
    /// The announced body is larger than the limit; nothing was allocated for it.
    TooLarge {
        /// The announced length.
        len: usize,
        /// The limit in force.
        max: usize,
    },
    /// The stream ended inside a frame.
    Truncated,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "frame i/o error: {e}"),
            Self::MissingLength => f.write_str("frame has no Content-Length"),
            Self::BadHeader => f.write_str("malformed frame header"),
            Self::TooLarge { len, max } => write!(f, "frame of {len} bytes exceeds {max}"),
            Self::Truncated => f.write_str("stream ended inside a frame"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<FrameError> for io::Error {
    fn from(e: FrameError) -> Self {
        match e {
            FrameError::Io(e) => e,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}

/// Reads one frame's body. `Ok(None)` is a clean end of stream between frames.
///
/// # Errors
///
/// A [`FrameError`] when the stream fails, the headers are malformed, or the body is larger than
/// `max` (checked before the body buffer is allocated).
pub fn read_frame<R: BufRead>(reader: &mut R, max: usize) -> Result<Option<Vec<u8>>, FrameError> {
    let mut length: Option<usize> = None;
    let mut line = Vec::new();
    for index in 0..=MAX_HEADERS {
        line.clear();
        let n = reader
            .by_ref()
            .take(MAX_HEADER_LINE)
            .read_until(b'\n', &mut line)
            .map_err(FrameError::Io)?;
        if n == 0 {
            return if index == 0 {
                Ok(None)
            } else {
                Err(FrameError::Truncated)
            };
        }
        if line.last() != Some(&b'\n') {
            return Err(if n as u64 >= MAX_HEADER_LINE {
                FrameError::BadHeader
            } else {
                FrameError::Truncated
            });
        }
        let text = std::str::from_utf8(&line).map_err(|_| FrameError::BadHeader)?;
        let text = text.trim_end_matches('\n').trim_end_matches('\r');
        if text.is_empty() {
            let len = length.ok_or(FrameError::MissingLength)?;
            if len > max {
                return Err(FrameError::TooLarge { len, max });
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).map_err(|e| match e.kind() {
                io::ErrorKind::UnexpectedEof => FrameError::Truncated,
                _ => FrameError::Io(e),
            })?;
            return Ok(Some(body));
        }
        let (name, value) = text.split_once(':').ok_or(FrameError::BadHeader)?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            let parsed = value
                .trim()
                .parse::<usize>()
                .map_err(|_| FrameError::BadHeader)?;
            length = Some(parsed);
        }
    }
    Err(FrameError::BadHeader)
}

/// Writes one frame and flushes.
///
/// # Errors
///
/// The writer's error.
pub fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(&mut out, body.as_bytes()).unwrap();
        out
    }

    #[test]
    fn should_round_trip_bodies_when_written_then_read() {
        // given
        let mut bytes = framed("{\"a\":1}");
        bytes.extend(framed("{}"));
        let mut reader = Cursor::new(bytes);

        // when
        let first = read_frame(&mut reader, MAX_FRAME_BYTES).unwrap();
        let second = read_frame(&mut reader, MAX_FRAME_BYTES).unwrap();
        let end = read_frame(&mut reader, MAX_FRAME_BYTES).unwrap();

        // then
        assert_eq!(first.as_deref(), Some(&b"{\"a\":1}"[..]));
        assert_eq!(second.as_deref(), Some(&b"{}"[..]));
        assert!(end.is_none());
    }

    #[test]
    fn should_accept_other_headers_and_any_case_when_content_length_present() {
        // given
        let raw = b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}";

        // when
        let body = read_frame(&mut Cursor::new(&raw[..]), MAX_FRAME_BYTES).unwrap();

        // then
        assert_eq!(body.as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn should_refuse_before_allocating_when_length_exceeds_limit() {
        // given: a header announcing far more than the limit, and no body at all
        let raw = format!("Content-Length: {}\r\n\r\n", usize::MAX);

        // when
        let err = read_frame(&mut Cursor::new(raw.into_bytes()), 64).unwrap_err();

        // then
        assert!(matches!(err, FrameError::TooLarge { max: 64, .. }), "{err}");
    }

    #[test]
    fn should_refuse_when_length_missing_or_header_malformed() {
        // given
        let cases = [
            &b"X: 1\r\n\r\n{}"[..],
            b"Content-Length 2\r\n\r\n{}",
            b"Content-Length: -1\r\n\r\n",
        ];

        // when
        let errors: Vec<FrameError> = cases
            .iter()
            .map(|raw| read_frame(&mut Cursor::new(*raw), MAX_FRAME_BYTES).unwrap_err())
            .collect();

        // then
        for err in errors {
            assert!(
                matches!(err, FrameError::MissingLength | FrameError::BadHeader),
                "{err}"
            );
        }
    }

    #[test]
    fn should_refuse_when_a_header_line_is_unbounded() {
        // given
        let raw = vec![b'a'; 10_000];

        // when
        let err = read_frame(&mut Cursor::new(raw), MAX_FRAME_BYTES).unwrap_err();

        // then
        assert!(matches!(err, FrameError::BadHeader), "{err}");
    }

    #[test]
    fn should_refuse_when_too_many_headers() {
        // given
        let raw = "X: 1\r\n".repeat(100);

        // when
        let err = read_frame(&mut Cursor::new(raw.into_bytes()), MAX_FRAME_BYTES).unwrap_err();

        // then
        assert!(matches!(err, FrameError::BadHeader), "{err}");
    }

    #[test]
    fn should_report_truncation_and_never_panic_when_cut_at_any_byte() {
        // given
        let full = framed("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"x\"}");

        for cut in 1..full.len() {
            // when
            let result = read_frame(&mut Cursor::new(&full[..cut]), MAX_FRAME_BYTES);

            // then
            assert!(
                matches!(result, Err(FrameError::Truncated)),
                "cut at {cut}: {result:?}"
            );
        }
    }
}
