//! Byte offsets, positions and their encodings (plan §2.3).
//!
//! The engine stores byte offsets into a file's UTF-8 bytes; a carrier speaks (line, column) in
//! the encoding it negotiated. [`LineIndex`] converts between the two without keeping the text:
//! it records where each line starts and ends and where every non-ASCII scalar sits, so a line of
//! ASCII converts by subtraction and only lines with wide characters walk them.
//!
//! Line terminators are the three of LSP 3.17 (and of the JLS, §3.4): `\n`, `\r\n` and a lone
//! `\r`. A `\r\n` is one terminator: the offset between its two bytes is inside it and refused,
//! so no offset maps to a column past the end of its line.
//!
//! Every conversion is total over its inputs: a position past a line or file end, or one that
//! lands inside a UTF-8 sequence, a surrogate pair or a `\r\n`, is a [`PositionError`], never a
//! clamp that silently answers for another symbol.
//!
//! Rejected alternatives: `line-index` (rust-analyzer, ADR 0020) is not `#![no_std]` (it uses
//! `std::arch`, `std::iter` and a std `HashMap`) and fails to build for `x86_64-unknown-none`, so
//! it is the property-test oracle instead (`tests/properties.rs`); it splits lines on `\n` only,
//! so it reads a copy of the text whose lone `\r`s are `\n`s. `text-size` would only rename `u32`.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::num::NonZeroU32;

/// A byte offset into a file's UTF-8 text. Files are bounded to `u32::MAX` bytes, as LSP's
/// positions are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ByteOffset(u32);

impl ByteOffset {
    /// The offset `value` bytes from the start of the text.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The offset `value` bytes in, from a `usize` such as a parser's byte offset; one past
    /// `u32::MAX` is refused.
    ///
    /// Pinned by: `text::tests::should_refuse_a_usize_offset_when_it_does_not_fit_u32`.
    pub fn try_from_usize(value: usize) -> Result<Self, PositionError> {
        u32::try_from(value)
            .map(Self)
            .map_err(|_| PositionError::TextTooLarge { len: value })
    }

    /// The offset as a number of bytes.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A half-open byte range `start..end` with `start <= end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteRange {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteRange {
    /// The range `start..end`; a reversed range is refused.
    ///
    /// Pinned by: `text::tests::should_refuse_a_byte_range_when_it_is_reversed`.
    pub fn new(start: ByteOffset, end: ByteOffset) -> Result<Self, PositionError> {
        if start > end {
            return Err(PositionError::ReversedRange);
        }
        Ok(Self { start, end })
    }

    /// The first byte of the range.
    #[must_use]
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// The byte after the last one of the range.
    #[must_use]
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// The number of bytes in the range.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.0.saturating_sub(self.start.0)
    }

    /// Whether the range holds no byte.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// How a column counts: the unit of [`Position::column`].
///
/// The names are LSP 3.17's `PositionEncodingKind` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum PositionEncoding {
    /// UTF-8 code units (bytes): the CLI's columns and LSP's `utf-8`.
    Utf8,
    /// UTF-16 code units: LSP's default and the `--json` ranges.
    #[default]
    Utf16,
    /// Unicode scalar values: LSP's `utf-32`.
    Utf32,
}

