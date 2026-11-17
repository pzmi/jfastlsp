//! A byte pipe in memory, for running a session in process: the in-process harness of
//! `tests/server.rs` (rust-analyzer's slow-tests pattern) and the benchmark.
//!
//! `lsp_server::Connection::memory()` is not used for this: it carries `lsp_server::Message`, whose
//! result is a `serde_json::Value`, so a session on it could not keep results on the typed path
//! (plan §5.3), and it would bypass the frame decoder under test. Its channels are unbounded too
//! (fnd §4.4 rule 5). A session in process runs on bytes instead, through the same reader and
//! writer as on stdio, so the harness measures and tests framing as well.

use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// Writes not yet read before a writer waits: one session's worth of small messages.
const PIPE_CAPACITY: usize = 256;

/// The writing end; dropping it is the reader's end of stream.
#[derive(Debug)]
pub struct PipeWriter(SyncSender<Vec<u8>>);

/// The reading end.
#[derive(Debug)]
pub struct PipeReader {
    chunks: Receiver<Vec<u8>>,
    current: Vec<u8>,
    at: usize,
}

/// A bounded pipe: each `write` is one chunk, at most [`PIPE_CAPACITY`] of them in flight.
#[must_use]
pub fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = sync_channel(PIPE_CAPACITY);
    (
        PipeWriter(tx),
        PipeReader {
            chunks: rx,
            current: Vec::new(),
            at: 0,
        },
    )
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.0
            .send(buf.to_vec())
            .map(|()| buf.len())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for PipeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.current.len() {
            match self.chunks.recv() {
                Ok(chunk) => {
                    self.current = chunk;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let rest = self.current.get(self.at..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        let (into, _) = buf.split_at_mut(n);
        into.copy_from_slice(rest.get(..n).unwrap_or_default());
        self.at = self.at.saturating_add(n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_what_was_written_when_the_writer_is_dropped() {
        // given
        let (mut writer, mut reader) = pipe();
        writer.write_all(b"hello, ").unwrap();
        writer.write_all(b"world").unwrap();
        drop(writer);

        // when
        let mut got = String::new();
        reader.read_to_string(&mut got).unwrap();

        // then
        assert_eq!(got, "hello, world");
    }

    #[test]
    fn should_fail_a_write_when_the_reader_is_gone() {
        // given
        let (mut writer, reader) = pipe();
        drop(reader);

        // when
        let got = writer.write(b"x");

        // then
        assert_eq!(got.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }
}
