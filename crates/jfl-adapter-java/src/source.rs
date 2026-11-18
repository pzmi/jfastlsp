//! What the parser reads, and how its offsets map back to the file.
//!
//! **Unicode escapes (JLS §3.3).** Java translates every eligible `\uXXXX` before it lexes, so
//! `\u0041bc` is the identifier `Abc` and `"\u0022"` ends a string. Neither grammar knows this
//! (their `identifier` has no backslash), so [`Translated::new`] does what `javac` does first: it
//! replaces each eligible escape with its character, and keeps an [`OffsetMap`] so every range
//! the extraction reports is a range of the file as written. A file with no `\u` is not copied
//! (the common case: one scan). An escape is eligible when its backslash follows an even number
//! of backslashes. `\uD83D\uDE00` is one character; `\u0000` is left as written (a NUL ends
//! tree-sitter's input); a lone surrogate, which UTF-8 cannot hold, and a malformed escape are
//! left as written too (`javac` refuses both, so a syntax error is right).
//!
//! **Names (JLS §3.8).** [`java_name`] drops the identifier-ignorable characters (the `Cf`
//! format characters such as U+200B and U+FEFF, and the ISO controls JLS lists), so one
//! identifier has one spelling in the summary and a use spelled otherwise still matches its
//! declaration.
//!
//! **A raw NUL byte** (not an escape) is a syntax error where tree-sitter meets it, and the
//! words of a comment it sits in may become reference sites: noisy, but the file carries the
//! diagnostic, so no summary passes for clean (`tests/goldens.rs`'s NUL case).
//!
//! **Line terminators.** A token may end on the `\n` of a `\r\n` (upstream's `line_comment` is
//! `//[^\n]*`, so it ends after the `\r`); `LineIndex` refuses such a position, so
//! [`clamp_crlf`] ends the range at the line's end, before the `\r` (IDX-01's decision).
//!
//! Pinned by: `source::tests` and `tests/goldens.rs` (the `UnicodeEscapes.java` and
//! `CrlfLines.java` fixtures).

use std::borrow::Cow;

/// The text the parser reads: the file's text with its eligible Unicode escapes translated.
#[derive(Debug)]
pub(crate) struct Translated<'a> {
    pub(crate) text: Cow<'a, str>,
    pub(crate) map: OffsetMap,
}

/// Where each translated escape lies in the translated text and in the file.
#[derive(Debug, Default)]
pub(crate) struct OffsetMap {
    /// `(translated start, translated length, file start, file length)`, in order.
    escapes: Vec<(usize, usize, usize, usize)>,
}

impl OffsetMap {
    /// The file offset of translated offset `at`, as the start of a range: inside a translated
    /// character it is the escape's start.
    pub(crate) fn start(&self, at: usize) -> usize {
        self.map(at, false)
    }

    /// The file offset of translated offset `at`, as the end of a range: inside a translated
    /// character it is the escape's end.
    pub(crate) fn end(&self, at: usize) -> usize {
        self.map(at, true)
    }

    fn map(&self, at: usize, end: bool) -> usize {
        let after = self.escapes.partition_point(|&(t, ..)| t <= at);
        let Some(&(t, t_len, f, f_len)) = after.checked_sub(1).and_then(|i| self.escapes.get(i))
        else {
            return at;
        };
        let t_end = t.saturating_add(t_len);
        let f_end = f.saturating_add(f_len);
        if at == t {
            f
        } else if at < t_end {
            if end { f_end } else { f }
        } else {
            f_end.saturating_add(at.saturating_sub(t_end))
        }
    }

    /// Whether no escape was translated.
    pub(crate) fn is_identity(&self) -> bool {
        self.escapes.is_empty()
    }
}

impl<'a> Translated<'a> {
    /// `text` with its eligible Unicode escapes translated.
    pub(crate) fn new(text: &'a str) -> Self {
        let bytes = text.as_bytes();
        if !bytes.windows(2).any(|w| w == b"\\u") {
            return Self {
                text: Cow::Borrowed(text),
                map: OffsetMap::default(),
            };
        }
        let mut out = String::with_capacity(text.len());
        let mut escapes = Vec::new();
        let mut copied = 0_usize;
        let mut at = 0_usize;
        let mut backslashes = 0_usize;
        while let Some(&byte) = bytes.get(at) {
            if byte != b'\\' {
                backslashes = 0;
                at = at.saturating_add(1);
                continue;
            }
            let eligible = backslashes.is_multiple_of(2);
            backslashes = backslashes.saturating_add(1);
            if !eligible {
                at = at.saturating_add(1);
                continue;
            }
            let Some((unit, len)) = escape_at(bytes, at) else {
                at = at.saturating_add(1);
                continue;
            };
            let (ch, len) = match char::from_u32(unit) {
                Some(ch) => (Some(ch), len),
                None => match (0xD800..0xDC00).contains(&unit).then(|| {
                    escape_at(bytes, at.saturating_add(len))
                        .filter(|(low, _)| (0xDC00..0xE000).contains(low))
                }) {
                    Some(Some((low, low_len))) => {
                        let high = unit.saturating_sub(0xD800) << 10;
                        let scalar = high
                            .saturating_add(low.saturating_sub(0xDC00))
                            .saturating_add(0x10000);
                        (char::from_u32(scalar), len.saturating_add(low_len))
                    }
                    _ => (None, len),
                },
            };
            // U+0000 stays as written: tree-sitter's lexers read a NUL as the end of input, and
            // Java never needs it translated (no token holds one; in a literal or a comment the
            // escape parses as written). The corpus has both (`'\u0000'`, `// \u0000 ...`).
            let ch = ch.filter(|&c| c != '\0');
            let Some(ch) = ch else {
                at = at.saturating_add(len);
                backslashes = 0;
                continue;
            };
            out.push_str(text.get(copied..at).unwrap_or(""));
            let translated_at = out.len();
            out.push(ch);
            escapes.push((translated_at, ch.len_utf8(), at, len));
            at = at.saturating_add(len);
            copied = at;
            // A translated backslash never starts another escape (JLS §3.3).
            backslashes = 0;
        }
        if escapes.is_empty() {
            return Self {
                text: Cow::Borrowed(text),
                map: OffsetMap::default(),
            };
        }
        out.push_str(text.get(copied..).unwrap_or(""));
        Self {
            text: Cow::Owned(out),
            map: OffsetMap { escapes },
        }
    }
}