impl PositionEncoding {
    /// The LSP `PositionEncodingKind` string: `utf-8`, `utf-16` or `utf-32`.
    #[must_use]
    pub const fn lsp_name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }

    /// The encoding an LSP `PositionEncodingKind` string names, if any (exact match, as the
    /// specification's values are lower case).
    #[must_use]
    pub fn from_lsp_name(name: &str) -> Option<Self> {
        match name {
            "utf-8" => Some(Self::Utf8),
            "utf-16" => Some(Self::Utf16),
            "utf-32" => Some(Self::Utf32),
            _ => None,
        }
    }

    /// The encoding the LSP carrier picks from the client's offered `positionEncodings`:
    /// `utf-8` when offered, `utf-16` otherwise (plan §2.3), so the common case needs no
    /// conversion and every client gets one it must support.
    ///
    /// Pinned by: `text::tests::should_pick_utf8_when_the_client_offers_it` and
    /// `text::tests::should_fall_back_to_utf16_when_the_client_offers_no_utf8`.
    #[must_use]
    pub fn negotiate<'a, I>(offered: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        if offered
            .into_iter()
            .any(|name| name == Self::Utf8.lsp_name())
        {
            Self::Utf8
        } else {
            Self::Utf16
        }
    }

    /// The units of this encoding in a scalar that takes `utf8_len` bytes.
    const fn units(self, utf8_len: u8) -> u32 {
        match self {
            Self::Utf8 => utf8_len as u32,
            Self::Utf16 => {
                if utf8_len == 4 {
                    2
                } else {
                    1
                }
            }
            Self::Utf32 => 1,
        }
    }
}

impl fmt::Display for PositionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.lsp_name())
    }
}

/// A 0-based (line, column) position; the column counts in a [`PositionEncoding`] that travels
/// beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    line: u32,
    column: u32,
}

impl Position {
    /// The position at `line`, `column`.
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }

    /// The 0-based line.
    #[must_use]
    pub const fn line(self) -> u32 {
        self.line
    }

    /// The 0-based column, in the units of the encoding in use. Arithmetic on it belongs in
    /// `jfl-domain`, which knows the encoding.
    #[must_use]
    pub const fn column(self) -> u32 {
        self.column
    }
}

/// A range of two positions with `start <= end`, in one encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Range {
    start: Position,
    end: Position,
}

impl Range {
    /// The range `start..end`; a reversed range is refused.
    ///
    /// Pinned by: `text::tests::should_refuse_a_range_when_it_is_reversed`.
    pub fn new(start: Position, end: Position) -> Result<Self, PositionError> {
        if start > end {
            return Err(PositionError::ReversedRange);
        }
        Ok(Self { start, end })
    }

    /// The first position.
    #[must_use]
    pub const fn start(self) -> Position {
        self.start
    }

    /// The position after the last character.
    #[must_use]
    pub const fn end(self) -> Position {
        self.end
    }
}

/// The CLI's `line:col`: 1-based lines and 1-based UTF-8 byte columns, as `rg --column` prints
/// them (plan §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CliPosition {
    line: NonZeroU32,
    column: NonZeroU32,
}

impl CliPosition {
    /// The 1-based `line` and byte `column`; zero in either is refused.
    ///
    /// Pinned by: `text::tests::should_refuse_a_cli_position_when_a_coordinate_is_zero`.
    pub fn new(line: u32, column: u32) -> Result<Self, CliPositionError> {
        match (NonZeroU32::new(line), NonZeroU32::new(column)) {
            (Some(line), Some(column)) => Ok(Self { line, column }),
            _ => Err(CliPositionError::Zero),
        }
    }

    /// The CLI form of a 0-based [`PositionEncoding::Utf8`] position; a coordinate at
    /// `u32::MAX` has no 1-based form and is refused.
    ///
    /// Pinned by: `text::tests::should_print_a_one_based_byte_column_when_a_line_holds_wide_characters`.
    pub fn from_utf8_position(position: Position) -> Result<Self, CliPositionError> {
        let line = position.line.checked_add(1).and_then(NonZeroU32::new);
        let column = position.column.checked_add(1).and_then(NonZeroU32::new);
        match (line, column) {
            (Some(line), Some(column)) => Ok(Self { line, column }),
            _ => Err(CliPositionError::Overflow),
        }
    }

    /// The 0-based position this names, in [`PositionEncoding::Utf8`].
    #[must_use]
    pub const fn to_utf8_position(self) -> Position {
        Position {
            line: self.line.get().saturating_sub(1),
            column: self.column.get().saturating_sub(1),
        }
    }

    /// The 1-based line.
    #[must_use]
    pub const fn line(self) -> u32 {
        self.line.get()
    }

