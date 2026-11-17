//! Offset ↔ position conversion in process (ver §10, doc/bench/README.md's in-process convention):
//! building a [`LineIndex`] and converting offsets to UTF-16 positions and back.
//!
//! Inputs: a synthetic Java-like text built here (always), and the pinned corpus's `.java` files
//! when `just bench-domain SIZE` streams them on stdin, each followed by a NUL byte. stdin, not
//! a path, because jfl-domain holds no filesystem grant (fnd §2.4). Before timing the corpus the
//! benchmark checks every file the way `tests/properties.rs` checks generated text: every char
//! boundary round-trips through a position in each encoding, and the columns agree with
//! `line-index` (which reads lone `\r`s as `\n`s). A mismatch exits 1 before any timing. Not run by `check`.

use std::io::{IsTerminal, Read};
use std::sync::OnceLock;

use divan::Bencher;
use divan::counter::{BytesCount, ItemsCount};
use jfl_domain::{ByteOffset, LineIndex, Position, PositionEncoding};

const ENCODINGS: [PositionEncoding; 3] = [
    PositionEncoding::Utf8,
    PositionEncoding::Utf16,
    PositionEncoding::Utf32,
];

/// One of every this many char boundaries is converted in the conversion benches.
const STRIDE: usize = 7;

static SYNTHETIC: OnceLock<String> = OnceLock::new();
static CORPUS: OnceLock<Vec<String>> = OnceLock::new();

/// About 64 KiB of Java-like source: mostly ASCII, a non-ASCII comment every few lines (a
/// 2-byte, a 3-byte and an astral scalar), CRLF on every other block. Deterministic.
fn synthetic() -> &'static str {
    SYNTHETIC.get_or_init(|| {
        let mut text = String::new();
        let mut block = 0_u32;
        while text.len() < 64 * 1024 {
            let newline = if block.is_multiple_of(2) {
                "\n"
            } else {
                "\r\n"
            };
            for line in [
                "    /** Returns the café total in € for the given 😀 order. */",
                "    public static long total(final java.util.List<Order> orders) {",
                "        long sum = 0L;",
                "        for (Order order : orders) { sum += order.amount(); }",
                "        return sum;",
                "    }",
                "",
            ] {
                text.push_str(line);
                text.push_str(newline);
            }
            block = block.wrapping_add(1);
        }
        text
    })
}

/// The corpus files from stdin (NUL-separated), or none when stdin is a terminal or empty.
fn corpus() -> &'static [String] {
    CORPUS.get_or_init(|| {
        let mut stdin = std::io::stdin();
        if stdin.is_terminal() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if let Err(err) = stdin.read_to_end(&mut bytes) {
            eprintln!("bench_positions_stdin_failed: {err}");
            return Vec::new();
        }
        let mut skipped = 0_usize;
        let files: Vec<String> = bytes
            .split(|&b| b == 0)
            .filter(|chunk| !chunk.is_empty())
            .filter_map(|chunk| match std::str::from_utf8(chunk) {
                Ok(text) => Some(text.to_owned()),
                Err(_) => {
                    skipped = skipped.saturating_add(1);
                    None
                }
            })
            .collect();
        if skipped > 0 {
            eprintln!("corpus: {skipped} files are not UTF-8 and are left out");
        }
        files
    })
}

/// Whether `at` is the `\n` of a `\r\n`.
fn inside_crlf(text: &str, at: usize) -> bool {
    let bytes = text.as_bytes();
    at.checked_sub(1).and_then(|before| bytes.get(before)) == Some(&b'\r')
        && bytes.get(at) == Some(&b'\n')
}