/// The UTF-16 unit and length of the escape `\u...uXXXX` at `at`, if one starts there.
fn escape_at(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let rest = bytes.get(at..)?;
    let after_backslash = rest.get(1..)?;
    let us = after_backslash.iter().take_while(|&&b| b == b'u').count();
    if us == 0 {
        return None;
    }
    let digits = after_backslash.get(us..us.checked_add(4)?)?;
    let mut unit = 0_u32;
    for &digit in digits {
        let value = char::from(digit).to_digit(16)?;
        unit = unit.checked_mul(16)?.checked_add(value)?;
    }
    Some((unit, us.checked_add(5)?))
}

/// Whether `c` is identifier-ignorable (JLS §3.8, `Character.isIdentifierIgnorable`): the ISO
/// controls that are not white space, and the format characters (`Cf`, Unicode 16).
pub(crate) fn is_ignorable(c: char) -> bool {
    const CF: [(u32, u32); 21] = [
        (0x00AD, 0x00AD),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x2064),
        (0x2066, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0001, 0xE0001),
        (0xE0020, 0xE007F),
    ];
    let u = u32::from(c);
    matches!(u, 0x00..=0x08 | 0x0E..=0x1B | 0x7F..=0x9F)
        || CF.iter().any(|&(lo, hi)| (lo..=hi).contains(&u))
}

/// `name` without its identifier-ignorable characters; borrowed when it has none.
pub(crate) fn java_name(name: &str) -> Cow<'_, str> {
    if name.bytes().all(|b| (0x20..0x7F).contains(&b)) || !name.chars().any(is_ignorable) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(name.chars().filter(|&c| !is_ignorable(c)).collect())
    }
}

/// `range` of `text` with an end inside a `\r\n` moved before the `\r`, and a start inside one
/// moved after the `\n` (never past the end).
pub(crate) fn clamp_crlf(text: &[u8], start: usize, end: usize) -> (usize, usize) {
    let inside = |at: usize| {
        at.checked_sub(1).and_then(|before| text.get(before)) == Some(&b'\r')
            && text.get(at) == Some(&b'\n')
    };
    let end = if inside(end) {
        end.saturating_sub(1)
    } else {
        end
    };
    let start = if inside(start) {
        start.saturating_add(1)
    } else {
        start
    };
    (start.min(end), end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_translate_eligible_escapes_when_a_file_spells_names_with_them() {
        // given
        let text = r#"class \u0041b { String s = "\\u0041"; char c = '\uuu0042'; }"#;
        // when
        let translated = Translated::new(text);
        // then
        assert_eq!(
            translated.text,
            r#"class Ab { String s = "\\u0041"; char c = 'B'; }"#
        );
    }

    #[test]
    fn should_map_translated_offsets_back_when_ranges_cover_an_escape() {
        // given: `\u0041` (6 bytes) becomes `A` (1 byte) at file offset 6
        let text = r"class \u0041b {}";
        let translated = Translated::new(text);
        // when
        let name = (translated.map.start(6), translated.map.end(8));
        let brace = (translated.map.start(9), translated.map.end(10));
        let before = (translated.map.start(0), translated.map.end(5));
        // then
        assert_eq!(name, (6, 13));
        assert_eq!(brace, (14, 15));
        assert_eq!(before, (0, 5));
    }

    #[test]
    fn should_join_a_surrogate_pair_and_keep_a_lone_surrogate_when_translating() {
        // given
        let pair = r"\uD83D\uDE00";
        let lone = r"\uD83D x";
        // when
        let joined = Translated::new(pair);
        let kept = Translated::new(lone);
        // then
        assert_eq!(joined.text, "\u{1F600}");
        assert_eq!((joined.map.start(0), joined.map.end(4)), (0, 12));
        assert_eq!(kept.text, lone);
        assert!(kept.map.is_identity());
    }

    #[test]
    fn should_drop_ignorable_characters_when_naming() {
        // given
        let spelled = "fo\u{200B}o\u{FEFF}\u{0007}";
        // when
        let name = java_name(spelled);
        // then
        assert_eq!(name, "foo");
        assert!(matches!(java_name("plain_$1"), Cow::Borrowed(_)));
    }

    #[test]
    fn should_end_a_range_before_the_cr_when_it_ends_inside_a_crlf() {
        // given: `// x\r\n` with the comment ending after the `\r`
        let text = b"// x\r\ny";
        // when
        let comment = clamp_crlf(text, 0, 5);
        let after = clamp_crlf(text, 5, 7);
        // then
        assert_eq!(comment, (0, 4));
        assert_eq!(after, (6, 7));
    }
}