    /// The 1-based byte column.
    #[must_use]
    pub const fn column(self) -> u32 {
        self.column.get()
    }
}

impl fmt::Display for CliPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A CLI position that cannot be formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CliPositionError {
    /// A line or column of 0: the CLI's coordinates start at 1.
    #[error("CLI lines and columns start at 1")]
    Zero,
    /// A 0-based coordinate of `u32::MAX` has no 1-based form.
    #[error("position too large for a 1-based CLI coordinate")]
    Overflow,
}

/// Why a position or offset does not name a place in the text. The values carry coordinates,
/// never source text (fnd §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PositionError {
    /// The text is larger than a [`ByteOffset`] can address.
    #[error("text of {len} bytes is larger than the 4 GiB a position can address")]
    TextTooLarge {
        /// The text's length in bytes.
        len: usize,
    },
    /// The offset lies after the last byte of the text.
    #[error("offset {offset} is past the end of the text ({len} bytes)")]
    OffsetPastEnd {
        /// The offset asked for.
        offset: u32,
        /// The text's length in bytes.
        len: u32,
    },
    /// The line does not exist.
    #[error("line {line} is past the end of the text ({line_count} lines)")]
    LinePastEnd {
        /// The 0-based line asked for.
        line: u32,
        /// How many lines the text has.
        line_count: u32,
    },
    /// The column lies after the end of its line (its terminator excluded).
    #[error("column {column} is past the end of line {line} ({line_length} units long)")]
    ColumnPastLineEnd {
        /// The 0-based line.
        line: u32,
        /// The column asked for.
        column: u32,
        /// The line's length in the requested encoding.
        line_length: u32,
    },
    /// The offset (or UTF-8 column, at this offset) splits a multi-byte UTF-8 sequence.
    #[error("offset {offset} is inside a multi-byte character")]
    InsideCharacter {
        /// The byte offset that splits the character.
        offset: u32,
    },
    /// The UTF-16 column points between the two halves of a surrogate pair.
    #[error("column {column} of line {line} is inside a surrogate pair")]
    InsideSurrogatePair {
        /// The 0-based line.
        line: u32,
        /// The UTF-16 column asked for.
        column: u32,
    },
    /// The offset lies between the `\r` and the `\n` of a `\r\n`.
    #[error("offset {offset} is inside a CRLF line terminator")]
    InsideLineTerminator {
        /// The byte offset of the `\n`.
        offset: u32,
    },
    /// A range whose start is after its end.
    #[error("range start is after its end")]
    ReversedRange,
}

/// A non-ASCII scalar: where it starts and how many bytes it takes (2 to 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wide {
    offset: u32,
    len: u8,
}

impl Wide {
    const fn end(self) -> u32 {
        self.offset.saturating_add(self.len as u32)
    }
}

/// Line starts, line ends and the non-ASCII scalars of one text: enough to convert between byte
/// offsets and positions in every [`PositionEncoding`] without the text.
///
/// A text has one more line than terminators: `"a\n"` has lines `0` (`a`) and `1` (empty), so
/// the position just after the last newline is valid, as LSP expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    len: u32,
    /// `starts[i]` is the offset of line `i`'s first byte.
    starts: Vec<u32>,
    /// `ends[i]` is the offset of line `i`'s terminator (or of the text's end).
    ends: Vec<u32>,
    /// Every non-ASCII scalar, in offset order.
    wide: Vec<Wide>,
}

