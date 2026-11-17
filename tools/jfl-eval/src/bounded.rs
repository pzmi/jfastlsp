//! Bounded reads of untrusted files: transcripts, trajectories and Harbor results come from an
//! agent's run, so a file is never read past a limit and a line never grows past one.
//!
//! An over-long line is skipped to its end and counted, never allocated whole; a file over its
//! limit is read to the limit and the cut is counted. Pinned by the tests below and by
//! `transcript::tests::should_count_and_skip_a_line_when_it_is_over_the_limit`.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::error::{Context, EvalError, Result};

/// The longest transcript line kept (a tool result holding a large file read fits).
pub const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
/// The most read of one transcript file.
pub const MAX_JSONL_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// The largest JSON document read whole (`result.json`, `trajectory.json`).
pub const MAX_JSON_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// The largest TOML file read (a task set or an arm).
pub const MAX_TOML_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// One line of a bounded read.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// A line within the limit, without its newline (the last line may lack one).
    Kept(Vec<u8>),
    /// A line longer than the limit: skipped to its newline, its bytes dropped.
    TooLong,
}

/// Read the next line of `reader`, keeping at most `max` bytes of it; `None` at the end.
pub fn next_line(reader: &mut impl BufRead, max: usize) -> std::io::Result<Option<Line>> {
    let mut kept: Vec<u8> = Vec::new();
    let mut too_long = false;
    let mut saw_any = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        saw_any = true;
        let (chunk, found_newline) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (available.get(..at).unwrap_or_default(), true),
            None => (available, false),
        };
        if !too_long {
            if kept.len().saturating_add(chunk.len()) > max {
                too_long = true;
                kept = Vec::new();
            } else {
                kept.extend_from_slice(chunk);
            }
        }
        let consumed = if found_newline {
            chunk.len().saturating_add(1)
        } else {
            chunk.len()
        };
        reader.consume(consumed);
        if found_newline {
            break;
        }
    }
    if !saw_any {
        return Ok(None);
    }
    Ok(Some(if too_long {
        Line::TooLong
    } else {
        Line::Kept(kept)
    }))
}

/// A file opened for a bounded line read: at most `max` bytes, and whether it was cut.
pub fn open_lines(path: &Path, max: u64) -> Result<(BufReader<std::io::Take<File>>, bool)> {
    let file = File::open(path).context(format!("open {}", path.display()))?;
    let len = file
        .metadata()
        .context(format!("stat {}", path.display()))?
        .len();
    Ok((BufReader::new(file.take(max)), len > max))
}

/// The whole file, refused when it is larger than `max`.
pub fn read_file(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = File::open(path).context(format!("open {}", path.display()))?;
    let len = file
        .metadata()
        .context(format!("stat {}", path.display()))?
        .len();
    if len > max {
        return Err(EvalError::new(format!(
            "{}: {len} bytes, over the {max}-byte limit",
            path.display()
        )));
    }
    let mut out = Vec::new();
    file.take(max)
        .read_to_end(&mut out)
        .context(format!("read {}", path.display()))?;
    Ok(out)
}

/// The whole file as UTF-8, refused when larger than `max` or not UTF-8.
pub fn read_text(path: &Path, max: u64) -> Result<String> {
    String::from_utf8(read_file(path, max)?)
        .map_err(|_| EvalError::new(format!("{}: not UTF-8", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn all(input: &[u8], max: usize) -> Vec<Line> {
        let mut reader = BufReader::with_capacity(4, Cursor::new(input.to_vec()));
        let mut out = Vec::new();
        while let Some(line) = next_line(&mut reader, max).unwrap() {
            out.push(line);
        }
        out
    }

    #[test]
    fn should_split_lines_when_they_span_buffer_refills() {
        // given: a 4-byte buffer, so every line crosses refills
        let input = b"abcdefgh\nij\n\nklm";
        // when
        let lines = all(input, 100);
        // then
        assert_eq!(
            lines,
            vec![
                Line::Kept(b"abcdefgh".to_vec()),
                Line::Kept(b"ij".to_vec()),
                Line::Kept(Vec::new()),
                Line::Kept(b"klm".to_vec()),
            ]
        );
    }

    #[test]
    fn should_skip_to_the_newline_when_a_line_is_too_long() {
        // given
        let input = b"0123456789\nok\n";
        // when
        let lines = all(input, 5);
        // then
        assert_eq!(lines, vec![Line::TooLong, Line::Kept(b"ok".to_vec())]);
    }

    #[test]
    fn should_return_nothing_when_the_input_is_empty() {
        // given
        // when
        let lines = all(b"", 5);
        // then
        assert!(lines.is_empty());
    }
}