/// Every check of the module doc over one file; the reason for the first mismatch.
fn check_file(text: &str) -> Result<u64, String> {
    let ours = LineIndex::new(text).map_err(|e| e.to_string())?;
    // line-index splits on `\n` only: it reads a copy whose lone `\r`s are `\n`s (same offsets).
    let bytes = text.as_bytes();
    let as_lf: String = text
        .char_indices()
        .map(|(at, c)| {
            if c == '\r' && bytes.get(at.saturating_add(1)) != Some(&b'\n') {
                '\n'
            } else {
                c
            }
        })
        .collect();
    let oracle = line_index::LineIndex::new(&as_lf);
    let mut checked = 0_u64;
    let ends = text.char_indices().map(|(at, _)| at).chain([text.len()]);
    for at in ends.filter(|&at| !inside_crlf(text, at)) {
        let offset = ByteOffset::new(u32::try_from(at).map_err(|e| e.to_string())?);
        let mut found = Vec::with_capacity(ENCODINGS.len());
        for encoding in ENCODINGS {
            let position = ours.position(offset, encoding).map_err(|e| e.to_string())?;
            if ours.offset(position, encoding) != Ok(offset) {
                return Err(format!("offset {at} does not round-trip in {encoding}"));
            }
            found.push(position);
        }
        let utf8 = oracle.line_col(line_index::TextSize::new(offset.get()));
        let wide = |e| {
            oracle
                .to_wide(e, utf8)
                .map(|p| Position::new(p.line, p.col))
        };
        let expected = vec![
            Some(Position::new(utf8.line, utf8.col)),
            wide(line_index::WideEncoding::Utf16),
            wide(line_index::WideEncoding::Utf32),
        ];
        if found.into_iter().map(Some).collect::<Vec<_>>() != expected {
            return Err(format!("offset {at} disagrees with line-index"));
        }
        checked = checked.saturating_add(1);
    }
    Ok(checked)
}

fn main() {
    let files = corpus();
    if !files.is_empty() {
        let bytes: usize = files.iter().map(String::len).sum();
        let mut boundaries = 0_u64;
        for (n, file) in files.iter().enumerate() {
            match check_file(file) {
                Ok(checked) => boundaries = boundaries.saturating_add(checked),
                Err(why) => {
                    eprintln!("corpus check failed in file {n} of the stream: {why}");
                    std::process::exit(1);
                }
            }
        }
        println!(
            "corpus check: {} files, {bytes} bytes, {boundaries} char boundaries round-trip in utf-8, utf-16 and utf-32 and agree with line-index",
            files.len()
        );
    }
    divan::main();
}

/// Every `STRIDE`th char boundary of `text` that is not inside a `\r\n`.
fn sample_offsets(text: &str) -> Vec<ByteOffset> {
    text.char_indices()
        .map(|(at, _)| at)
        .step_by(STRIDE)
        .filter(|&at| !inside_crlf(text, at))
        .filter_map(|at| u32::try_from(at).ok().map(ByteOffset::new))
        .collect()
}

fn utf16_positions(index: &LineIndex, offsets: &[ByteOffset]) -> Vec<Position> {
    offsets
        .iter()
        .filter_map(|&o| index.position(o, PositionEncoding::Utf16).ok())
        .collect()
}

#[divan::bench_group(name = "synthetic_64k")]
mod synthetic_text {
    use super::*;

    #[divan::bench]
    fn build_line_index(bencher: Bencher) {
        let text = synthetic();
        bencher
            .counter(BytesCount::of_str(text))
            .bench(|| LineIndex::new(divan::black_box(text)));
    }

    #[divan::bench]
    fn offset_to_utf16_position(bencher: Bencher) {
        let text = synthetic();
        let Ok(index) = LineIndex::new(text) else {
            return;
        };
        let offsets = sample_offsets(text);
        bencher.counter(ItemsCount::of_iter(&offsets)).bench(|| {
            for &offset in &offsets {
                let _ = divan::black_box(index.position(offset, PositionEncoding::Utf16));
            }
        });
    }

    #[divan::bench]
    fn utf16_position_to_offset(bencher: Bencher) {
        let text = synthetic();
        let Ok(index) = LineIndex::new(text) else {
            return;
        };
        let positions = utf16_positions(&index, &sample_offsets(text));
        bencher.counter(ItemsCount::of_iter(&positions)).bench(|| {
            for &position in &positions {
                let _ = divan::black_box(index.offset(position, PositionEncoding::Utf16));
            }
        });
    }
}