impl LineIndex {
    /// Indexes `text`; a text of more than `u32::MAX` bytes is refused.
    ///
    /// Pinned by: `tests/properties.rs::should_agree_with_line_index_when_a_lone_cr_reads_as_a_newline`.
    pub fn new(text: &str) -> Result<Self, PositionError> {
        let bytes = text.as_bytes();
        let len = u32::try_from(bytes.len())
            .map_err(|_| PositionError::TextTooLarge { len: bytes.len() })?;
        let mut starts = vec![0];
        let mut ends = Vec::new();
        let mut wide = Vec::new();
        let mut after_cr = false;
        // `bytes` first: zip stops on its end without advancing the counter past `len`.
        for (&byte, at) in bytes.iter().zip(0_u32..) {
            if byte == b'\n' {
                ends.push(if after_cr { at.saturating_sub(1) } else { at });
                starts.push(at.saturating_add(1));
                after_cr = false;
                continue;
            }
            if after_cr {
                // The previous `\r` stood alone: it ended its line.
                let cr = at.saturating_sub(1);
                ends.push(cr);
                starts.push(at);
            }
            after_cr = byte == b'\r';
            if byte >= 0xC0 {
                // A lead byte of valid UTF-8: its high bits give the sequence length.
                let width = if byte >= 0xF0 {
                    4
                } else if byte >= 0xE0 {
                    3
                } else {
                    2
                };
                wide.push(Wide {
                    offset: at,
                    len: width,
                });
            }
        }
        if after_cr {
            ends.push(len.saturating_sub(1));
            starts.push(len);
        }
        ends.push(len);
        Ok(Self {
            len,
            starts,
            ends,
            wide,
        })
    }

    /// The text's length in bytes.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.len
    }

    /// Whether the text is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many lines the text has: one more than its terminators.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX)
    }

    /// The bytes of `line` without its terminator, if the line exists.
    #[must_use]
    pub fn line_range(&self, line: u32) -> Option<ByteRange> {
        let (start, end) = self.line_bounds(line)?;
        Some(ByteRange {
            start: ByteOffset(start),
            end: ByteOffset(end),
        })
    }

    /// The length of `line` (terminator excluded) in `encoding`'s units.
    ///
    /// Pinned by: `text::tests::should_measure_a_line_in_each_encoding_when_it_holds_an_astral_character`.
    pub fn line_length(&self, line: u32, encoding: PositionEncoding) -> Result<u32, PositionError> {
        let (start, end) = self.line_bounds(line).ok_or(PositionError::LinePastEnd {
            line,
            line_count: self.line_count(),
        })?;
        Ok(self.units_between(start, end, encoding))
    }

    /// The position of `offset` with its column in `encoding`.
    ///
    /// Refused: an offset past the end, one inside a multi-byte character, and the offset between
    /// the two bytes of a `\r\n`.
    ///
    /// Pinned by: `tests/properties.rs::should_round_trip_every_char_boundary_when_converting_through_a_position`
    /// and `text::tests::should_refuse_an_offset_when_it_is_past_the_end`,
    /// `text::tests::should_refuse_an_offset_when_it_splits_a_multibyte_character` and
    /// `text::tests::should_refuse_an_offset_when_it_is_inside_a_crlf`.
    pub fn position(
        &self,
        offset: ByteOffset,
        encoding: PositionEncoding,
    ) -> Result<Position, PositionError> {
        let at = offset.0;
        if at > self.len {
            return Err(PositionError::OffsetPastEnd {
                offset: at,
                len: self.len,
            });
        }
        // The last line starting at or before `at`; line 0 starts at 0, so there is one.
        let line = self
            .starts
            .partition_point(|&start| start <= at)
            .saturating_sub(1);
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        let (start, end) = self.line_bounds(line).ok_or(PositionError::OffsetPastEnd {
            offset: at,
            len: self.len,
        })?;
        if at > end {
            return Err(PositionError::InsideLineTerminator { offset: at });
        }
        // Only the last wide character starting before `at` can contain it.
        let before = self.wide_at(start, at);
        if before.last().is_some_and(|w| at < w.end()) {
            return Err(PositionError::InsideCharacter { offset: at });
        }
        Ok(Position {
            line,
            column: units_over(at.saturating_sub(start), before, encoding),
        })
    }

    /// The byte offset of `position`, whose column counts in `encoding`.
    ///
    /// Refused: a line past the last, a column past the end of its line, a UTF-8 column inside a
    /// multi-byte character and a UTF-16 column inside a surrogate pair.
    ///
    /// Pinned by: `tests/properties.rs::should_refuse_or_round_trip_every_column_when_converting_to_an_offset`
    /// and `text::tests::should_refuse_a_position_when_its_line_is_past_the_end`,
    /// `text::tests::should_refuse_a_column_when_it_is_past_the_line_end`,
    /// `text::tests::should_refuse_a_utf16_column_when_it_is_inside_a_surrogate_pair` and
    /// `text::tests::should_refuse_a_utf8_column_when_it_is_inside_a_multibyte_character`.
    pub fn offset(
        &self,
        position: Position,
        encoding: PositionEncoding,
    ) -> Result<ByteOffset, PositionError> {
        let Position { line, column } = position;
        let (start, end) = self.line_bounds(line).ok_or(PositionError::LinePastEnd {
            line,
            line_count: self.line_count(),
        })?;
        let past_end = || PositionError::ColumnPastLineEnd {
            line,
            column,
            line_length: self.units_between(start, end, encoding),
        };
        let mut units = 0_u32;
        let mut byte = start;
        for w in self.wide_at(start, end) {
            let run = w.offset.saturating_sub(byte);
            let to_wide = units.saturating_add(run);
            if column <= to_wide {
                return Ok(ByteOffset(
                    byte.saturating_add(column.saturating_sub(units)),
                ));
            }
            let width = encoding.units(w.len);
            let past_wide = to_wide.saturating_add(width);
            if column < past_wide {
                return Err(match encoding {
                    PositionEncoding::Utf16 => PositionError::InsideSurrogatePair { line, column },
                    _ => PositionError::InsideCharacter {
                        offset: w.offset.saturating_add(column.saturating_sub(to_wide)),
                    },
                });
            }
            units = past_wide;
            byte = w.end();
        }
        let run = end.saturating_sub(byte);
        if column > units.saturating_add(run) {
            return Err(past_end());
        }
        Ok(ByteOffset(
            byte.saturating_add(column.saturating_sub(units)),
        ))
    }

    /// The range of positions of `range`, columns in `encoding`.
    ///
    /// Pinned by: `text::tests::should_convert_a_range_when_it_spans_lines`.
    pub fn range(
        &self,
        range: ByteRange,
        encoding: PositionEncoding,
    ) -> Result<Range, PositionError> {
        Ok(Range {
            start: self.position(range.start, encoding)?,
            end: self.position(range.end, encoding)?,
        })
    }

    /// The byte range of `range`, whose columns count in `encoding`.
    ///
    /// Pinned by: `text::tests::should_convert_a_range_when_it_spans_lines`.
    pub fn byte_range(
        &self,
        range: Range,
        encoding: PositionEncoding,
    ) -> Result<ByteRange, PositionError> {
        ByteRange::new(
            self.offset(range.start, encoding)?,
            self.offset(range.end, encoding)?,
        )
    }

    /// `position` with its column re-counted from `from` into `to`.
    ///
    /// Pinned by: `tests/properties.rs::should_round_trip_between_encodings_when_the_position_is_valid`.
    pub fn convert(
        &self,
        position: Position,
        from: PositionEncoding,
        to: PositionEncoding,
    ) -> Result<Position, PositionError> {
        self.position(self.offset(position, from)?, to)
    }

    fn line_bounds(&self, line: u32) -> Option<(u32, u32)> {
        let at = usize::try_from(line).ok()?;
        Some((*self.starts.get(at)?, *self.ends.get(at)?))
    }

    /// The wide characters starting in `start..end`.
    fn wide_at(&self, start: u32, end: u32) -> &[Wide] {
        let from = self.wide.partition_point(|w| w.offset < start);
        let to = self.wide.partition_point(|w| w.offset < end);
        self.wide.get(from..to).unwrap_or(&[])
    }

    /// The units of `encoding` in `start..at`, both on character boundaries of one line.
    fn units_between(&self, start: u32, at: u32, encoding: PositionEncoding) -> u32 {
        units_over(at.saturating_sub(start), self.wide_at(start, at), encoding)
    }
}