#[divan::bench_group(name = "corpus")]
mod corpus_files {
    use super::*;

    #[divan::bench]
    fn build_line_index_per_file(bencher: Bencher) {
        let files = corpus();
        let bytes: usize = files.iter().map(String::len).sum();
        bencher.counter(BytesCount::new(bytes)).bench(|| {
            for file in files {
                let _ = divan::black_box(LineIndex::new(divan::black_box(file)));
            }
        });
    }

    #[divan::bench]
    fn offset_to_utf16_position(bencher: Bencher) {
        let prepared: Vec<(LineIndex, Vec<ByteOffset>)> = corpus()
            .iter()
            .filter_map(|f| Some((LineIndex::new(f).ok()?, sample_offsets(f))))
            .collect();
        let count: usize = prepared.iter().map(|(_, o)| o.len()).sum();
        bencher.counter(ItemsCount::new(count)).bench(|| {
            for (index, offsets) in &prepared {
                for &offset in offsets {
                    let _ = divan::black_box(index.position(offset, PositionEncoding::Utf16));
                }
            }
        });
    }

    #[divan::bench]
    fn utf16_position_to_offset(bencher: Bencher) {
        let prepared: Vec<(LineIndex, Vec<Position>)> = corpus()
            .iter()
            .filter_map(|f| {
                let index = LineIndex::new(f).ok()?;
                let positions = utf16_positions(&index, &sample_offsets(f));
                Some((index, positions))
            })
            .collect();
        let count: usize = prepared.iter().map(|(_, p)| p.len()).sum();
        bencher.counter(ItemsCount::new(count)).bench(|| {
            for (index, positions) in &prepared {
                for &position in positions {
                    let _ = divan::black_box(index.offset(position, PositionEncoding::Utf16));
                }
            }
        });
    }

    /// Every sampled (file, offset) in a fixed pseudo-random order (a Fibonacci-hash sort, no
    /// randomness): the access pattern of LSP requests, where the sequential benches above are
    /// cache- and branch-friendly.
    fn shuffled<T: Copy>(per_file: &[(LineIndex, Vec<T>)]) -> Vec<(usize, T)> {
        let mut all: Vec<(u64, usize, T)> = per_file
            .iter()
            .enumerate()
            .flat_map(|(file, (_, items))| items.iter().map(move |&item| (file, item)))
            .zip(0_u64..)
            .map(|((file, item), n)| (n.wrapping_mul(0x9E37_79B9_7F4A_7C15), file, item))
            .collect();
        all.sort_unstable_by_key(|&(key, _, _)| key);
        all.into_iter()
            .map(|(_, file, item)| (file, item))
            .collect()
    }

    #[divan::bench]
    fn offset_to_utf16_position_shuffled(bencher: Bencher) {
        let prepared: Vec<(LineIndex, Vec<ByteOffset>)> = corpus()
            .iter()
            .filter_map(|f| Some((LineIndex::new(f).ok()?, sample_offsets(f))))
            .collect();
        let order = shuffled(&prepared);
        bencher.counter(ItemsCount::of_iter(&order)).bench(|| {
            for &(file, offset) in &order {
                if let Some((index, _)) = prepared.get(file) {
                    let _ = divan::black_box(index.position(offset, PositionEncoding::Utf16));
                }
            }
        });
    }

    #[divan::bench]
    fn utf16_position_to_offset_shuffled(bencher: Bencher) {
        let prepared: Vec<(LineIndex, Vec<Position>)> = corpus()
            .iter()
            .filter_map(|f| {
                let index = LineIndex::new(f).ok()?;
                let positions = utf16_positions(&index, &sample_offsets(f));
                Some((index, positions))
            })
            .collect();
        let order = shuffled(&prepared);
        bencher.counter(ItemsCount::of_iter(&order)).bench(|| {
            for &(file, position) in &order {
                if let Some((index, _)) = prepared.get(file) {
                    let _ = divan::black_box(index.offset(position, PositionEncoding::Utf16));
                }
            }
        });
    }
}