/// The units of `encoding` in `bytes` bytes that hold exactly the wide characters `wide`.
fn units_over(bytes: u32, wide: &[Wide], encoding: PositionEncoding) -> u32 {
    if encoding == PositionEncoding::Utf8 {
        return bytes;
    }
    wide.iter().fold(bytes, |units, w| {
        units.saturating_sub(u32::from(w.len).saturating_sub(encoding.units(w.len)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(text: &str) -> LineIndex {
        LineIndex::new(text).unwrap()
    }

    #[test]
    fn should_pick_utf8_when_the_client_offers_it() {
        // given
        let offered = ["utf-32", "utf-8", "utf-16"];
        // when
        let picked = PositionEncoding::negotiate(offered);
        // then
        assert_eq!(picked, PositionEncoding::Utf8);
    }

    #[test]
    fn should_fall_back_to_utf16_when_the_client_offers_no_utf8() {
        // given
        let offered = ["utf-32"];
        // when
        let picked = PositionEncoding::negotiate(offered);
        let none = PositionEncoding::negotiate([]);
        // then
        assert_eq!(picked, PositionEncoding::Utf16);
        assert_eq!(none, PositionEncoding::Utf16);
    }

    #[test]
    fn should_round_trip_lsp_names_when_parsing_each_encoding() {
        // given
        let all = [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ];
        // when
        let parsed: Vec<_> = all
            .iter()
            .map(|e| PositionEncoding::from_lsp_name(e.lsp_name()))
            .collect();
        // then
        assert_eq!(parsed, all.map(Some));
        assert_eq!(PositionEncoding::from_lsp_name("UTF-16"), None);
    }

    #[test]
    fn should_count_lines_when_the_text_ends_with_and_without_a_newline() {
        // given
        let texts = ["", "a", "a\n", "a\nb", "a\r\nb\r\n", "a\rb"];
        // when
        let counts: Vec<u32> = texts.iter().map(|t| index(t).line_count()).collect();
        // then
        assert_eq!(counts, [1, 1, 2, 2, 3, 2]);
    }

    #[test]
    fn should_measure_a_line_in_each_encoding_when_it_holds_an_astral_character() {
        // given: `é` is 2 bytes, `😀` 4 bytes and a surrogate pair
        let idx = index("aé😀b\r\nx");
        // when
        let lengths = [
            idx.line_length(0, PositionEncoding::Utf8).unwrap(),
            idx.line_length(0, PositionEncoding::Utf16).unwrap(),
            idx.line_length(0, PositionEncoding::Utf32).unwrap(),
        ];
        // then
        assert_eq!(lengths, [8, 5, 4]);
        assert_eq!(
            idx.line_length(2, PositionEncoding::Utf8),
            Err(PositionError::LinePastEnd {
                line: 2,
                line_count: 2
            })
        );
    }

    #[test]
    fn should_convert_each_encoding_when_a_line_holds_wide_characters() {
        // given
        let idx = index("x\naé😀b");
        let b = ByteOffset::new(9); // `b`: 2 + 1 + 2 + 4
        // when
        let utf8 = idx.position(b, PositionEncoding::Utf8).unwrap();
        let utf16 = idx.position(b, PositionEncoding::Utf16).unwrap();
        let utf32 = idx.position(b, PositionEncoding::Utf32).unwrap();
        // then
        assert_eq!(utf8, Position::new(1, 7));
        assert_eq!(utf16, Position::new(1, 4));
        assert_eq!(utf32, Position::new(1, 3));
        assert_eq!(idx.offset(utf16, PositionEncoding::Utf16), Ok(b));
        assert_eq!(idx.offset(utf32, PositionEncoding::Utf32), Ok(b));
        assert_eq!(idx.offset(utf8, PositionEncoding::Utf8), Ok(b));
    }

    #[test]
    fn should_refuse_an_offset_when_it_is_past_the_end() {
        // given
        let idx = index("ab");
        // when
        let refused = idx.position(ByteOffset::new(3), PositionEncoding::Utf16);
        let end = idx.position(ByteOffset::new(2), PositionEncoding::Utf16);
        // then
        assert_eq!(
            refused,
            Err(PositionError::OffsetPastEnd { offset: 3, len: 2 })
        );
        assert_eq!(end, Ok(Position::new(0, 2)));
    }

    #[test]
    fn should_refuse_an_offset_when_it_splits_a_multibyte_character() {
        // given
        let idx = index("a😀");
        // when
        let refused: Vec<_> = (2..5)
            .map(|o| idx.position(ByteOffset::new(o), PositionEncoding::Utf8))
            .collect();
        // then
        assert_eq!(
            refused,
            [2, 3, 4].map(|offset| Err(PositionError::InsideCharacter { offset }))
        );
    }

    #[test]
    fn should_refuse_an_offset_when_it_is_inside_a_crlf() {
        // given
        let idx = index("a\r\nb");
        // when
        let at_cr = idx.position(ByteOffset::new(1), PositionEncoding::Utf16);
        let at_lf = idx.position(ByteOffset::new(2), PositionEncoding::Utf16);
        let next = idx.position(ByteOffset::new(3), PositionEncoding::Utf16);
        // then
        assert_eq!(at_cr, Ok(Position::new(0, 1)));
        assert_eq!(
            at_lf,
            Err(PositionError::InsideLineTerminator { offset: 2 })
        );
        assert_eq!(next, Ok(Position::new(1, 0)));
    }

    #[test]
    fn should_break_lines_when_a_cr_stands_alone() {
        // given
        let idx = index("a\rb\r");
        // when
        let b = idx.position(ByteOffset::new(2), PositionEncoding::Utf16);
        let end = idx.position(ByteOffset::new(4), PositionEncoding::Utf16);
        // then
        assert_eq!(b, Ok(Position::new(1, 0)));
        assert_eq!(end, Ok(Position::new(2, 0)));
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn should_refuse_a_position_when_its_line_is_past_the_end() {
        // given
        let idx = index("a\n");
        // when
        let last = idx.offset(Position::new(1, 0), PositionEncoding::Utf16);
        let refused = idx.offset(Position::new(2, 0), PositionEncoding::Utf16);
        // then
        assert_eq!(last, Ok(ByteOffset::new(2)));
        assert_eq!(
            refused,
            Err(PositionError::LinePastEnd {
                line: 2,
                line_count: 2
            })
        );
    }

    #[test]
    fn should_refuse_a_column_when_it_is_past_the_line_end() {
        // given: the column of the `\r` is the end; one more would be the terminator
        let idx = index("aé\r\nb");
        // when
        let at_end = idx.offset(Position::new(0, 2), PositionEncoding::Utf16);
        let refused = idx.offset(Position::new(0, 3), PositionEncoding::Utf16);
        let refused_utf8 = idx.offset(Position::new(0, 4), PositionEncoding::Utf8);
        // then
        assert_eq!(at_end, Ok(ByteOffset::new(3)));
        assert_eq!(
            refused,
            Err(PositionError::ColumnPastLineEnd {
                line: 0,
                column: 3,
                line_length: 2
            })
        );
        assert_eq!(
            refused_utf8,
            Err(PositionError::ColumnPastLineEnd {
                line: 0,
                column: 4,
                line_length: 3
            })
        );
    }

    #[test]
    fn should_refuse_a_utf16_column_when_it_is_inside_a_surrogate_pair() {
        // given
        let idx = index("a😀b");
        // when
        let before = idx.offset(Position::new(0, 1), PositionEncoding::Utf16);
        let refused = idx.offset(Position::new(0, 2), PositionEncoding::Utf16);
        let after = idx.offset(Position::new(0, 3), PositionEncoding::Utf16);
        // then
        assert_eq!(before, Ok(ByteOffset::new(1)));
        assert_eq!(
            refused,
            Err(PositionError::InsideSurrogatePair { line: 0, column: 2 })
        );
        assert_eq!(after, Ok(ByteOffset::new(5)));
    }

    #[test]
    fn should_refuse_a_utf8_column_when_it_is_inside_a_multibyte_character() {
        // given
        let idx = index("x\naé");
        // when
        let refused = idx.offset(Position::new(1, 2), PositionEncoding::Utf8);
        // then
        assert_eq!(refused, Err(PositionError::InsideCharacter { offset: 4 }));
    }

    #[test]
    fn should_refuse_a_byte_range_when_it_is_reversed() {
        // given
        let (start, end) = (ByteOffset::new(3), ByteOffset::new(2));
        // when
        let refused = ByteRange::new(start, end);
        let empty = ByteRange::new(end, end).unwrap();
        // then
        assert_eq!(refused, Err(PositionError::ReversedRange));
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn should_refuse_a_usize_offset_when_it_does_not_fit_u32() {
        // given
        let fits = usize::try_from(u32::MAX).unwrap();
        let too_large = fits.checked_add(1).unwrap();
        // when
        let accepted = ByteOffset::try_from_usize(fits);
        let refused = ByteOffset::try_from_usize(too_large);
        // then
        assert_eq!(accepted, Ok(ByteOffset::new(u32::MAX)));
        assert_eq!(refused, Err(PositionError::TextTooLarge { len: too_large }));
    }

    #[test]
    fn should_refuse_a_range_when_it_is_reversed() {
        // given
        let (start, end) = (Position::new(1, 0), Position::new(0, 9));
        // when
        let refused = Range::new(start, end);
        // then
        assert_eq!(refused, Err(PositionError::ReversedRange));
    }

    #[test]
    fn should_convert_a_range_when_it_spans_lines() {
        // given
        let idx = index("ab\r\n😀c");
        let bytes = ByteRange::new(ByteOffset::new(1), ByteOffset::new(8)).unwrap();
        // when
        let range = idx.range(bytes, PositionEncoding::Utf16).unwrap();
        let back = idx.byte_range(range, PositionEncoding::Utf16).unwrap();
        // then
        assert_eq!(range.start(), Position::new(0, 1));
        assert_eq!(range.end(), Position::new(1, 2));
        assert_eq!(back, bytes);
        assert_eq!(idx.line_range(1).map(ByteRange::len), Some(5));
    }

    #[test]
    fn should_refuse_a_cli_position_when_a_coordinate_is_zero() {
        // given
        let coordinates = [(0, 1), (1, 0), (0, 0)];
        // when
        let refused: Vec<_> = coordinates
            .iter()
            .map(|&(l, c)| CliPosition::new(l, c))
            .collect();
        // then
        assert!(refused.iter().all(|r| *r == Err(CliPositionError::Zero)));
        assert_eq!(
            CliPosition::from_utf8_position(Position::new(u32::MAX, 0)),
            Err(CliPositionError::Overflow)
        );
    }

    #[test]
    fn should_print_a_one_based_byte_column_when_a_line_holds_wide_characters() {
        // given: `rg --column` reports `b` of the second line at 2:8 (bytes, 1-based)
        let idx = index("x\naé😀b");
        let b = idx
            .position(ByteOffset::new(9), PositionEncoding::Utf8)
            .unwrap();
        // when
        let cli = CliPosition::from_utf8_position(b).unwrap();
        let parsed = CliPosition::new(2, 8).unwrap().to_utf8_position();
        // then
        assert_eq!(alloc::format!("{cli}"), "2:8");
        assert_eq!(
            idx.offset(parsed, PositionEncoding::Utf8),
            Ok(ByteOffset::new(9))
        );
    }
}
